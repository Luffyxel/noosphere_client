# Linux packages

**English** | [Français](README.fr.md)

The Linux release targets 64-bit x86 systems. It includes its CEF/Chromium
runtime and supports X11 desktops. Wayland desktops need XWayland.

## AppImage

Use the AppImage on distributions without a DEB or RPM package:

```bash
chmod +x Noosphere_0.1.0_amd64.AppImage
./Noosphere_0.1.0_amd64.AppImage
```

On NixOS:

```bash
nix-shell -p appimage-run --run 'appimage-run ./Noosphere_0.1.0_amd64.AppImage'
```

## Debian and Ubuntu

```bash
sudo apt install ./Noosphere_0.1.0_amd64.deb
```

## Fedora

```bash
sudo dnf install ./Noosphere_0.1.0_x86_64.rpm
```

## openSUSE

```bash
sudo zypper install ./Noosphere_0.1.0_x86_64.rpm
```

## Wayland

Noosphere currently runs through XWayland. With Hyprland managed by NixOS,
enable `programs.hyprland.xwayland.enable = true` and restart the session.

The AppImage passed the two-client smoke test on NixOS 26.05 under Xvfb. A
Hyprland session still needs XWayland for a visible launch. The same media test
runs in the Ubuntu 22.04 packaging container.

## Checksums

```bash
sha256sum -c SHA256SUMS.txt
```
