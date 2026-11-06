# active-linux-watermark

A Windows-style **"Activate Linux"** watermark in the bottom-right corner of every monitor.
Transparent, click-through, always on top. Works on X11 and Wayland (via XWayland).
Tiny: ~550 KB binary, ~12 MB RAM, ~0% CPU.

## Install

Download the package from [Releases](https://github.com/theld21/active-linux-watermark/releases):

    sudo dnf install ./active-linux-watermark-*.x86_64.rpm   # Fedora / RHEL
    sudo apt install ./active-linux-watermark_*_amd64.deb    # Ubuntu 22.04+ / Debian 12+

It starts automatically at login. To start it now, open **Active Linux Watermark Settings** from the app menu.

## Configure

**Active Linux Watermark Settings** lets you edit both lines, their font sizes, and the right/bottom margins.
Changes apply instantly. Config file: `~/.config/active-linux-watermark.conf`.

## Uninstall

    sudo dnf remove active-linux-watermark   # or: sudo apt remove active-linux-watermark

## Build

    cargo install cargo-deb cargo-generate-rpm
    cargo build --release && cargo deb --no-build && cargo generate-rpm

## Release

Bump `version` in `Cargo.toml`, then `git tag vX.Y.Z && git push --tags`.
GitHub Actions builds the `.deb` and `.rpm` and attaches them to the release.
