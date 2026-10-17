#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod logs;
mod game_host;
mod installation;
mod accounts;
mod auth;
mod clients;
mod downloads;
mod dependencies;
mod graphics;
mod maintenance;
mod osrs;
mod rs3;
mod runelite;
mod developer_plugins;
mod developer_profile;
mod settings;
mod storage;
mod tray;
mod updates;
mod update_install;

use auth::{AccountState, Callback, Phase};
use serde::Serialize;
use settings::{Preferences, Settings};
use std::{fs, path::PathBuf, sync::Mutex, time::Duration};
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

struct AppState {
    settings: Mutex<Settings>,
    account: Mutex<AccountState>,
    wallet: Mutex<()>,
    update: update_install::SharedProgress,
    dependencies: dependencies::Shared,
    http: reqwest::Client,
    clients: Mutex<clients::Manager>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    requirements: dependencies::Status,
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
    local_origin(window)?;
    if let Some(state) = window.app_handle().try_state::<AppState>() {
        if state.update.lock().unwrap().busy { return Err("Wait for the launcher update to finish.".into()); }
    }
    Ok(())
}
fn local_origin(window: &WebviewWindow) -> Result<(), String> {
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
    let requirements = dependencies::status(&state.dependencies, selected);
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
        if cfg!(windows) && descriptor.id == clients::ClientId::OfficialOsrs && !state.dependencies.lock().unwrap().vc_ready {
            descriptor.available = false;
            descriptor.unavailable_reason = Some("Requirements missing.");
        }
        #[cfg(target_os="linux")]
        if state.dependencies.lock().unwrap().plans.iter().any(|plan| plan.client==dependencies::linux::client(descriptor.id) && plan.supported && !plan.missing.is_empty()) {
            descriptor.available=false; descriptor.unavailable_reason=Some("Requirements missing.");
        }
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
        client_available: !requirements.required && selected.unavailable_reason(std::env::consts::OS, std::env::consts::ARCH).is_none(),
        requirements,
        clients: descriptors,
        selected_client: selected,
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
        client_version: if selected == clients::ClientId::Runelite {
            selected.installed_version(&client_root(&app)?).and_then(|_| app.path().home_dir().ok().and_then(|home| runelite::client_version(&home)))
        } else {
            selected.installed_version(&client_root(&app)?)
        },
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

// The file picker is exposed only through the authenticated local launcher window.
#[tauri::command]
async fn add_developer_plugin(window: WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_dialog::DialogExt;
    local(&window)?;
    let handle = app.clone();
    let plugin = tauri::async_runtime::spawn_blocking(
        move || -> Result<Option<developer_plugins::LocalPlugin>, String> {
            let file = handle
                .dialog()
                .file()
                .add_filter("RuneLite plugin JAR", &["jar"])
                .blocking_pick_file();
            match file {
                Some(file) => Ok(Some(developer_plugins::inspect(
                    &file.into_path().map_err(|_| "Choose a local JAR file.")?,
                )?)),
                None => Ok(None),
            }
        },
    )
    .await
    .map_err(|_| "Could not inspect the selected JAR.")??;
    if let Some(plugin) = plugin {
        let state = app.state::<AppState>();
        let mut current = state.settings.lock().unwrap();
        let mut updated = current.clone();
        if updated
            .developer_plugins
            .jars
            .iter()
            .any(|p| p.path == plugin.path || p.sha256 == plugin.sha256)
        {
            return Err("This plugin JAR is already listed.".into());
        }
        if updated.developer_plugins.jars.len() >= 16 {
            return Err("Remove a plugin before adding another (maximum 16).".into());
        }
        updated.developer_plugins.jars.push(plugin);
        persist_settings(&app, &updated)?;
        *current = updated;
    }
    Ok(())
}
#[tauri::command]
fn configure_developer_plugins(
    window: WebviewWindow,
    app: tauri::AppHandle,
    enabled: bool,
    plugin: Option<String>,
    remove: Option<bool>,
    plugin_enabled: Option<bool>,
) -> Result<(), String> {
    local(&window)?;
    let state = app.state::<AppState>();
    let mut current = state.settings.lock().unwrap();
    let mut updated = current.clone();
    updated.developer_plugins.enabled = enabled;
    if let Some(hash) = plugin {
        let index = updated
            .developer_plugins
            .jars
            .iter()
            .position(|p| p.sha256 == hash)
            .ok_or("Plugin is no longer listed.")?;
        if remove.unwrap_or(false) {
            updated.developer_plugins.jars.remove(index);
        } else if let Some(value) = plugin_enabled {
            updated.developer_plugins.jars[index].enabled = value;
        }
    }
    persist_settings(&app, &updated)?;
    *current = updated;
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

fn log_path(app: &tauri::AppHandle, kind: logs::Kind) -> Result<PathBuf, String> {
    Ok(kind.path(&client_root(app)?, &app.path().home_dir().map_err(|_| "Cannot locate home directory.")?))
}
#[tauri::command]
fn read_log(window: WebviewWindow, app: tauri::AppHandle, state: tauri::State<'_, AppState>, kind: logs::Kind) -> Result<logs::View, String> {
    local(&window)?;
    let mut view = logs::read(&log_path(&app, kind)?)?;
    let account = state.account.lock().unwrap();
    for saved in &account.book.accounts {
        for secret in [Some(saved.session.session_id.as_str()), Some(saved.session.refresh_token.as_str()), saved.session.oauth_id_token.as_deref()].into_iter().flatten() {
            if !secret.is_empty() { view.text = view.text.replace(secret, "[credential omitted]"); }
        }
    }
    Ok(view)
}
#[tauri::command]
fn open_log_folder(window: WebviewWindow, app: tauri::AppHandle, kind: logs::Kind) -> Result<(), String> {
    local(&window)?;
    let path = log_path(&app, kind)?;
    let folder = path.parent().ok_or("Cannot locate log folder.")?;
    fs::create_dir_all(folder).map_err(|_| "Cannot create log folder.")?;
    #[cfg(windows)]
    let mut command = std::process::Command::new("explorer.exe");
    #[cfg(not(windows))]
    let mut command = std::process::Command::new("xdg-open");
    command.arg(folder);
    clients::configure_credentials(&mut command, &[]);
    for (name, _) in std::env::vars_os() { if name.to_string_lossy().starts_with("JX_") { command.env_remove(name); } }
    for name in ["LD_LIBRARY_PATH", "LD_PRELOAD", "APPIMAGE", "APPDIR", "ARGV0"] { command.env_remove(name); }
    command.spawn().map_err(|_| "Cannot open the log folder.")?;
    Ok(())
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
    if state.dependencies.lock().unwrap().busy { return Err("Wait for dependency setup to finish.".into()); }
    #[cfg(windows)]
    if selected == clients::ClientId::OfficialOsrs && !dependencies::windows::vc_ready() { return Err("Install Microsoft Visual C++ Runtime to use this client.".into()); }
    #[cfg(target_os="linux")]
    { let plan=cabbage_dependency_helper::probe(dependencies::linux::client(selected)); if plan.supported && !plan.missing.is_empty() { return Err("Install the missing requirements before playing. Select Check again after setup.".into()); } }
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
        let developer = state.settings.lock().unwrap().developer_plugins.clone();
        if developer_plugins::enabled_for(&developer, selected) {
            developer_plugins::preflight(&developer)?;
        }
        // Downloads may take time. Renew the captured account/character immediately before spawning.
        let prepared = tauri::async_runtime::block_on(selected.prepare(&root, &progress))?;
        let developer_runtime = match &prepared {
            clients::PreparedClient::RuneLite {root, ..} if developer_plugins::enabled_for(&developer, selected) =>
                Some(tauri::async_runtime::block_on(developer_plugins::prepare(root, &developer, &progress, &handle.path().home_dir().map_err(|_| "Cannot locate RuneLite settings.")?))?),
            _ => None,
        };
        let credentials = prepare_play(&handle, &target)?;
        let prefer_gpu = state
            .settings
            .lock()
            .unwrap()
            .preferences
            .prefer_dedicated_gpu;
        let child = match (prepared, developer_runtime) {
            (clients::PreparedClient::RuneLite {root, install}, Some(runtime)) =>
                developer_plugins::spawn(&root, &install, &runtime, &credentials, prefer_gpu)?,
            (prepared, _) => prepared.spawn(&credentials, prefer_gpu)?,
        };
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

#[tauri::command]
fn acknowledge_update(window: WebviewWindow, app: tauri::AppHandle) -> Result<Option<String>, String> {
    local_origin(&window)?;
    let layout = installation_layout(&app)?;
    if cfg!(windows) && (std::env::var_os("CABBAGE_UPDATE_ACK").is_some() || std::env::var_os("CABBAGE_UPDATE_ROLLBACK").is_some()) && installation::status(&layout).running_installed {
        if let Some(source) = installation::source() { installation::install(&layout, &source)?; }
    }
    update_install::acknowledge_startup();
    std::env::remove_var("CABBAGE_UPDATE_ROLLBACK");
    let error = installation::source().map(|p| p.with_extension("update-error"));
    let message = error.as_ref().and_then(|p| fs::read_to_string(p).ok()).filter(|s| s.len() < 4096);
    if let Some(path) = error { let _ = fs::remove_file(path); }
    Ok(message)
}
#[tauri::command]
fn update_progress(window: WebviewWindow, app: tauri::AppHandle) -> Result<update_install::Progress, String> {
    local_origin(&window)?;
    Ok(app.state::<AppState>().update.lock().unwrap().clone())
}
fn update_idle(state: &AppState) -> Result<(), String> {
    if state.dependencies.lock().unwrap().busy { return Err("Wait for dependency setup to finish before updating Cabbage.".into()); }
    let _wallet = state.wallet.try_lock().map_err(|_| "Wait for account changes to finish, then retry.")?;
    if state.account.lock().unwrap().attempt.is_some() { return Err("Finish signing in before updating Cabbage.".into()); }
    let mut clients = state.clients.lock().unwrap();
    for client in clients::registry() {
        if clients.progress(client.id).lock().unwrap().busy { return Err("Wait for the game download or installation to finish, then retry.".into()); }
    }
    Ok(())
}
#[tauri::command]
async fn install_update(window: WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    local(&window)?;
    let state = app.state::<AppState>();
    let target = update_install::target()?;
    {
        let mut progress = state.update.lock().unwrap();
        if progress.busy { return Err("An update is already in progress.".into()); }
        update_idle(&state)?;
        *progress = update_install::Progress { busy: true, percent: None, message: "Checking update signature…".into() };
    }
    let result: Result<(), String> = async {
        let beta = state.settings.lock().unwrap().preferences.include_beta_updates;
        let release = updates::latest(&state.http, beta).await?;
        let v = updates::version(&release.tag_name)?;
        if v <= updates::version(env!("CARGO_PKG_VERSION"))? { return Err("You have the latest version.".into()); }
        let http = update_install::http()?;
        let (staged, sha256) = update_install::stage(&http, &release.tag_name, &target, &state.update).await?;
        let apply = (|| {
            update_idle(&state)?;
            update_install::apply(target, staged.clone(), sha256, v.to_string(), &client_root(&app)?.join("launcher-update"))
        })();
        if apply.is_err() { let _ = fs::remove_file(staged); }
        apply?;
        state.update.lock().unwrap().message = "Restarting Cabbage…".into();
        app.exit(0);
        Ok(())
    }.await;
    if let Err(error) = &result {
        *state.update.lock().unwrap() = update_install::Progress { busy: false, percent: None, message: error.clone() };
    }
    result
}

#[tauri::command]
fn recheck_requirements(window: WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    local(&window)?;
    let state = app.state::<AppState>();
    let mut dependencies = state.dependencies.lock().unwrap();
    if dependencies.busy { return Err("Finish dependency setup before checking again.".into()); }
    #[cfg(windows)] { dependencies.vc_ready = dependencies::windows::vc_ready(); }
    #[cfg(target_os="linux")] { dependencies.plans=dependencies::linux::probe(); }
    dependencies.message.clear();
    Ok(())
}
#[tauri::command]
async fn install_requirements(window: WebviewWindow, app: tauri::AppHandle, client: clients::ClientId) -> Result<String, String> {
    local(&window)?;
    #[cfg(not(any(windows,target_os="linux")))] { let _ = (app, client); return Err("Dependency setup is not supported on this platform.".into()); }
    #[cfg(target_os="linux")] {
        let state=app.state::<AppState>(); update_idle(&state)?;
        {
            let mut dependencies=state.dependencies.lock().unwrap();
            if dependencies.busy { return Err("Dependency setup is already running.".into()); }
            dependencies.busy=true; dependencies.message="Opening terminal setup…".into(); dependencies.message_client=Some(client);
        }
        let handle=app.clone();
        let result=tauri::async_runtime::spawn_blocking(move || {
            let root=handle.path().app_data_dir().map_err(|_|"Cannot locate setup folder.")?.join("dependency-setup");
            dependencies::linux::install(client,&root,&handle.state::<AppState>().dependencies)
        }).await.map_err(|_|"Dependency setup failed.".to_string()).and_then(|result|result);
        let mut dependencies=state.dependencies.lock().unwrap();
        dependencies.busy=false; dependencies.plans=dependencies::linux::probe();
        dependencies.message=match &result {Ok(message)=>message.clone(),Err(error)=>error.clone()};
        result
    }
    #[cfg(windows)] {
        if client != clients::ClientId::OfficialOsrs || std::env::consts::ARCH != "x86_64" { return Err("No Windows dependency installer is available for this client.".into()); }
        let state = app.state::<AppState>();
        update_idle(&state)?;
        {
            let mut dependencies = state.dependencies.lock().unwrap();
            if dependencies.busy { return Err("Dependency setup is already running.".into()); }
            if dependencies::windows::vc_ready() { dependencies.vc_ready=true; dependencies.message.clear(); return Ok("Requirements are already installed.".into()); }
            dependencies.busy=true; dependencies.message="Preparing requirements…".into(); dependencies.message_client=Some(client);
        }
        let handle=app.clone();
        let result=tauri::async_runtime::spawn_blocking(move || {
            let root=handle.path().app_local_data_dir().map_err(|_| "Cannot locate dependency download folder.")?.join("dependency-setup");
            dependencies::windows::install(dependencies::Installer::VisualCpp,&root,|message| handle.state::<AppState>().dependencies.lock().unwrap().message=message.into())
        }).await.map_err(|_| "Dependency setup failed.".to_string()).and_then(|result|result);
        let mut dependencies=state.dependencies.lock().unwrap();
        dependencies.busy=false; dependencies.vc_ready=dependencies::windows::vc_ready();
        dependencies.message=match &result {
            Ok(true)=>"Restart Windows to finish installing requirements.".into(),
            Ok(false) if dependencies.vc_ready=>"Requirements installed. Ready to play.".into(),
            Ok(false)=>"Setup finished, but requirements are still missing. Select Check again or retry installation.".into(),
            Err(error)=>error.clone(),
        };
        result?;
        Ok(dependencies.message.clone())
    }
}

fn installation_layout(app: &tauri::AppHandle) -> Result<installation::Layout, String> {
    #[cfg(windows)] {
        Ok(installation::Layout::windows(
            &app.path().local_data_dir().map_err(|_| "Cannot locate installation directory.")?,
            &app.path().data_dir().map_err(|_| "Cannot locate Start Menu directory.")?,
        ))
    }
    #[cfg(not(windows))] {
        Ok(installation::Layout::new(&app.path().data_dir().map_err(|_| "Cannot locate application directory.")?))
    }
}
#[tauri::command]
fn installation_status(window: WebviewWindow, app: tauri::AppHandle) -> Result<installation::Status, String> {
    local(&window)?;
    Ok(installation::status(&installation_layout(&app)?))
}
#[tauri::command]
async fn install_launcher(window: WebviewWindow, app: tauri::AppHandle) -> Result<String, String> {
    local(&window)?;
    let layout = installation_layout(&app)?;
    let source = installation::source().ok_or("Install Cabbage from its launcher download.")?;
    tauri::async_runtime::spawn_blocking(move || installation::install(&layout, &source)).await.map_err(|_| "Installation failed.")??;
    Ok(if cfg!(windows) { "Installed. Open Cabbage from the Start Menu." } else { "Installed. Open Cabbage from your application menu." }.into())
}
#[tauri::command]
async fn uninstall_launcher(window: WebviewWindow, app: tauri::AppHandle) -> Result<String, String> {
    local(&window)?;
    let layout = installation_layout(&app)?;
    let must_exit = tauri::async_runtime::spawn_blocking(move || installation::uninstall(&layout)).await.map_err(|_| "Uninstall failed.")??;
    if must_exit { app.exit(0); }
    Ok("Cabbage uninstalled. Saved accounts and games are kept.".into())
}
#[tauri::command]
fn keep_portable(window: WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    local(&window)?;
    installation::keep_portable(&installation_layout(&app)?)
}

