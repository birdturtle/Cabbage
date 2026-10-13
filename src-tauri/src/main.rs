#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod accounts;
mod auth;
mod clients;
mod downloads;
mod graphics;
mod maintenance;
mod osrs;
mod rs3;
mod runelite;
mod settings;
mod storage;
mod tray;
mod updates;

use auth::{AccountState, Callback, Phase};
use serde::Serialize;
use settings::{Preferences, Settings};
use std::{fs, path::PathBuf, sync::Mutex, time::Duration};
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

struct AppState {
    settings: Mutex<Settings>,
    account: Mutex<AccountState>,
    wallet: Mutex<()>,
    http: reqwest::Client,
    clients: Mutex<clients::Manager>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    settings: Settings,
    accounts: Vec<AccountSummary>,
    selected_account: Option<String>,
    characters: Vec<auth::Character>,
    selected_character: Option<String>,
    signed_in: bool,
    signing_in: bool,
    needs_sign_in: bool,
    account_message: String,
    platform: &'static str,
    clients: Vec<clients::Descriptor>,
    selected_client: clients::ClientId,
    client_available: bool,
    client_running: bool,
    client_busy: bool,
    client_in_use: bool,
    client_files_present: bool,
    client_message: String,
    client_version: Option<String>,
    installed_version: String,
}

#[derive(Serialize)]
struct AccountSummary {
    id: String,
    name: String,
}

