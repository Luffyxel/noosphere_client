# Noosphere

[English](README.md) | **Français**

Noosphere est un client de messagerie de bureau qui échange des messages
chiffrés de bout en bout par GitHub. Il fonctionne sans serveur Noosphere ni
compte supplémentaire.

## Téléchargements

- [Installateur Windows](Noosphere_0.1.6_x64-setup.exe)
- Linux : [AppImage](https://github.com/Luffyxel/noosphere_client/releases/download/v0.1.6/Noosphere_0.1.6_amd64.AppImage), [DEB](https://github.com/Luffyxel/noosphere_client/releases/download/v0.1.6/Noosphere_0.1.6_amd64.deb), [RPM](https://github.com/Luffyxel/noosphere_client/releases/download/v0.1.6/Noosphere_0.1.6_x86_64.rpm), [sommes de contrôle](https://github.com/Luffyxel/noosphere_client/releases/download/v0.1.6/SHA256SUMS.txt)

Les instructions d’installation et les sommes de contrôle se trouvent dans
[`release/`](release/README.fr.md).

## Fonctionnalités

- chiffrement de bout en bout avec libsignal ;
- contacts et demandes d’amis liés aux comptes GitHub ;
- acceptation automatique lorsque les deux personnes s’envoient une demande ;
- acceptation ou refus des demandes d’amis ;
- profil des amis avec suppression du contact ;
- échange direct par WebRTC lorsque les deux personnes sont en ligne ;
- appels audio avec caméra activable ;
- choix du microphone, de la sortie audio et de la caméra ;
- test du microphone, réduction du bruit, annulation d’écho et réglage du gain ;
- paquets de bureau pour Windows et Linux.

## Fonctionnement

Chaque utilisateur relie Noosphere à GitHub et autorise l’application sur un
seul dépôt public. Ce dépôt contient les données chiffrées du protocole. Les
clés privées et les messages lisibles restent sur l’ordinateur. Chaque compte
GitHub conserve sa propre identité cryptographique, quel que soit l’ordre dans
lequel les comptes sont ouverts.

Le client vérifie les changements d’étoile GitHub toutes les trois secondes
pour les demandes d’amis, les messages et les appels, puis lit le dépôt
concerné. Une connexion WebRTC active transmet les événements directement.

Les messages et la signalisation des appels passent d’un réseau à l’autre par
GitHub. Les appels utilisent une connexion WebRTC directe avec le serveur STUN
de Cloudflare. Aucun relais TURN n’est configuré : un appel peut donc échouer
derrière un NAT symétrique, un CGNAT ou un pare-feu restrictif.

GitHub peut voir les métadonnées du dépôt, notamment les dates de commit, le
volume d’activité et les comptes impliqués. WebRTC révèle aux deux pairs les
informations réseau nécessaires à ICE.

## Linux

La version Linux est disponible pour x86_64 dans trois formats :

- AppImage pour Arch Linux, NixOS et les autres distributions ;
- DEB pour Debian et Ubuntu ;
- RPM pour Fedora, Red Hat Enterprise Linux 9 et 10, Rocky Linux 9 et 10,
  et openSUSE.

Les paquets incluent le moteur CEF/Chromium utilisé par le client. X11 est pris
en charge directement ; une session Wayland doit disposer de XWayland.

Sous NixOS, lancez l’AppImage avec `appimage-run` :

```bash
nix-shell -p appimage-run --run 'appimage-run ./Noosphere_0.1.6_amd64.AppImage'
```

Avec Hyprland géré par NixOS, activez
`programs.hyprland.xwayland.enable = true`, puis redémarrez la session. Les
commandes d’installation sont détaillées dans la
[documentation Linux](release/linux/README.fr.md).

## Compiler le projet

Outils nécessaires :

- Node.js 22.12 ou plus récent ;
- Rust 1.98.0 avec `rustup` ;
- Protocol Buffers Compiler (`protoc`) 29 ou plus récent ;
- les paquets système demandés par Tauri 2.

Sous Windows, les outils de compilation C++ de Visual Studio sont nécessaires.
Sous Debian ou Ubuntu :

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev libxkbcommon-x11-0 \
  librsvg2-dev patchelf protobuf-compiler rpm
```

Installez les dépendances puis lancez l’application :

```bash
npm ci
npm run dev
```

Créez un paquet pour la plateforme courante :

```bash
npm run package:windows
npm run package:linux
```

Les paquets Linux peuvent aussi être produits dans l’environnement conteneurisé
du projet :

```bash
npm run package:linux:container
```

Le conteneur place l’AppImage, le DEB, le RPM et `SHA256SUMS.txt` dans
`release/linux/`.

## Organisation du dépôt

- `src-tauri/` : cœur Rust, accès à GitHub, stockage local et chiffrement ;
- `app/`, `components/` et `src/` : interface React ;
- `lib/` : protocole côté interface et gestion des médias ;
- `packaging/` et `scripts/` : création des versions ;
- `tests/` : tests de l’interface et des paquets ;
- `public/brand/` et `src-tauri/icons/` : ressources de l’application.

## Vérifications

```bash
npm run format:check
npm run lint
npm test
npm run check
```

## Autorisations

La GitHub App utilise `Contents: write` et `Metadata: read` uniquement sur le
dépôt Noosphere de l’utilisateur. L’autorisation de compte `Starring: write`
sert à la synchronisation.

L’accès au microphone et à la caméra suit les autorisations de bureau du
système d’exploitation. Un appel continue avec les périphériques demandés qui
restent disponibles.

## Sécurité

Aucun audit de sécurité indépendant n’a encore été réalisé. Les vulnérabilités
doivent être signalées en privé comme indiqué dans [SECURITY.md](SECURITY.md).

## Licence

Noosphere est distribué sous licence [AGPL-3.0-only](LICENSE).
