# Debian and Ubuntu dependency setup

Working branch: `feature/windows-dependency-setup`. No stable release changes.

Automatic setup supports Debian 12/13 and Ubuntu 22.04/24.04 on x64. Detection uses explicit ID and VERSION_ID, not ID_LIKE. Other releases and derivatives retain their existing launch behavior.

The existing terminal review lists missing requirements and mapped repository packages. Cancel remains the default. After approval, the helper runs two separate sudo commands:

```sh
apt-get update --error-on=any
apt-get install --no-remove --reinstall -o APT::Get::Assume-Yes=false -o APT::Get::Assume-No=false -- <packages>
```

A failed package-list refresh stops installation. APT retains its transaction confirmation. No full system upgrade, repository changes, extra architectures, unattended approval, automatic repair of broken packages or lock-file deletion is performed. APT handles its own locks; existing lock files alone do not imply a running transaction. Reinstallation permits repair of missing files in already installed dependency packages.

Debian 12 and Ubuntu 22.04 use libasound2; Debian 13 and Ubuntu 24.04 use libasound2t64. SDL2, graphics and X11 libraries map to each distribution's package names. Wine comes from the configured distro repositories. Unavailable packages or disabled repositories remain visible errors in the terminal.

Tests cover release detection, package mapping and allowlisting, failed refresh stopping installation, failed install, successful transaction sequencing and APT review text. CI also simulates the complete dependency transaction in each of the four distro containers without installing packages.

An interactive install and cancellation still need acceptance in a Debian/Ubuntu desktop VM. No Debian/Ubuntu package transaction has been run on the development host.