fn local(window: &WebviewWindow) -> Result<(), String> {
    let url = window
        .url()
        .map_err(|_| "Cannot validate the launcher window.".to_string())?;
    if window.label() != "main"
        || !(url.scheme() == "tauri"
            || ((url.scheme() == "http" || url.scheme() == "https")
                && url.host_str() == Some("tauri.localhost")))
    {
        return Err("This command is available only in the local launcher.".into());
    }
    Ok(())
}
fn config(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|p| p.join("settings.json"))
        .map_err(|_| "Cannot locate launcher settings.".into())
}
#[tauri::command]
async fn launcher_status(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Status, String> {
    local(&window)?;
    let settings = state.settings.lock().unwrap().clone();
    let account = state.account.lock().unwrap();
    let selected = settings.selected_client();
    let key = account
        .book
        .selected
        .clone()
        .zip(account.book.current().and_then(|s| s.selected.clone()))
        .map(|(account, character)| clients::LaunchKey { account, character });
    let mut manager = state.clients.lock().unwrap();
    manager.reap();
    let running = key.as_ref().is_some_and(|key| manager.is_running(key));
    let running_message = key.as_ref().and_then(|key| manager.running_message(key));
    let mut descriptors = clients::registry();
    let root = client_root(&app)?;
    for descriptor in &mut descriptors {
        descriptor.files_present =
            maintenance::has_files(&root.join(descriptor.id.directory()), descriptor.id);
        descriptor.in_use = manager.client_in_use(descriptor.id);
        let progress = manager.progress(descriptor.id);
        let progress = progress.lock().unwrap();
        descriptor.busy = progress.busy;
        descriptor.message = progress.message.clone();
    }
    let progress = manager.progress(selected);
    let client = progress.lock().unwrap();
    Ok(Status {
        clients: descriptors,
        selected_client: selected,
        client_available: selected
            .unavailable_reason(std::env::consts::OS, std::env::consts::ARCH)
            .is_none(),
        client_running: running,
        client_busy: client.busy,
        client_in_use: manager.client_in_use(selected),
        client_files_present: maintenance::has_files(
            &client_root(&app)?.join(selected.directory()),
            selected,
        ),
        client_message: running_message.unwrap_or_else(|| {
            selected
                .unavailable_reason(std::env::consts::OS, std::env::consts::ARCH)
                .map(str::to_owned)
                .unwrap_or_else(|| client.message.clone())
        }),
        client_version: selected.installed_version(&client_root(&app)?),
        accounts: account
            .book
            .accounts
            .iter()
            .map(|a| AccountSummary {
                id: a.id.clone(),
                name: a
                    .session
                    .characters
                    .iter()
                    .map(|c| c.display_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            })
            .collect(),
        selected_account: account.book.selected.clone(),
        characters: account
            .book
            .current()
            .map(|s| s.characters.clone())
            .unwrap_or_default(),
        selected_character: account.book.current().and_then(|s| s.selected.clone()),
        signed_in: account.book.current().is_some(),
        signing_in: account.attempt.is_some(),
        needs_sign_in: account.book.current().is_some_and(|s| s.needs_sign_in),
        account_message: account.message.clone(),
        settings,
        platform: std::env::consts::OS,
        installed_version: app.package_info().version.to_string(),
    })
}

#[tauri::command]
fn save_preferences(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    preferences: Preferences,
) -> Result<(), String> {
    local(&window)?;
    let mut current = state.settings.lock().unwrap();
    let mut settings = current.clone();
    settings.preferences = preferences;
    persist_settings(&app, &settings)?;
    *current = settings;
    Ok(())
}

fn persist_settings(app: &tauri::AppHandle, settings: &Settings) -> Result<(), String> {
    let path = config(app)?;
    fs::create_dir_all(path.parent().unwrap())
        .map_err(|_| "Cannot create the settings directory.".to_string())?;
    fs::write(path, serde_json::to_vec_pretty(settings).unwrap())
        .map_err(|_| "Cannot save launcher settings.".to_string())
}

// The wallet lock serializes refresh-token rotation with login, switching and
// removal. No account/process mutex is held across network requests.
fn prepare_play(
    app: &tauri::AppHandle,
    target: &clients::LaunchKey,
) -> Result<Vec<(String, String)>, String> {
    let state = app.state::<AppState>();
    let _wallet = state.wallet.lock().unwrap();
    let mut book = load_book(app)?;
    let id = target.account.clone();
    let mut session = book.session_for_launch(&id, &target.character)?.clone();
    if session.needs_sign_in {
        return Err("Sign in again to continue playing.".into());
    }
    let persist = |book: &accounts::AccountBook| -> Result<(), String> {
        // Retain a rotated token in memory even if disk activation fails. A retry
        // must save it before launching, rather than use the superseded token.
        {
            let mut account = state.account.lock().unwrap();
            account.book = book.clone();
            account.loaded = true;
        }
        storage::save(&config(app)?.with_file_name("session.enc"), book)
    };
    let result = tauri::async_runtime::block_on(auth::ensure_session(
        &state.http,
        &mut session,
        &id,
        |current| {
            book.accounts
                .iter_mut()
                .find(|a| a.id == id)
                .unwrap()
                .session = current.clone();
            persist(&book)
        },
    ));
    if let Err(error) = result {
        let message = error.message();
        state.account.lock().unwrap().message = message.clone();
        return Err(message);
    }
    state.account.lock().unwrap().message.clear();
    let character = session
        .characters
        .iter()
        .find(|c| c.account_id == target.character)
        .ok_or("Choose a character before playing.")?;
    Ok(vec![
        ("JX_SESSION_ID".into(), session.session_id.clone()),
        ("JX_CHARACTER_ID".into(), character.account_id.clone()),
        ("JX_DISPLAY_NAME".into(), character.display_name.clone()),
    ])
}

fn client_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|p| p.join("clients"))
        .map_err(|_| "Cannot locate the client data directory.".into())
}

#[tauri::command]
fn select_client(
    window: WebviewWindow,
    app: tauri::AppHandle,
    game: clients::Game,
    client: clients::ClientId,
) -> Result<(), String> {
    local(&window)?;
    let state = app.state::<AppState>();
    let mut settings = state.settings.lock().unwrap();
    let mut updated = settings.clone();
    updated.select(game, client)?;
    persist_settings(&app, &updated)?;
    *settings = updated;
    Ok(())
}

