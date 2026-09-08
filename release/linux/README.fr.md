# Paquets Linux

[English](README.md) | **Français**

La version Linux cible les systèmes x86 64 bits. Elle inclut son moteur
CEF/Chromium et fonctionne sur les bureaux X11. Sous Wayland, XWayland est
nécessaire.

## AppImage

Utilisez l’AppImage sur les distributions sans paquet DEB ou RPM :

```bash
chmod +x Noosphere_0.1.1_amd64.AppImage
./Noosphere_0.1.1_amd64.AppImage
```

Sous NixOS :

```bash
nix-shell -p appimage-run --run 'appimage-run ./Noosphere_0.1.1_amd64.AppImage'
```

## Debian et Ubuntu

```bash
sudo apt install ./Noosphere_0.1.1_amd64.deb
```

## Fedora

```bash
sudo dnf install ./Noosphere_0.1.1_x86_64.rpm
```

## openSUSE

```bash
sudo zypper install ./Noosphere_0.1.1_x86_64.rpm
```

## Wayland

Noosphere utilise actuellement XWayland. Avec Hyprland géré par NixOS, activez
`programs.hyprland.xwayland.enable = true`, puis redémarrez la session.

L’AppImage a passé le test à deux clients sous NixOS 26.05 avec Xvfb. Une
session Hyprland a toujours besoin de XWayland pour l’affichage. Le même test
multimédia s’exécute dans le conteneur de compilation Ubuntu 22.04.

## Sommes de contrôle

```bash
sha256sum -c SHA256SUMS.txt
```
