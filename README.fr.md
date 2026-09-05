# Noosphere

[English](README.md) | **Français**

## [Télécharger pour Windows](Noosphere_0.1.0_x64-setup.exe)

Noosphere est un client de messagerie de bureau qui utilise GitHub pour échanger
des messages chiffrés. Il n’y a pas de serveur Noosphere à héberger ni de compte
supplémentaire à créer.

L’application est construite avec Tauri, Rust, React et Vite.

## Fonctionnalités

- messages chiffrés de bout en bout avec libsignal ;
- contacts liés aux comptes GitHub ;
- synchronisation des messages par dépôt GitHub dédié ;
- échange direct par WebRTC lorsque les deux personnes sont en ligne ;
- appels audio avec caméra activable pendant l’appel ;
- choix du microphone, de la sortie audio et de la caméra ;
- test du microphone, réduction du bruit, annulation d’écho et réglage du gain ;
- client natif pour Windows, avec prise en charge de Linux prévue par la chaîne
  de compilation.

## Fonctionnement

Chaque utilisateur relie Noosphere à GitHub et crée un dépôt public dédié. Ce
dépôt ne contient que des données chiffrées. Les clés privées et les messages
lisibles restent sur l’ordinateur.

Les changements d’étoile GitHub servent de signal léger pour les demandes
d’amis, les nouveaux messages et les appels. Noosphere vérifie ces signaux toutes
les trois secondes, puis lit uniquement le dépôt concerné. Quand une connexion
WebRTC est disponible, elle permet de recevoir les événements sans attendre la
prochaine synchronisation.

GitHub peut toujours voir les métadonnées normales d’un dépôt : dates de commit,
volume d’activité et comptes impliqués. Une connexion WebRTC révèle également
aux pairs les informations réseau nécessaires à ICE.

## Installation

L’installateur Windows se trouve à la racine du dépôt. La version portable et
les sommes SHA-256 sont disponibles dans `release/windows/`.

Le binaire Windows n’est pas encore signé avec un certificat Authenticode.
SmartScreen peut donc afficher un avertissement au premier lancement.

## Compiler le projet

Il faut installer :

- Node.js 22.12 ou plus récent ;
- Rust 1.98.1 avec `rustup` ;
- Protocol Buffers Compiler (`protoc`) 29 ou plus récent ;
- les dépendances système demandées par Tauri 2.

Sous Windows, les outils de compilation C++ de Visual Studio sont nécessaires.
Sous Debian ou Ubuntu :

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf \
  protobuf-compiler
```

Installez les dépendances puis lancez le client :

```bash
npm ci
npm run dev
```

Pour créer un paquet natif :

```bash
npm run package:windows
# ou
npm run package:linux
```

Les paquets sont générés dans `src-tauri/target/release/bundle/`.

## Développement

Les principaux dossiers sont :

- `src-tauri/` : cœur Rust, accès à GitHub, stockage local et chiffrement ;
- `app/`, `components/` et `src/` : interface React ;
- `lib/` : logique partagée côté interface ;
- `tests/` : tests du client ;
- `public/brand/` et `src-tauri/icons/` : ressources embarquées.

Commandes utiles :

```bash
npm run format
npm run lint
npm test
npm run check
```

## Autorisations

La GitHub App doit être installée uniquement sur le dépôt Noosphere de
l’utilisateur avec les autorisations `Contents: write` et `Metadata: read`.
L’autorisation de compte `Starring: write` est utilisée pour les signaux de
synchronisation.

Le microphone et la caméra dépendent des autorisations accordées aux
applications de bureau dans Windows ou Linux. Un appel peut continuer avec le
microphone si la caméra est indisponible, et inversement lorsque la caméra est
activée.

## Sécurité

Noosphere est encore jeune et n’a pas fait l’objet d’un audit indépendant. Les
problèmes de sécurité doivent être signalés de manière privée. Les détails du
modèle de sécurité et ses limites sont décrits dans [SECURITY.md](SECURITY.md).

## Licence

Noosphere est distribué sous licence [AGPL-3.0-only](LICENSE).