#[tauri::command]
async fn maintain_client(
    window: WebviewWindow,
    app: tauri::AppHandle,
    client: clients::ClientId,
    operation: maintenance::Operation,
) -> Result<String, String> {
    local(&window)?;
    let progress = app
        .state::<AppState>()
        .clients
        .lock()
        .unwrap()
        .begin_maintenance(client)?;
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let root = client_root(&handle)?;
        tauri::async_runtime::block_on(maintenance::run(&root, client, operation, &progress))
    })
    .await
    .map_err(|_| "Client maintenance failed.".to_string())
    .and_then(|result| result);
    let success = match operation {
        maintenance::Operation::Repair => "Client repaired.",
        maintenance::Operation::Uninstall => "Client uninstalled.",
    };
    app.state::<AppState>()
        .clients
        .lock()
        .unwrap()
        .finish_maintenance(client, &result, success);
    result?;
    Ok(success.into())
}

#[tauri::command]
async fn launch_client(window: WebviewWindow, app: tauri::AppHandle) -> Result<String, String> {
    local(&window)?;
    let state = app.state::<AppState>();
    let selected = state.settings.lock().unwrap().selected_client();
    let (key, name) = {
        let account = state.account.lock().unwrap();
        let session = account.book.current().ok_or("Add an account to play.")?;
        let character = session
            .characters
            .iter()
            .find(|c| Some(&c.account_id) == session.selected.as_ref())
            .ok_or("Choose a character to play.")?;
        (
            clients::LaunchKey {
                account: account.book.selected.clone().unwrap(),
                character: character.account_id.clone(),
            },
            character.display_name.clone(),
        )
    };
    let progress = state.clients.lock().unwrap().begin(selected, &key)?;
    let handle = app.clone();
    let target = key.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let state = handle.state::<AppState>();
        let root = client_root(&handle)?;
        // Downloads may take time. Renew the captured account/character immediately before spawning.
        let prepared = tauri::async_runtime::block_on(selected.prepare(&root, &progress))?;
        let credentials = prepare_play(&handle, &target)?;
        let prefer_gpu = state
            .settings
            .lock()
            .unwrap()
            .preferences
            .prefer_dedicated_gpu;
        let child = prepared.spawn(&credentials, prefer_gpu)?;
        state
            .clients
            .lock()
            .unwrap()
            .started(selected, target, name, child);
        Ok(())
    })
    .await;
    let result = result
        .map_err(|_| "The client launch task failed.".to_string())
        .and_then(|r| r);
    state
        .clients
        .lock()
        .unwrap()
        .finish(selected, &key, &result);
    result?;
    let after_play = state.settings.lock().unwrap().preferences.after_play;
    match after_play {
        settings::AfterPlay::KeepOpen => {}
        settings::AfterPlay::HideToTray => {
            if tray::available(&app) {
                let _ = window.hide();
            } else {
                let _ = window.minimize();
            }
        }
        settings::AfterPlay::CloseLauncher => app.exit(0),
    }
    Ok(format!("{} launched.", selected.name()))
}

fn login_failure(app: &tauri::AppHandle, attempt_id: &str, message: String) {
    let changed = {
        let state = app.state::<AppState>();
        let mut account = state.account.lock().unwrap();
        if account.attempt.as_ref().map(|a| a.state.as_str()) == Some(attempt_id) {
            account.attempt = None;
            account.message = message;
            true
        } else {
            false
        }
    };
    if changed {
        if let Some(window) = app.get_webview_window("jagex-auth") {
            let _ = window.close();
        }
    }
}