fn main() {
    #[cfg(windows)]
    if !dependencies::windows::startup() { return; }
    // GTK's Wayland window ID derives from the program name, even when the
    // GtkApplication has its own ID. Match the installed desktop entry.
    #[cfg(target_os = "linux")]
    gtk::glib::set_prgname(Some("com.birdturtle.cabbage"));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            #[cfg(windows)]
            if args.iter().any(|arg| arg == "--uninstall-launcher") {
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    match installation_layout(&app).and_then(|layout| installation::uninstall(&layout)) {
                        Ok(_) => app.exit(0),
                        Err(error) => eprintln!("{error}"),
                    }
                });
                return;
            }
            let _ = args;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            #[cfg(windows)]
            if std::env::args_os().any(|arg| arg == "--uninstall-launcher") {
                if let Some(window) = app.get_webview_window("main") { let _ = window.hide(); }
                let layout = installation_layout(app.handle())?;
                installation::uninstall(&layout)?;
                app.handle().exit(0);
                return Ok(());
            }
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
                update: Default::default(),
                dependencies: dependencies::initial(),
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
            installation_status,
            install_launcher,
            uninstall_launcher,
            keep_portable,
            launcher_status,
            read_log,
            open_log_folder,
            launch_client,
            maintain_client,
            select_client,
            save_preferences,
            add_developer_plugin,
            configure_developer_plugins,
            begin_login,
            cancel_login,
            load_accounts,
            select_account,
            remove_account,
            select_character,
            check_updates,
            recheck_requirements,
            install_requirements,
            install_update,
            update_progress,
            acknowledge_update
        ])
        .run(tauri::generate_context!())
        .expect("Launcher could not start");
}
