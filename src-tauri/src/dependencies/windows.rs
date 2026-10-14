use super::*;
use std::os::windows::{ffi::OsStringExt, fs::OpenOptionsExt, process::CommandExt};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    System::{
        Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ},
        SystemInformation::GetSystemDirectoryW,
    },
    UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONERROR, MB_ICONQUESTION, MB_OK, MB_YESNO},
};
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn system_directory() -> Option<PathBuf> {
    let mut buffer = vec![0u16; 32768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    (length > 0 && length < buffer.len())
        .then(|| PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length])))
}
fn registry_string(hive: HKEY, path: &str, name: &str) -> Option<String> {
    let path = wide(path);
    let name = wide(name);
    let mut buffer = [0u16; 256];
    let mut bytes = 512u32;
    let result = unsafe {
        RegGetValueW(
            hive,
            path.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if result != 0 || bytes < 2 || bytes > 512 {
        return None;
    }
    String::from_utf16(&buffer[..bytes as usize / 2])
        .ok()
        .map(|v| v.trim_end_matches('\0').to_owned())
}
fn valid_version(value: &str) -> bool {
    let parts: Option<Vec<u32>> = value.split('.').map(|v| v.parse().ok()).collect();
    parts.is_some_and(|parts| parts.len() == 4 && parts.iter().any(|v| *v > 0))
}
pub fn webview_ready() -> bool {
    [(HKEY_CURRENT_USER,r"Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"),
     (HKEY_LOCAL_MACHINE,r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"),
     (HKEY_LOCAL_MACHINE,r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}")]
     .into_iter().any(|(hive,path)| registry_string(hive,path,"pv").is_some_and(|v| valid_version(&v)))
}
pub fn vc_ready() -> bool {
    system_directory().is_some_and(|root| {
        [
            "msvcp140.dll",
            "vcruntime140.dll",
            "vcruntime140_1.dll",
            "vcomp140.dll",
        ]
        .iter()
        .all(|name| root.join(name).is_file())
    })
}
fn message(text: &str, flags: u32) -> i32 {
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Cabbage").as_ptr(),
            flags,
        )
    }
}
fn powershell(script: &str) -> Result<Command, String> {
    let system = system_directory().ok_or("Cannot locate Windows system tools.")?;
    let encoded: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    use base64::Engine;
    let mut command = Command::new(system.join("WindowsPowerShell/v1.0/powershell.exe"));
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &base64::engine::general_purpose::STANDARD.encode(encoded),
        ])
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::clients::configure_credentials(&mut command, &[]);
    Ok(command)
}
fn wait(mut command: Command, timeout: Duration) -> Result<u32, String> {
    let mut child = command
        .spawn()
        .map_err(|_| "Cannot start Windows dependency setup.")?;
    let deadline = Instant::now() + timeout;
    loop {
        match child
            .try_wait()
            .map_err(|_| "Cannot track Windows dependency setup.")?
        {
            Some(exit) => return Ok(exit.code().unwrap_or(1) as u32),
            None if Instant::now() >= deadline => {
                // Do not terminate an elevated installer during a system transaction.
                return Err("Setup is still running. Finish the Windows installer, then select Check again.".into());
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}
const VERIFY: &str = r#"
$ErrorActionPreference='Stop'
$s=Get-AuthenticodeSignature -LiteralPath $env:CABBAGE_DEPENDENCY_FILE
if($s.Status -ne 'Valid' -or $s.SignerCertificate.Subject -notmatch '(^|,\s*)O=Microsoft Corporation(,|$)'){exit 1}
"#;
const INSTALL: &str = r#"
$ErrorActionPreference='Stop'
try {
  $options=@{FilePath=$env:CABBAGE_DEPENDENCY_FILE;ArgumentList=$env:CABBAGE_DEPENDENCY_ARGS;Wait=$true;PassThru=$true}
  if($env:CABBAGE_DEPENDENCY_ELEVATE -eq '1'){$options.Verb='RunAs'}
  $process=Start-Process @options
  exit $process.ExitCode
}catch{
  $errorCode=$_.Exception.NativeErrorCode
  if($errorCode -eq 1223 -or $_.Exception.InnerException.NativeErrorCode -eq 1223){exit 1602}
  exit 1
}
"#;
fn verify(path: &Path) -> Result<(), String> {
    let mut command = powershell(VERIFY)?;
    command.env("CABBAGE_DEPENDENCY_FILE", path);
    if wait(command, Duration::from_secs(90))? != 0 {
        return Err(
            "The installer's Microsoft signature could not be verified. Nothing was installed."
                .into(),
        );
    }
    Ok(())
}
pub fn install(installer: Installer, root: &Path, progress: impl Fn(&str)) -> Result<bool, String> {
    fs::create_dir_all(root).map_err(|_| "Cannot create dependency download folder.")?;
    let folder = root.join(format!("setup-{:x}", rand::random::<u64>()));
    fs::create_dir(&folder).map_err(|_| "Cannot stage dependency setup.")?;
    let path = folder.join("MicrosoftSetup.exe");
    let result = (|| {
        progress("Downloading requirements from Microsoft…");
        let bytes = tauri::async_runtime::block_on(async {
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(180))
                .connect_timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() >= 8 || !trusted_url(attempt.url()) {
                        attempt.error("Download is outside Microsoft servers")
                    } else {
                        attempt.follow()
                    }
                }))
                .build()
                .map_err(|_| "Cannot initialize dependency download.")?;
            let response = http
                .get(installer.url())
                .send()
                .await
                .map_err(|_| "Cannot download requirements. Check your connection and retry.")?
                .error_for_status()
                .map_err(|_| "Microsoft could not provide the installer. Retry.")?;
            crate::downloads::read_limited(response, 128 * 1024 * 1024).await
        })?;
        fs::write(&path, bytes).map_err(|_| "Cannot save dependency installer.")?;
        // Deny modification and deletion between signature verification and execution.
        let _guard = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .map_err(|_| "Cannot protect dependency installer.")?;
        progress("Verifying Microsoft's installer…");
        verify(&path)?;
        progress("Installing requirements. Complete any Windows permission prompt…");
        let mut command = powershell(INSTALL)?;
        command
            .env("CABBAGE_DEPENDENCY_FILE", &path)
            .env("CABBAGE_DEPENDENCY_ARGS", installer.arguments())
            .env(
                "CABBAGE_DEPENDENCY_ELEVATE",
                if matches!(installer, Installer::VisualCpp) {
                    "1"
                } else {
                    "0"
                },
            );
        installer_result(wait(command, Duration::from_secs(15 * 60))?)
    })();
    // An active installer may keep this file locked; leave it until a later cleanup.
    let _ = fs::remove_dir_all(folder);
    result
}
pub fn startup() -> bool {
    if webview_ready() {
        return true;
    }
    if message(
        "Cabbage needs Microsoft Edge WebView2 Runtime. Download and install it now?",
        MB_YESNO | MB_ICONQUESTION,
    ) != IDYES
    {
        return false;
    }
    let Some(root) =
        system_directory().and_then(|_| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
    else {
        message("Cannot locate the download folder. Install WebView2 from Microsoft and reopen Cabbage.",MB_OK|MB_ICONERROR);
        return false;
    };
    match install(
        Installer::WebView,
        &root.join("com.birdturtle.cabbage/dependency-setup"),
        |_| {},
    ) {
        Ok(_) if webview_ready() => true,
        Ok(restart) => {
            message(
                if restart {
                    "Restart Windows to finish WebView2 setup, then reopen Cabbage."
                } else {
                    "WebView2 setup did not finish. Install it from Microsoft, then reopen Cabbage."
                },
                MB_OK | MB_ICONERROR,
            );
            false
        }
        Err(error) => {
            message(&error, MB_OK | MB_ICONERROR);
            false
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_or_malformed_webview_registry_versions_are_not_ready() {
        for value in ["", "0.0.0.0", "1", "1.2.3.bad"] {
            assert!(!valid_version(value));
        }
        assert!(valid_version("140.0.1234.5"));
    }
    #[test]
    fn cancelled_windows_permission_prompt_is_reported_as_cancellation() {
        let script=format!("function Start-Process {{ throw [System.ComponentModel.Win32Exception]::new(1223) }}\n{INSTALL}");
        let mut command = powershell(&script).unwrap();
        command
            .env("CABBAGE_DEPENDENCY_FILE", "unused.exe")
            .env("CABBAGE_DEPENDENCY_ARGS", "/install")
            .env("CABBAGE_DEPENDENCY_ELEVATE", "1");
        assert_eq!(wait(command, Duration::from_secs(30)).unwrap(), 1602);
    }
    #[test]
    fn unsigned_installer_is_not_executed() {
        let root =
            std::env::temp_dir().join(format!("cabbage dependencies {:x}", rand::random::<u64>()));
        fs::create_dir(&root).unwrap();
        let path = root.join("fixture.exe");
        fs::write(&path, b"not an installer").unwrap();
        assert!(verify(&path).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