async fn handle_callback(app: tauri::AppHandle, callback: Callback) {
    let state = app.state::<AppState>();
    let work = {
        let mut account = state.account.lock().unwrap();
        let Some(attempt) = account.attempt.as_mut() else {
            return;
        };
        match attempt.accept_callback(&callback) {
            Ok(true) => Ok(Some((
                attempt.state.clone(),
                attempt.verifier.clone(),
                attempt.nonce.clone(),
                attempt.subject.clone(),
                attempt.refresh_token.clone(),
                attempt.oauth_expires_at,
            ))),
            Ok(false) => Ok(None),
            Err(error) => Err((attempt.state.clone(), error)),
        }
    };
    let (id, verifier, nonce, subject, refresh, oauth_expiry) = match work {
        Ok(Some(work)) => work,
        Ok(None) => return,
        Err((id, error)) => {
            login_failure(&app, &id, error);
            return;
        }
    };
    let result: Result<(), String> = async {
        match callback {
            Callback::Launcher { code, .. } => {
                let tokens = auth::exchange(&state.http, &code, &verifier).await?;
                let claims = auth::launcher_claims(&state.http, &tokens.id_token).await?;
                let next = {
                    let mut account = state.account.lock().unwrap();
                    let Some(attempt) = account.attempt.as_mut().filter(|a| a.state == id) else {
                        return Ok(());
                    };
                    attempt.subject = Some(claims.sub);
                    attempt.oauth_expires_at = auth::now().saturating_add(tokens.expires_in);
                    attempt.refresh_token = Some(tokens.refresh_token);
                    attempt.phase = Phase::Consent;
                    attempt.consent_url(&tokens.id_token)
                };
                if let Some(window) = app.get_webview_window("jagex-auth") {
                    window
                        .navigate(next)
                        .map_err(|_| "Cannot open Jagex consent.".to_string())?;
                }
            }
            Callback::Consent { token, .. } => {
                let claims = auth::consent_claims(&state.http, &token).await?;
                if claims.nonce.as_deref() != Some(&nonce) || Some(&claims.sub) != subject.as_ref()
                {
                    return Err("This identity response does not match your sign-in.".into());
                }
                let mut session = auth::game_session(
                    &state.http,
                    &token,
                    refresh.ok_or("Missing launcher session.")?,
                )
                .await?;
                session.oauth_expires_at = oauth_expiry;
                let handle = app.clone();
                let attempt_id = id.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let state = handle.state::<AppState>();
                    let _wallet = state.wallet.lock().unwrap();
                    if state
                        .account
                        .lock()
                        .unwrap()
                        .attempt
                        .as_ref()
                        .map(|a| a.state.as_str())
                        != Some(attempt_id.as_str())
                    {
                        return Ok::<(), String>(());
                    }
                    let mut book = load_book(&handle)?;
                    book.add(claims.sub, session);
                    storage::save(&config(&handle)?.with_file_name("session.enc"), &book)?;
                    let mut account = state.account.lock().unwrap();
                    account.book = book;
                    account.loaded = true;
                    if account.attempt.as_ref().map(|a| a.state.as_str())
                        == Some(attempt_id.as_str())
                    {
                        account.attempt = None;
                        account.message = "Account added.".into();
                    }
                    Ok(())
                })
                .await
                .map_err(|_| "Cannot save the account.".to_string())??;
                if let Some(window) = app.get_webview_window("jagex-auth") {
                    let _ = window.close();
                }
            }
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        login_failure(&app, &id, error);
    }
}

