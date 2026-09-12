# Linux packages

**English** | [Français](README.fr.md)

The Linux release targets 64-bit x86 systems. Remote desktop capture uses
PipeWire and the system portal on Wayland. The Noosphere shell uses XWayland
when the compositor cannot host CEF directly.

## AppImage

Use the AppImage on distributions without a DEB or RPM package:

```bash
chmod +x Noosphere-Linux-AppImage-x64.AppImage
./Noosphere-Linux-AppImage-x64.AppImage
```

On NixOS:

```bash
nix-shell -p appimage-run --run 'appimage-run ./Noosphere-Linux-AppImage-x64.AppImage'
```

The AppImage includes the GStreamer, PipeWire and SPA modules required by the
native video engine. It still uses the desktop portal installed by the system
to request screen and input access.

GitHub authentication uses the XDG OpenURI portal first, then the user systemd
service or desktop launchers. On NixOS, these launchers are isolated from the
libraries bundled in the AppImage so the system browser can start. The code
screen also provides an **Open GitHub** button. Credentials are stored in
encrypted files restricted to the current Unix user, without creating a
desktop keyring password.

On NixOS, updates automatically relaunch the AppImage through `appimage-run`.
The original AppImage file must remain writable by the current user.

## Arch Linux

Install FUSE 2, then run the AppImage:

```bash
sudo pacman -S --needed fuse2
chmod +x Noosphere-Linux-AppImage-x64.AppImage
./Noosphere-Linux-AppImage-x64.AppImage
```

## Debian and Ubuntu

```bash
sudo apt install ./Noosphere-Linux-DEB-x64.deb
```

## Fedora, Red Hat Enterprise Linux, and Rocky Linux

```bash
sudo dnf install ./Noosphere-Linux-RPM-x64.rpm
```

The RPM installs and resolves its runtime libraries on Rocky Linux 9 and 10.

## openSUSE

```bash
sudo zypper install ./Noosphere-Linux-RPM-x64.rpm
```

## Wayland and Linux desktops

- GNOME and KDE use their XDG RemoteDesktop portal for display, keyboard and
  pointer access.
- Hyprland and Sway use their ScreenCast portal plus the Wayland
  virtual-keyboard and wlr-virtual-pointer protocols.
- X11 uses `ximagesrc` for capture and XTest for input; XWayland remains useful
  for the main application shell.

Noosphere accepts the variable-rate PipeWire streams produced by Wayland
compositors and normalizes them to the selected frame rate. The first Wayland
capture opens the desktop's secure screen picker.

For NixOS/Hyprland, import
[`packaging/nixos/remote-test.nix`](../../packaging/nixos/remote-test.nix) or
enable the same PipeWire, WirePlumber and portal services. Run
`./scripts/test-linux-remote.sh` to verify the media path.

If Hyprland is started by a custom script, import its environment into the user
services before Noosphere starts:

```bash
dbus-update-activation-environment --systemd \
  WAYLAND_DISPLAY XDG_CURRENT_DESKTOP HYPRLAND_INSTANCE_SIGNATURE
```

The reproducible NixOS/Hyprland and X11 validation report is in
[`docs/linux-remote-validation.md`](../../docs/linux-remote-validation.md).

## Checksums

```bash
sha256sum -c SHA256SUMS.txt
```
