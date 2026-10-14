# openSUSE dependency setup

Supports standard openSUSE Tumbleweed and Leap 16.0 on x64. Explicit distribution IDs are required. MicroOS, Aeon, Kalpa and systems with transactional-update are excluded. Other editions keep existing launch behavior.

The terminal review defaults to Cancel. After approval, separate sudo commands run:

```sh
zypper refresh
zypper install --force --no-allow-vendor-change <reviewed requirements>
```

Refresh failures stop installation. Zypper retains its transaction confirmation and handles repository trust and package locks. No distribution upgrade, repository additions, automatic approval, vendor switch or lock deletion is performed. Force reinstalls only the reviewed requirements, allowing missing files in already installed packages to be restored.

Libraries are requested by fixed RPM 64-bit soname capabilities rather than names that vary between releases. For example, libEGL.so.1()(64bit) lets the configured repositories choose the package providing that exact host library. Wine, GNOME Keyring and XWayland use their repository package names. All targets use a fixed allowlist and direct process arguments; capability parentheses never pass through a shell.

Tests cover explicit edition detection, fixed capability targets, transaction arguments and refresh failure stopping installation. CI resolves the full transaction with Zypper's dry-run mode in Tumbleweed and Leap 16.0 containers. Interactive VM acceptance remains pending. No openSUSE package transaction has run on the development host.

Reference: [Zypper manual](https://manpages.opensuse.org/Tumbleweed/zypper/zypper.8.en.html).