#[tauri::command]
async fn begin_login(window: WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    local(&window)?;
    if let Some(existing) = app.get_webview_window("jagex-auth") {
        let _ = existing.set_focus();
        return Ok(());
    }
    let attempt = auth::Attempt::new();
    let url = attempt.login_url();
    let id = attempt.state.clone();
    {
        let state = app.state::<AppState>();
        let mut account = state.account.lock().unwrap();
        account.generation += 1;
        account.attempt = Some(attempt);
        account.message = "Complete sign-in in the Jagex window.".into();
    }
    let handle = app.clone();
    let browser_profile = app
        .path()
        .app_data_dir()
        .map_err(|_| "Cannot locate the sign-in browser profile.".to_string())?
        .join("jagex-browser");
    fs::create_dir_all(&browser_profile)
        .map_err(|_| "Cannot create the sign-in browser profile.".to_string())?;
    let builder = WebviewWindowBuilder::new(
        &app,
        "jagex-auth",
        WebviewUrl::External(url::Url::parse("about:blank").unwrap()),
    )
    .title("Sign in to Jagex • Cabbage")
    .inner_size(540.0, 760.0)
    // Keep challenge cookies and browser storage across login redirects and retries.
    // This profile is local to Cabbage and has no launcher capabilities.
    .data_directory(browser_profile)
    .devtools(false)
    .on_navigation(move |url| {
        // No remote window gets launcher IPC. Intercept callbacks before a network request.
        match auth::callback(url) {
            Ok(Some(callback)) => {
                let app = handle.clone();
                tauri::async_runtime::spawn(handle_callback(app, callback));
                false
            }
            Err(error) => {
                let app = handle.clone();
                let state = app.state::<AppState>();
                let id = state
                    .account
                    .lock()
                    .unwrap()
                    .attempt
                    .as_ref()
                    .map(|a| a.state.clone());
                if let Some(id) = id {
                    tauri::async_runtime::spawn(async move {
                        login_failure(&app, &id, error);
                    });
                }
                false
            }
            Ok(None) => {
                let allowed = auth::allow_browser_navigation(url);
                // Never print paths, query strings, fragments, or auth responses.
                eprintln!(
                    "Jagex browser navigation: scheme={} host={} allowed={}",
                    url.scheme(),
                    url.host_str().unwrap_or("(none)"),
                    allowed
                );
                allowed
            }
        }
    });
    match builder.build() {
        Ok(auth_window) => {
            #[cfg(target_os = "linux")]
            auth_window
                .with_webview(|webview| {
                    use webkit2gtk::WebViewExt;
                    webview
                        .inner()
                        .connect_load_failed(|_, _, failed_uri, error| {
                            let host = url::Url::parse(failed_uri)
                                .ok()
                                .and_then(|u| u.host_str().map(str::to_owned))
                                .unwrap_or_default();
                            // Never log redirect paths, codes, tokens, or page content.
                            eprintln!(
                                "Jagex browser load failed: host={} domain={:?} network_error={:?}",
                                host,
                                error.domain(),
                                error.kind::<webkit2gtk::NetworkError>()
                            );
                            false
                        });
                })
                .map_err(|_| "Cannot initialize the sign-in browser.".to_string())?;
            auth_window
                .navigate(url)
                .map_err(|_| "Cannot open Jagex sign-in.".to_string())?;
            let app = app.clone();
            auth_window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    login_failure(
                        &app,
                        &id,
                        "Sign-in window closed. You can try again.".into(),
                    );
                }
            });
            Ok(())
        }
        Err(_) => {
            let state = app.state::<AppState>();
            state.account.lock().unwrap().attempt = None;
            Err("The Jagex sign-in window could not open.".into())
        }
    }
}

#[tauri::command]
fn cancel_login(window: WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    local(&window)?;
    let state = app.state::<AppState>();
    {
        let mut account = state.account.lock().unwrap();
        account.generation += 1;
        account.attempt = None;
        account.message = "Sign-in cancelled.".into();
    }
    if let Some(window) = app.get_webview_window("jagex-auth") {
        let _ = window.close();
    }
    Ok(())
}

// Called only while the wallet mutex serializes all account changes.
fn load_book(app: &tauri::AppHandle) -> Result<accounts::AccountBook, String> {
    let state = app.state::<AppState>();
    let account = state.account.lock().unwrap();
    if account.loaded {
        return Ok(account.book.clone());
    }
    drop(account);
    Ok(storage::restore(&config(app)?.with_file_name("session.enc"))?.unwrap_or_default())
}

#[tauri::command]
async fn load_accounts(window: WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    local(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _wallet = state.wallet.lock().unwrap();
        let book = load_book(&app)?;
        let mut account = state.account.lock().unwrap();
        account.book = book;
        account.loaded = true;
        Ok(())
    })
    .await
    .map_err(|_| "Cannot load saved accounts.".to_string())?
}

