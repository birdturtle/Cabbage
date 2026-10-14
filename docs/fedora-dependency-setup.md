# Fedora dependency setup

Automatic dependency setup supports standard Fedora 43 and 44 on x64. It requires the Fedora ID, supported VERSION_ID and /usr/bin/dnf. Atomic variants are excluded by VARIANT_ID and the /run/ostree-booted marker. Fedora derivatives and unsupported versions keep their existing launch behavior.

The same terminal helper reviews missing requirements and defaults to Cancel. After approval it runs:

```sh
sudo -- /usr/bin/dnf --refresh --setopt=assumeyes=False --setopt=assumeno=False install -- <packages.x86_64>
```

DNF shows its transaction for confirmation. There is no full system upgrade, automatic approval, repository addition, --allowerasing or skipped broken dependency. Sudo owns its password prompt and DNF owns package locks. Failed installation returns a visible error and Cabbage rechecks requirements.

Wine uses the Fedora wine package. SDL2 uses sdl2-compat. Graphics requirements use libglvnd-egl, libglvnd-opengl and libglvnd-glx. X11 library package names retain Fedora capitalization. Explicit x86_64 package targets match the managed game runtimes.

If installation succeeds but reviewed requirements still have missing files, the helper offers DNF's normal reinstall transaction for those packages only. Newly missing packages require a new review. Reinstallation is also subject to DNF confirmation.

Tests cover distro and Atomic exclusion, package mappings, fixed command arguments, cancellation/failure and terminal review. CI checks package availability in Fedora 43/44 containers. Interactive installation and cancellation still need testing in a Fedora desktop VM. No Fedora package transaction has run on the development host.

References: [DNF install behavior](https://dnf5.readthedocs.io/en/stable/commands/install.8.html), [DNF confirmation settings](https://dnf5.readthedocs.io/en/stable/dnf5.conf.5.html), [Fedora SDL2 package](https://packages.fedoraproject.org/pkgs/sdl2-compat/sdl2-compat/).
