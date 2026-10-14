# Arch and CachyOS dependency setup

Working branch: `feature/windows-dependency-setup`. No stable release changes.

Cabbage embeds a separate Rust terminal helper built with Ratatui and Crossterm. It has no account-vault access or GUI dependencies. Helpers are installed under their content hash and verified before reuse. The existing game helper uses the same publishing utility; its startup and update protocol are unchanged.

## Requirements

Initial automatic installation supports Arch and CachyOS on x64. Other Linux distributions retain their existing launch behavior; Arch-specific probes do not disable their clients. Client requirements are probed once at startup and explicitly after setup or Check again; normal UI polling reads cached results.

- All clients: an existing keyring provider and XWayland on Wayland desktops.
- Official OSRS: Wine.
- RuneScape: SDL2, EGL and OpenGL.
- RuneLite: X11, input/rendering libraries, fonts, ALSA and OpenGL used by its Java runtime.

Library checks inspect host library files rather than the launcher's AppImage libraries. Package names are a fixed allowlist. Existing GNOME Keyring, KWallet and KeePassXC installations are not replaced; starting, unlocking or configuring an existing provider remains the user's responsibility. Driver changes and AUR packages are outside this installer.

## Terminal flow

Install requirements on Linux opens a terminal directly. Supported launchers include Konsole, GNOME Terminal, Xfce Terminal, Kitty, Alacritty, Foot and XTerm. No dependency confirmation dialog appears in Cabbage's GUI.

The TUI lists missing requirements and packages. It states that the transaction also updates installed system packages. Cancel is selected by default. Packages cannot be installed from a terminal too small to show the review; the list scrolls and the system-update notice stays visible. The interface redraws only on input or resize.

After consent, terminal raw mode and the alternate screen are restored before invoking `/usr/bin/sudo -- /usr/bin/pacman -Syu -- <packages>`. Sudo reads the password directly from the terminal. Pacman presents the full transaction for approval. There is no `--noconfirm`, partial refresh-only upgrade, automatic lock-file removal or shell command interpolation. Requirements are checked again before executing; newly missing packages require a new review. Existing package locks cause a visible error.

Completion, cancellation and failures return through a private, per-session JSON status file with a random correlation token and helper PID. Jagex environment variables and AppImage library paths are removed from helper/package commands. Cabbage rechecks requirements afterward. Closing the launcher does not deliberately terminate a package transaction. If the terminal closes unexpectedly, users are told to finish any active transaction before rechecking.

## Verification

Local probes found no missing requirements for the three clients on the development CachyOS machine. Tests cover explicit distro support, package allowlisting, duplicate packages, system-update arguments, terminal review and Linux GUI routing. Launcher regression tests passed. PTY checks exercised Cancel and Install in preview mode without installing anything.

Windows and Linux CI passed for commit `ae90c80`. The user reported successful dependency setup in a CachyOS VM. The initial live-session attempt could not finish; the subsequent test worked. Sudo cancellation was not separately reported. No host packages were changed during development.

For visual testing without package installation:

```sh
cargo run --locked --manifest-path dependency-helper/Cargo.toml -- --preview
```

For read-only inspection:

```sh
cargo run --locked --manifest-path dependency-helper/Cargo.toml -- --check officialOsrs
```
