$ErrorActionPreference = 'Stop'
$cabbageSource = Get-Content src-tauri/src/installation_windows.rs -Raw
function Invoke-CabbageScript([string]$name) {
    $cabbageMatch = [regex]::Match($cabbageSource, ('(?s)const ' + $name + ': &str = r#"(.*?)"#;'))
    if (-not $cabbageMatch.Success) { throw "Missing $name script" }
    $cabbageEncoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($cabbageMatch.Groups[1].Value))
    & powershell.exe -NoProfile -NonInteractive -EncodedCommand $cabbageEncoded
    if ($LASTEXITCODE -ne 0) { throw "$name failed with exit code $LASTEXITCODE" }
}
$cabbageTestRoot = Join-Path ([IO.Path]::GetTempPath()) ('Cabbage install smoke ' + [guid]::NewGuid())
$env:CABBAGE_INSTALL_ROOT = Join-Path $cabbageTestRoot 'cabbage/launcher'
$env:CABBAGE_INSTALL_BINARY = Join-Path $env:CABBAGE_INSTALL_ROOT 'Cabbage.exe'
$env:CABBAGE_INSTALL_SHORTCUT = Join-Path $cabbageTestRoot 'Start Menu/Programs/Cabbage.lnk'
$env:CABBAGE_INSTALL_ICON = Join-Path $env:CABBAGE_INSTALL_ROOT 'Cabbage.ico'
$env:CABBAGE_INSTALL_VERSION = (Get-Content src-tauri/tauri.conf.json | ConvertFrom-Json).version
$env:CABBAGE_INSTALL_KEY = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Cabbage.Smoke.' + [guid]::NewGuid()
try {
    New-Item -ItemType Directory -Path $env:CABBAGE_INSTALL_ROOT -Force | Out-Null
    New-Item -ItemType Directory -Path (Split-Path $env:CABBAGE_INSTALL_SHORTCUT) -Force | Out-Null
    [IO.File]::WriteAllBytes($env:CABBAGE_INSTALL_BINARY, [Text.Encoding]::ASCII.GetBytes('MZfixture'))
    Copy-Item src-tauri/icons/icon.ico $env:CABBAGE_INSTALL_ICON
    Set-Content (Join-Path $env:CABBAGE_INSTALL_ROOT 'installation.json') '{}'
    Set-Content (Join-Path $cabbageTestRoot 'keep.txt') 'keep'
    Invoke-CabbageScript 'INTEGRATE'
    $env:CABBAGE_SHORTCUT_HASH = (Get-FileHash -LiteralPath $env:CABBAGE_INSTALL_SHORTCUT).Hash
    $env:CABBAGE_ICON_HASH = (Get-FileHash -LiteralPath $env:CABBAGE_INSTALL_ICON).Hash
    Remove-Item Env:CABBAGE_UNINSTALL_PID -ErrorAction SilentlyContinue
    Invoke-CabbageScript 'CLEANUP'
    if (Test-Path -LiteralPath $env:CABBAGE_INSTALL_BINARY) { throw 'Launcher remains installed' }
    if (Test-Path -LiteralPath $env:CABBAGE_INSTALL_SHORTCUT) { throw 'Shortcut remains installed' }
    if (-not (Test-Path -LiteralPath (Join-Path $cabbageTestRoot 'keep.txt'))) { throw 'Unrelated data was removed' }
} finally {
    if (Test-Path -LiteralPath $env:CABBAGE_INSTALL_KEY) { Remove-Item -LiteralPath $env:CABBAGE_INSTALL_KEY -Recurse -Force }
    if (Test-Path -LiteralPath $cabbageTestRoot) { Remove-Item -LiteralPath $cabbageTestRoot -Recurse -Force }
}
