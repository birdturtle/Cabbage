fn main() {
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(&[
            "launcher_status",
            "launch_client",
            "select_client",
            "save_preferences",
            "begin_login",
            "cancel_login",
            "remove_account",
            "select_character",
            "load_accounts",
            "select_account",
            "check_updates",
        ]))
        .windows_attributes(
            tauri_build::WindowsAttributes::new().window_icon_path("icons/icon.ico"),
        );
    tauri_build::try_build(attributes).expect("Cannot build launcher permissions");
}
