# Noosphere

**English** | [Français](README.fr.md)

Noosphere is a desktop messaging client that exchanges end-to-end encrypted
messages through GitHub. It runs without a Noosphere server or a separate
account.

## Downloads

- [Windows installer](https://github.com/Luffyxel/noosphere_client/releases/latest/download/Noosphere-Windows-Setup-x64.exe) · [Windows portable](https://github.com/Luffyxel/noosphere_client/releases/latest/download/Noosphere-Windows-Portable-x64.exe)
- Linux: [AppImage](https://github.com/Luffyxel/noosphere_client/releases/latest/download/Noosphere-Linux-AppImage-x64.AppImage), [DEB](https://github.com/Luffyxel/noosphere_client/releases/latest/download/Noosphere-Linux-DEB-x64.deb), [RPM](https://github.com/Luffyxel/noosphere_client/releases/latest/download/Noosphere-Linux-RPM-x64.rpm), [checksums](https://github.com/Luffyxel/noosphere_client/releases/latest/download/SHA256SUMS.txt)

Installation notes and checksums are kept in [`release/`](release/README.md).

## Features

- end-to-end encryption with libsignal;
- contacts and friend requests linked to GitHub accounts;
- automatic acceptance when both users send each other a request;
- friend requests can be accepted or declined;
- friend profiles with contact removal;
- direct WebRTC transport when both users are online;
- audio calls with an optional camera;
- microphone, audio output, and camera selection;
- microphone test, noise suppression, echo cancellation, and gain control;
- desktop packages for Windows and Linux.

## How it works

Each user connects Noosphere to GitHub and gives the application access to one
public repository. The repository stores encrypted protocol data. Private keys
and readable messages remain on the user's computer. Each GitHub account keeps
its own cryptographic identity regardless of the order in which accounts are
opened.

The client checks GitHub star changes every three seconds for friend requests,
messages, and calls, then reads the affected repository. An active WebRTC
connection delivers events directly.

Messages and call signaling work between networks through GitHub. Calls use a
direct WebRTC connection with Cloudflare STUN. No TURN relay is configured, so
calls can fail behind symmetric NAT, carrier-grade NAT, or restrictive
firewalls.

GitHub can observe repository metadata such as commit dates, activity volume,
and the accounts involved. WebRTC exposes the network information required by
ICE to both peers.

## Remote desktop development

A native remote desktop engine is being developed in `src-tauri/remote`.
The dedicated Remote Desktop section discovers machines belonging to your account
and friends. Discovery starts at sign-in without enabling hosting. Friends can
receive access to your computer while their own host is disabled. Each device
needs a build supporting the directory. Settings list users, including your own
account, and save screen, keyboard, mouse and unattended access even before a
machine is discovered. The rule is applied to that user's verified machines as
they appear. Granting screen access enables the local host.
Encrypted GitHub mailboxes carry requests and decisions; grants remain in protected
local storage.
A second computer keeps its own identity without replacing the first computer's
published profile. Chat keys and history are not transferred between computers.

On Windows, an approved request can now open a direct native H.264 session with
GPU capture/encoding, QUIC DATAGRAM, GPU viewing and keyboard/mouse return. Bitrate,
keyframes and FEC react to client network feedback. Encrypted GitHub machine mailboxes
carry signaling independently of the selected conversation.
STUN advertises a public candidate when the router permits it; without a TURN server,
some CGNAT paths remain unreachable. No router port forwarding is required. Windows
asks once for administrator approval to allow inbound UDP for the exact executable;
the UAC prompt opens Noosphere directly without a PowerShell window. Moving or
replacing the portable build triggers that approval again. This is a validation
build rather than the cross-platform V0.2.0. See the [engine documentation](docs/remote-desktop.md)
for implementation status and reproducible benchmarks.

## Linux

The Linux release is available for x86_64 in three formats:

- AppImage for Arch Linux, NixOS, and other distributions;
- DEB for Debian and Ubuntu;
- RPM for Fedora, Red Hat Enterprise Linux 9 and 10, Rocky Linux 9 and 10,
  and openSUSE.

The packages include the CEF/Chromium runtime used by the client. Remote
desktop capture uses PipeWire and the system portal on Wayland. XWayland may
still be required for the main application shell.

GNOME and KDE use the RemoteDesktop portal. Hyprland and Sway use ScreenCast
with native Wayland virtual-input protocols. X11 sessions use `ximagesrc` and
XTest. Video queues are capped at one frame so stale frames cannot accumulate.
Real capture has been validated on NixOS 26.05 with Hyprland 0.55.4;
variable-rate PipeWire streams are normalized to the selected frame rate
without blocking newer frames.

On NixOS, run the AppImage through `appimage-run`:

```bash
nix-shell -p appimage-run --run 'appimage-run ./Noosphere-Linux-AppImage-x64.AppImage'
```

For Hyprland managed by NixOS, enable PipeWire, WirePlumber,
`xdg-desktop-portal-hyprland`, and XWayland. A reproducible configuration is
provided in `packaging/nixos/remote-test.nix`. See the
[Linux package notes](release/linux/README.md) for installation commands.

## Building from source

Required tools:

- Node.js 22.12 or newer;
- Rust 1.98.0 through `rustup`;
- Protocol Buffers Compiler (`protoc`) 29 or newer;
- the system packages required by Tauri 2.

Windows builds require the Visual Studio C++ build tools. On Debian or Ubuntu:

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev libxkbcommon-x11-0 \
  librsvg2-dev patchelf protobuf-compiler rpm
```

Install dependencies and start the application:

```bash
npm ci
npm run dev
```

Build a platform package:

```bash
npm run package:windows
npm run package:linux
```

Linux packages can also be built in the pinned container environment:

```bash
npm run package:linux:container
```

The container exports the AppImage, DEB, RPM, and `SHA256SUMS.txt` to
`release/linux/`.

## Repository layout

- `src-tauri/`: Rust core, GitHub access, local storage, and encryption;
- `app/`, `components/`, and `src/`: React interface;
- `lib/`: frontend protocol and media code;
- `packaging/` and `scripts/`: release tooling;
- `tests/`: frontend and packaging tests;
- `public/brand/` and `src-tauri/icons/`: application assets.

## Checks

```bash
npm run format:check
npm run lint
npm test
npm run check
```

## Permissions

The GitHub App needs `Contents: write` and `Metadata: read` on the user's
Noosphere repository. The `Starring: write` account permission is used for
synchronization.

Microphone and camera access follows the desktop permissions configured by the
operating system. Calls continue with any available requested device.

## Security

No independent security audit has been completed. Report vulnerabilities
privately as described in [SECURITY.md](SECURITY.md).

## License

Noosphere is distributed under the [AGPL-3.0-only](LICENSE) license.
