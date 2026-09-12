# Paquets Linux

[English](README.md) | **Français**

La version Linux cible les systèmes x86 64 bits. Le bureau distant utilise
PipeWire et le portail système sous Wayland. L’interface Noosphere utilise
XWayland lorsque le compositeur ne prend pas CEF en charge directement.

## AppImage

Utilisez l’AppImage sur les distributions sans paquet DEB ou RPM :

```bash
chmod +x Noosphere-Linux-AppImage-x64.AppImage
./Noosphere-Linux-AppImage-x64.AppImage
```

Sous NixOS :

```bash
nix-shell -p appimage-run --run 'appimage-run ./Noosphere-Linux-AppImage-x64.AppImage'
```

L’AppImage contient les modules GStreamer, PipeWire et SPA nécessaires au
moteur vidéo. Elle utilise toujours le portail du bureau installé par le
système pour demander l’écran et les entrées.

L’authentification GitHub utilise d’abord le portail XDG OpenURI, puis
`xdg-open`, `gio open` ou `sensible-browser`. L’écran du code propose aussi un
bouton **Ouvrir GitHub**. Les identifiants sont conservés dans des fichiers
chiffrés réservés à l’utilisateur Unix courant, sans créer de mot de passe de
trousseau au démarrage.

Sous NixOS, l’actualisation relance automatiquement l’AppImage avec
`appimage-run`. Le fichier AppImage d’origine doit rester modifiable par
l’utilisateur courant.

## Arch Linux

Installez FUSE 2, puis lancez l’AppImage :

```bash
sudo pacman -S --needed fuse2
chmod +x Noosphere-Linux-AppImage-x64.AppImage
./Noosphere-Linux-AppImage-x64.AppImage
```

## Debian et Ubuntu

```bash
sudo apt install ./Noosphere-Linux-DEB-x64.deb
```

## Fedora, Red Hat Enterprise Linux et Rocky Linux

```bash
sudo dnf install ./Noosphere-Linux-RPM-x64.rpm
```

Le RPM s’installe et résout ses bibliothèques d’exécution sous Rocky Linux 9
et 10.

## openSUSE

```bash
sudo zypper install ./Noosphere-Linux-RPM-x64.rpm
```

## Wayland et bureaux Linux

- GNOME et KDE : le portail RemoteDesktop du bureau gère l’écran, le clavier
  et la souris.
- Hyprland et Sway : le portail ScreenCast gère l’écran ; les protocoles
  Wayland virtual-keyboard et wlr-virtual-pointer gèrent les entrées.
- X11 : `ximagesrc` capture le bureau et XTest injecte les entrées ; XWayland
  reste utile pour l’interface principale.

Noosphere accepte les flux PipeWire à fréquence variable utilisés par les
compositeurs Wayland et les normalise vers la fréquence choisie. La première
capture Wayland affiche le sélecteur d’écran sécurisé du bureau.

Sous NixOS/Hyprland, importez
[`packaging/nixos/remote-test.nix`](../../packaging/nixos/remote-test.nix) ou
activez les mêmes services PipeWire, WirePlumber et portails. Vérifiez ensuite
la chaîne média avec `./scripts/test-linux-remote.sh`.

Si Hyprland est lancé par un script personnalisé, importez son environnement dans
les services utilisateur avant Noosphere :

```bash
dbus-update-activation-environment --systemd \
  WAYLAND_DISPLAY XDG_CURRENT_DESKTOP HYPRLAND_INSTANCE_SIGNATURE
```

Le rapport reproductible du test NixOS/Hyprland et du fallback X11 se trouve dans
[`docs/linux-remote-validation.md`](../../docs/linux-remote-validation.md).

## Sommes de contrôle

```bash
sha256sum -c SHA256SUMS.txt
```