async fn edit_accounts(
    app: tauri::AppHandle,
    edit: impl FnOnce(&mut accounts::AccountBook) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _wallet = state.wallet.lock().unwrap();
        let mut book = load_book(&app)?;
        edit(&mut book)?;
        storage::save(&config(&app)?.with_file_name("session.enc"), &book)?;
        let mut account = state.account.lock().unwrap();
        account.book = book;
        account.loaded = true;
        account.message.clear();
        Ok(())
    })
    .await
    .map_err(|_| "Cannot save account changes.".to_string())?
}

#[tauri::command]
async fn select_account(
    window: WebviewWindow,
    app: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    local(&window)?;
    edit_accounts(app, move |book| book.select(&id)).await
}

#[tauri::command]
async fn remove_account(
    window: WebviewWindow,
    app: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    local(&window)?;
    edit_accounts(app, move |book| book.remove(&id)).await
}

#[tauri::command]
async fn select_character(
    window: WebviewWindow,
    app: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    local(&window)?;
    edit_accounts(app, move |book| {
        let session = book
            .current_mut()
            .ok_or("Add an account to choose a character.")?;
        if !session.characters.iter().any(|c| c.account_id == id) {
            return Err("Character is not in this account.".into());
        }
        session.selected = Some(id);
        Ok(())
    })
    .await
}

#[tauri::command]
async fn check_updates(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<updates::Release, String> {
    local(&window)?;
    let state = app.state::<AppState>();
    let beta = state
        .settings
        .lock()
        .unwrap()
        .preferences
        .include_beta_updates;
    let release = updates::latest(&state.http, beta).await?;
    updates::describe(release, &app.package_info().version.to_string())
}

fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--rs3-host")) {
        let args: Vec<_> = std::env::args_os().collect();
        let result = if args.len() == 4 {
            rs3::host(
                std::path::Path::new(&args[2]),
                std::path::Path::new(&args[3]),
            )
        } else {
            Err("Invalid RuneScape host invocation.".into())
        };
        match result {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }

    // GTK's Wayland window ID derives from the program name, even when the
    // GtkApplication has its own ID. Match the installed desktop entry.
    #[cfg(target_os = "linux")]
    gtk::glib::set_prgname(Some("com.birdturtle.cabbage"));
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            let _ = args;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            #[cfg(target_os = "linux")]
            {
                use gtk::prelude::*;
                gtk::Window::set_default_icon_name("com.birdturtle.cabbage");
                if let Some(window) = app.get_webview_window("main") {
                    window
                        .gtk_window()?
                        .set_icon_name(Some("com.birdturtle.cabbage"));
                }
            }
            let settings = config(app.handle())
                .ok()
                .and_then(|path| fs::read(path).ok())
                .and_then(|data| serde_json::from_slice::<Settings>(&data).ok())
                .unwrap_or_default();
            app.manage(AppState {
                settings: Mutex::new(settings),
                account: Mutex::new(AccountState::default()),
                wallet: Mutex::new(()),
                clients: Mutex::new(clients::Manager::default()),
                http: reqwest::Client::builder()
                    .timeout(Duration::from_secs(25))
                    .redirect(reqwest::redirect::Policy::none())
                    .user_agent("Cabbage/0.1")
                    .build()?,
            });
            if let Err(error) = tray::setup(app) {
                eprintln!("Cannot create launcher tray: {error}");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main"
                    && tray::available(window.app_handle())
                    && window.hide().is_ok()
                {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            launcher_status,
            launch_client,
            maintain_client,
            select_client,
            save_preferences,
            begin_login,
            cancel_login,
            load_accounts,
            select_account,
            remove_account,
            select_character,
            check_updates
        ])
        .run(tauri::generate_context!())
        .expect("Launcher could not start");
}
