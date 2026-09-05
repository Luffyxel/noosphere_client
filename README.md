# Noosphere

**English** | [Français](README.fr.md)

## [Download for Windows](Noosphere_0.1.0_x64-setup.exe)

Noosphere is a desktop messaging client that uses GitHub to exchange encrypted
messages. It does not require a Noosphere server or a separate user account.

The application is built with Tauri, Rust, React, and Vite.

## Features

- end-to-end encrypted messages powered by libsignal;
- contacts linked to GitHub accounts;
- message synchronization through dedicated GitHub repositories;
- direct WebRTC transport when both users are online;
- audio calls with optional video during a call;
- microphone, audio output, and camera selection;
- microphone test, noise suppression, echo cancellation, and gain control;
- a native Windows client, with Linux supported by the build toolchain.

## How it works

Each user connects Noosphere to GitHub and creates a dedicated public
repository. The repository only contains encrypted data. Private keys and
readable messages remain on the user's computer.

GitHub star changes act as lightweight signals for friend requests, new
messages, and calls. Noosphere checks these signals every three seconds and then
reads only the relevant repository. When a WebRTC connection is available, it
delivers events without waiting for the next synchronization cycle.

GitHub can still observe normal repository metadata, including commit dates,
activity volume, and the accounts involved. A WebRTC connection also reveals
the network information required by ICE to the peers.

## Installation

The Windows installer is available at the root of the repository. A portable
executable and SHA-256 checksums are available in `release/windows/`.

The Windows binaries are not signed with an Authenticode certificate yet.
SmartScreen may display a warning on first launch.

## Building from source

Install the following tools:

- Node.js 22.12 or newer;
- Rust 1.98.1 through `rustup`;
- Protocol Buffers Compiler (`protoc`) 29 or newer;
- the system dependencies required by Tauri 2.

Windows builds require the Visual Studio C++ build tools. On Debian or Ubuntu:

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf \
  protobuf-compiler
```

Install the project dependencies and start the client:

```bash
npm ci
npm run dev
```

To create a native package:

```bash
npm run package:windows
# or
npm run package:linux
```

Packages are written to `src-tauri/target/release/bundle/`.

## Development

The main directories are:

- `src-tauri/`: Rust core, GitHub access, local storage, and encryption;
- `app/`, `components/`, and `src/`: React interface;
- `lib/`: shared frontend logic;
- `tests/`: client tests;
- `public/brand/` and `src-tauri/icons/`: bundled assets.

Useful commands:

```bash
npm run format
npm run lint
npm test
npm run check
```

## Permissions

The GitHub App must be installed only on the user's Noosphere repository with
the `Contents: write` and `Metadata: read` permissions. The `Starring: write`
account permission is used for synchronization signals.

Microphone and camera access depends on the permissions granted to desktop
applications in Windows or Linux. A call can continue with the microphone when
the camera is unavailable, or with the camera when it has been enabled and the
microphone is unavailable.

## Security

Noosphere is still a young project and has not undergone an independent security
audit. Security issues should be reported privately. See
[SECURITY.md](SECURITY.md) for the security model and known limitations.

## License

Noosphere is distributed under the [AGPL-3.0-only](LICENSE) license.
