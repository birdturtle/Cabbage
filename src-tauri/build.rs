fn main() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../game-host/Cargo.toml");
    let target = std::env::var("TARGET").unwrap();
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("game-host");
    for path in [
        "../game-host/src",
        "../game-host/Cargo.toml",
        "../game-host/Cargo.lock",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    let status = std::process::Command::new(std::env::var_os("CARGO").unwrap())
        .args(["build", "--locked", "--release", "--manifest-path"])
        .arg(&manifest)
        .arg("--target")
        .arg(&target)
        .arg("--target-dir")
        .arg(&output)
        .status()
        .expect("Cannot build game host");
    assert!(status.success(), "Cannot build standalone game host");
    let executable = if target.contains("windows") {
        "cabbage-game-host.exe"
    } else {
        "cabbage-game-host"
    };
    println!(
        "cargo:rustc-env=CABBAGE_GAME_HOST={}",
        output
            .join(target)
            .join("release")
            .join(executable)
            .display()
    );

    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(&[
            "installation_status",
            "install_launcher",
            "uninstall_launcher",
            "keep_portable",
            "launcher_status",
            "launch_client",
            "maintain_client",
            "select_client",
            "save_preferences",
            "begin_login",
            "cancel_login",
            "remove_account",
            "select_character",
            "load_accounts",
            "select_account",
            "check_updates",
            "recheck_requirements",
            "install_requirements",
            "install_update",
            "update_progress",
            "acknowledge_update",
        ]))
        .windows_attributes(
            tauri_build::WindowsAttributes::new().window_icon_path("icons/icon.ico"),
        );
    tauri_build::try_build(attributes).expect("Cannot build launcher permissions");
}
