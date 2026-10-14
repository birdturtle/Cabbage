#[cfg(target_os = "linux")]
mod tui;
#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = tui::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This dependency helper is for Linux.");
    std::process::exit(1);
}
