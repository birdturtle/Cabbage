use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use std::{
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn key(layout: &Layout) -> String {
    let name = if cfg!(test) {
        format!(
            "Cabbage.Test.{:x}",
            Sha256::digest(layout.root.to_string_lossy().as_bytes())
        )
    } else {
        ID.to_owned()
    };
    format!("HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{name}")
}
fn command(layout: &Layout, script: &str) -> Command {
    let encoded: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &STANDARD.encode(encoded),
        ])
        .env("CABBAGE_INSTALL_ROOT", &layout.root)
        .env("CABBAGE_INSTALL_BINARY", layout.binary())
        .env("CABBAGE_INSTALL_SHORTCUT", &layout.desktop)
        .env("CABBAGE_INSTALL_ICON", &layout.icon)
        .env("CABBAGE_INSTALL_VERSION", env!("CARGO_PKG_VERSION"))
        .env("CABBAGE_INSTALL_KEY", key(layout))
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(test)]
    command.stderr(Stdio::inherit());
    crate::clients::configure_credentials(&mut command, &[]);
    command
}
fn run(mut command: Command) -> Result<(), String> {
    let mut child = command
        .spawn()
        .map_err(|_| "Cannot start Windows installation helper.")?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait().map_err(|_| "Cannot track Windows installation helper.")? {
            Some(exit) if exit.success() => return Ok(()),
            Some(_) => return Err("Windows installation failed. Check access to the installation and Start Menu folders.".into()),
            None if Instant::now() >= deadline => { let _ = child.kill(); let _ = child.wait(); return Err("Windows installation timed out. Retry installation.".into()); },
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}
const INTEGRATE: &str = r#"
$ErrorActionPreference='Stop'
$binary=$env:CABBAGE_INSTALL_BINARY
$shortcut=$env:CABBAGE_INSTALL_SHORTCUT
$shell=New-Object -ComObject WScript.Shell
$link=$shell.CreateShortcut($shortcut)
$link.TargetPath=$binary
$link.WorkingDirectory=$env:CABBAGE_INSTALL_ROOT
$link.IconLocation=$env:CABBAGE_INSTALL_ICON
$link.Description='Cabbage'
$link.Save()
$key=$env:CABBAGE_INSTALL_KEY
New-Item -Path $key -Force | Out-Null
$values=@{DisplayName='Cabbage';DisplayVersion=$env:CABBAGE_INSTALL_VERSION;Publisher='birdturtle';InstallLocation=$env:CABBAGE_INSTALL_ROOT;DisplayIcon=$env:CABBAGE_INSTALL_ICON;UninstallString=('"'+$binary+'" --uninstall-launcher')}
foreach($name in $values.Keys){New-ItemProperty -Path $key -Name $name -Value $values[$name] -PropertyType String -Force | Out-Null}
foreach($name in @('NoModify','NoRepair')){New-ItemProperty -Path $key -Name $name -Value 1 -PropertyType DWord -Force | Out-Null}
"#;
pub fn integrate(layout: &Layout) -> Result<String, String> {
    safe_directory(layout.desktop.parent().ok_or("Invalid Start Menu path.")?)?;
    run(command(layout, INTEGRATE))?;
    let bytes = fs::read(&layout.desktop).map_err(|_| "Cannot verify Start Menu shortcut.")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
const CLEANUP: &str = r#"
$ErrorActionPreference='Stop'
if($env:CABBAGE_UNINSTALL_PID){
  try{$parent=[System.Diagnostics.Process]::GetProcessById([int]$env:CABBAGE_UNINSTALL_PID)}catch{$parent=$null}
  if($parent -and -not $parent.WaitForExit(30000)){exit 1}
}
$binary=$env:CABBAGE_INSTALL_BINARY
$removed=$false
for($attempt=0;$attempt -lt 40;$attempt++){
  try {if(Test-Path -LiteralPath $binary){Remove-Item -LiteralPath $binary -Force};$removed=$true;break}catch{Start-Sleep -Milliseconds 250}
}
if(-not $removed){exit 1}
$shortcut=$env:CABBAGE_INSTALL_SHORTCUT
if((Test-Path -LiteralPath $shortcut) -and (Get-FileHash -LiteralPath $shortcut -Algorithm SHA256).Hash -eq $env:CABBAGE_SHORTCUT_HASH){Remove-Item -LiteralPath $shortcut -Force}
$icon=$env:CABBAGE_INSTALL_ICON
if((Test-Path -LiteralPath $icon) -and (Get-FileHash -LiteralPath $icon -Algorithm SHA256).Hash -eq $env:CABBAGE_ICON_HASH){Remove-Item -LiteralPath $icon -Force}
$key=$env:CABBAGE_INSTALL_KEY
if(Test-Path -LiteralPath $key){
  if((Get-ItemProperty -LiteralPath $key).InstallLocation -eq $env:CABBAGE_INSTALL_ROOT){Remove-Item -LiteralPath $key -Recurse -Force}
}
Remove-Item -LiteralPath (Join-Path $env:CABBAGE_INSTALL_ROOT 'installation.json') -Force
"#;
pub fn uninstall(layout: &Layout) -> Result<bool, String> {
    let marker: Marker = serde_json::from_slice(
        &fs::read(layout.marker()).map_err(|_| "Cannot read installation record.")?,
    )
    .map_err(|_| "Invalid installation record.")?;
    let active = std::env::current_exe()
        .ok()
        .and_then(|p| fs::canonicalize(p).ok())
        == fs::canonicalize(layout.binary()).ok();
    let mut command = command(layout, CLEANUP);
    command
        .env("CABBAGE_SHORTCUT_HASH", marker.desktop)
        .env("CABBAGE_ICON_HASH", format!("{:x}", Sha256::digest(ICON)))
        .env_remove("CABBAGE_UNINSTALL_PID");
    atomic_file(&layout.root.join("portable.json"), b"true", false)?;
    if active {
        command.env("CABBAGE_UNINSTALL_PID", std::process::id().to_string());
        command
            .spawn()
            .map_err(|_| "Cannot start Cabbage uninstaller.")?;
        Ok(true)
    } else {
        run(command)?;
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_waits_for_launcher_and_preserves_unrelated_files() {
        let root =
            std::env::temp_dir().join(format!("cabbage windows cleanup {}", rand::random::<u64>()));
        fs::create_dir_all(&root).unwrap();
        let layout = Layout::windows(&root.join("Local"), &root.join("Roaming"));
        let source = root.join("download.exe");
        fs::write(&source, b"MZfixture").unwrap();
        install(&layout, &source).unwrap();
        fs::write(layout.root.join("keep.txt"), b"keep").unwrap();
        let marker: Marker = serde_json::from_slice(&fs::read(layout.marker()).unwrap()).unwrap();
        let mut parent = command(&layout, "Start-Sleep -Seconds 2").spawn().unwrap();
        let mut worker = command(&layout, CLEANUP);
        worker
            .env("CABBAGE_UNINSTALL_PID", parent.id().to_string())
            .env("CABBAGE_SHORTCUT_HASH", marker.desktop)
            .env("CABBAGE_ICON_HASH", format!("{:x}", Sha256::digest(ICON)));
        let mut worker = worker.spawn().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(layout.binary().exists());
        assert!(parent.wait().unwrap().success());
        assert!(worker.wait().unwrap().success());
        assert!(!layout.binary().exists());
        assert!(!layout.desktop.exists());
        assert!(!layout.marker().exists());
        assert_eq!(fs::read(layout.root.join("keep.txt")).unwrap(), b"keep");
        assert!(source.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
