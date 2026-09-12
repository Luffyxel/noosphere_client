# Native remote desktop

The application has a dedicated Remote Desktop workspace with separate machine
navigation and host settings. An approved Windows connection exchanges ephemeral
certificates and candidates through an encrypted machine mailbox, starts the native
host, opens authenticated QUIC and presents H.264 in a native GPU viewer. Keyboard
and mouse events return through QUIC DATAGRAM and pass the current permission guard.

## Components

`src-tauri/remote` is a Rust crate without a Tauri, React, WebView, or WebRTC
dependency. Its executables are `noosphere-remote-host`,
`noosphere-remote-bench` and the folder-backed `noosphere-remote-lab`. The Windows portable application can also execute its
host entry point through `--remote-host`, before initializing Tauri.

| Module        | Current implementation                                                                                                              |
| ------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `capture`     | Windows DXGI acquisition with an owned D3D11 texture and a frame release guard; enumeration on the default adapter                  |
| `encoder`     | Windows hardware H.264 MFT, D3D11 BGRA→NV12 processing, bitrate changes, forced keyframes; capability negotiation                   |
| `decoder`     | Windows H.264 MFT with D3D11 output; rejects CPU output                                                                             |
| `viewer`      | Native Win32 window and DXGI flip presentation; no WebView video path                                                               |
| `diagnostic`  | Bounded local GPU/QUIC session, measured stages, app-visible result, cancellation                                                   |
| `transport`   | Quinn TLS 1.3, mutually trusted certificates, application authentication, datagrams, bounded frame assembly                         |
| `congestion`  | Selectable Quinn CUBIC/BBR; experimental application bitrate controller and packet pacer                                            |
| `fec`         | XOR parity and single-erasure recovery for groups of up to 16 shards; loss-dependent group selection                                |
| `nat`         | Bounded STUN binding request and XOR-MAPPED-ADDRESS parser for IPv4 and IPv6                                                        |
| `signaling`   | Session binding, domain-separated authentication transcripts and replay window                                                      |
| `identity`    | Signatures using the existing libsignal identity key; no messaging ratchet access                                                   |
| `permissions` | Default-deny grants bound to both machines, GitHub ID and the public identity key                                                   |
| `access`      | Host-owned access requests, explicit/automatic decisions, persistence format, cancellation and revocation                           |
| `directory`   | Signed per-device registration and encrypted directed GitHub control messages                                                       |
| `input`       | Permission/replay checks, state snapshots, 250 ms watchdog, disconnect/revocation cleanup; Windows keyboard/mouse SendInput backend |
| `audio`       | Bounded packet playout queue                                                                                                        |
| `live`        | Ephemeral offers, direct candidates, clock mapping, Windows GPU host/viewer loops and bounded native input capture                  |
| `daemon`      | Separate process, authenticated local IPC, configuration validation, live sessions and diagnostics                                  |

The Tauri media adapter is `src-tauri/src/remote_access.rs`; GitHub discovery and
access commands live in `src-tauri/src/remote_directory.rs`. React views live in
`components/remote-access/`; the conversation page mounts the navigation and global
incoming request component. It retains the conversation DOM when changing sections.
Neither the native engine nor the settings depend on the selected conversation.

## Machines, permissions and requests

Signing in registers the local installation with a persisted UUID, even when
Remote Desktop is never opened. Registration does not enable hosting. The friends
list follows the existing Noosphere contacts, including contacts with no registered
device. A discovered guest can receive per-machine grants while its hosting is
disabled or it is offline. Both users need a build supporting device registration;
older clients cannot advertise a machine ID. Unknown devices receive no grants.
The same UUID is shared across local accounts; a connection to that same physical
installation is rejected. Two computers using the same GitHub account have distinct
machine IDs and may request access to one another. Account equality does not grant
access. A new installation keeps its local libsignal key without replacing an
existing published messaging profile. This does not synchronize chat keys, contact
stores or message history to a second computer.

Each repository stores signed device records under `remote/devices/<machine>.json`.
These records expose a public identity key, machine UUID and numeric GitHub/repository
IDs. Repository ownership is checked against immutable IDs. Machine names, host
preferences, permissions, requests and decisions are encrypted in directed files
under `remote/mail/<sender-machine>/<recipient-machine>.json`.

Control envelopes use libsignal X25519 keys, a fresh ephemeral sending key,
HKDF-SHA256 and ChaCha20-Poly1305. A domain-separated libsignal signature covers the
recipient, sender, ephemeral key, nonce and ciphertext. Monotonic per-machine
counters are saved before publication; incoming counters survive restart. A mailbox
expires after at most 120 seconds. These control envelopes do not use or mutate
the messaging Double Ratchet and do not provide post-compromise secrecy against
compromise of the recipient's long-term private key. QUIC session keys are separate.

Grants and decisions are saved through DPAPI on Windows or the existing Linux secret
store. The UI edits access per user and includes the signed-in user for their other
machines. A user policy can be saved before a device is discovered. When that account
publishes a signed device registration, the engine materializes a separate grant
matching the exact numeric GitHub ID, identity key, requesting machine ID and host
machine ID. Revoking the policy removes it and every materialized device grant. A
changed key is verified again before a grant is materialized. The UI marks a
device whose key differs from the known contact identity and exposes its fingerprint
for explicit verification before approval. Without an explicit user policy or device
grant, every new device is default-deny.

Host settings enable access requests and approved Windows video sessions.
The host accepts a request automatically only for an explicit unattended grant,
intersecting requested permissions with that grant. Other requests produce a global
popup with screen, keyboard and mouse choices, independent memory and unattended
options, and refusal. Disabling access, revoking a grant or cancelling a request
invalidates existing decisions. A session receives only the intersection of requested
and approved permissions.

Discovery runs while the signed-in UI is open, independent of chat
selection. The UI polls every five seconds; directory scans are cached for 60 seconds,
mailboxes use conditional reads, and normal presence publication is limited to once
per 45 seconds (the polling cadence may make this 60 seconds). Explicit changes and
new decisions publish immediately. Manual refresh bypasses the scan cache. Failures
and changes to the contact list cause discovery to retry without waiting for the cache.
Failures remain visible; there is no offline delivery guarantee or always-on signaling daemon.
The current bounds are 32 device records per repository and 100 discovered machines.
Large contact lists require further GitHub request-budget work before production use.

## Authentication and local control

A QUIC session requires TLS 1.3 with mutual certificates. Certificate hashes,
both principals, an unpredictable session ID and a short expiration form the
session binding. Each party signs a transcript containing this binding, its role
and the TLS exporter. Application authentication has a five-second timeout;
0-RTT is disabled. IP addresses and login names never constitute identity.

The caller obtains the binding, certificates and candidates through authenticated
GitHub/libsignal machine signaling. The daemon does not receive the messaging
ratchet or independently open its store. Tauri
transfers the existing identity signing key through authenticated IPC, together
with the GitHub numeric ID and persisted machine ID. Bootstrap and request buffers
containing secrets are zeroed after use. Both loopback roles use that same local
identity; the CLI/packaged smoke instead uses a disposable test identity.

Local control uses Windows named pipes rejecting remote clients, or a Unix
socket with mode 0600. The parent transfers a random capability through an
inherited pipe, not command arguments or environment variables. Both sides prove
possession using direction-separated HMAC challenges. Requests are length-bounded
and time-limited. This protects against other OS users; code running as the same
user is inside the local trust boundary. Unix endpoints require a private parent
directory.

The diagnostic host exits after two idle minutes. Closing Tauri lets an active
bounded test finish and release its inputs. Changes of account or saved settings
request a graceful host shutdown before replacing it. It is not yet an unattended Windows agent or systemd
service. Windows requests elevation before creating an inbound UDP rule for the
exact executable path and verifies that rule before publishing a host offer.

## Media wire format

All integers are unsigned and big-endian. A media datagram starts with:

| Bytes     | Field                                         |
| --------- | --------------------------------------------- |
| 0–3       | `NSR1` protocol discriminator                 |
| 4         | Kind: video, audio, input, parity or feedback |
| 5         | Keyframe flag; other bits rejected            |
| 6–7       | Shard index                                   |
| 8–9       | Number of data shards                         |
| 10–11     | Reserved, must be zero                        |
| 12–19     | Frame number                                  |
| 20–27     | Packet sequence                               |
| 28–35     | Capture timestamp in microseconds             |
| 36–39     | Lifetime in microseconds                      |
| 40–43     | Encoded frame length                          |
| 44 onward | Payload                                       |

Datagrams are bounded to 1200 bytes and the negotiated QUIC path limit. Frames
are bounded to 4 MiB and 4096 shards. The receiver holds at most three incomplete
frames and 8 MiB of chunk payload. Expiration also runs when no packet arrives.
A newer complete frame retires older incomplete frames. Missing video references
prevent decoding until a keyframe arrives. Real-time media never use reliable
streams.

Authentication proofs carry a signed send time. Each peer estimates the remote clock
offset with the QUIC RTT and maps packet deadlines without assuming equal wall clocks.
The receiver samples its supplied clock after awaiting a datagram. The pacer
rejects packets that cannot be serialized before their deadline. QUIC datagrams
are not retransmitted. The deadline predicate for selective retransmission is
present. The guest sends bounded datagram feedback every 250 ms; a lost reference
requests a hardware IDR from the host.

FEC parity recovers one lost shard per group in the live session. Its group size
adapts to measured loss, and redundancy is disabled while RTT indicates congestion.
The application layer controller consumes QUIC RTT plus delivered/lost datagram
feedback and applies bitrate changes directly through Windows ICodecAPI. Quinn's
CUBIC controller remains underneath it. This has not been validated on a WAN.

## Reproducible checks

```sh
npm run format:check
npm run lint
npm test
npm run check
npm run bench:remote
npm run test:remote:e2e
npm run smoke:remote:live
```

`npm test` includes an actual two-endpoint QUIC exchange using libsignal
signatures, an incorrect-identity rejection test, and an independent host process
controlled through authenticated IPC after its bootstrap pipe closes.
It also exchanges encrypted access requests between two machine identities on one
account, restores remembered grants, checks automatic decisions and enforces the
resulting keyboard/mouse permissions through the native input guard. Changed keys,
other machine IDs, replay, expiry and revocation are covered. The folder-backed
laboratory does not contact live GitHub accounts. On Windows, `smoke:remote:live`
launches two processes, captures the real desktop and requires 60 GPU-presented frames.

The distributed laboratory can split those processes across two Windows systems.
It binds both QUIC endpoints to all local interfaces, advertises an explicit test
address, keeps the 120-second authenticated-session limit and writes machine-readable
host and guest reports to the shared folder. A VMware Workstation run on 2026-09-11
used a clean Windows 11 Enterprise 25H2 guest behind VMnet8 NAT and the physical
Windows host. With the guest firewall enabled and scoped to the laboratory binary,
both distinct same-account machine identities authenticated. The client reassembled
11 frames (90,112 bytes), dropped the deliberately incomplete frame without blocking,
accepted permitted input, and rejected input after revocation. Measured QUIC RTT was
1.031 ms at the host and 1.704 ms at the client. The portable 0.1.15 executable also
completed its Tauri bridge, social bridge, WebView2, brand-asset and synthetic-media
smoke test in that VM with no diagnostics.

After building the desktop package, `npm run smoke:remote` checks two host
processes using the packaged executable. It checks IPC authentication from Node
to Rust, independent process IDs, and acknowledged shutdown. The existing
`npm run smoke:tauri` remains a separate check for messaging-runtime bridges and
WebRTC media.

On a Windows machine with a desktop session and hardware H.264 MFT:

```sh
npm run bench:remote -- --media
npm run bench:remote:session
npm run smoke:remote:gpu
```

`--media` measures capture→hardware encode→GPU decode for five seconds.
`--session` additionally opens a native viewer and transports the actual encoded
frames over QUIC. `--gpu` tests the packaged application's host entry point and
authenticated IPC for a 15-second session. Keep the native window in the foreground
for the F24 and mouse-click tests. The pointer is restored unless the user moved it.
No screen contents are saved. GPU tests are explicit because
headless CI cannot supply the required desktop and driver.

The app test runs in the host, on a dedicated COM/media worker, with separate
Tokio network workers. The main React page never handles frames. It reports
encoder completion time, encoded-frame transfer time and decoder submission time.
Decoder submission is **not GPU completion latency**; accepted DXGI presents do
not measure photons or verify display visibility. Input-to-photon stays `null`.
The normal live diagnostic keeps both QUIC endpoints in one process and on one
monotonic clock. The distributed laboratory proves separate-computer connectivity
and authentication, but uses synthetic frame bytes and does not prove NAT traversal
outside the VMware network or hardware capture across the VM boundary. The source capture is limited to the default display adapter; changes
of desktop, GPU loss and monitor reconfiguration currently terminate the test.

The benchmark uses a fixed seed and 1800 frames at 60 FPS. It simulates fibre,
Wi-Fi and mobile paths, random loss, jitter, reordering, packet serialization,
a bounded router queue, and a bandwidth reduction between frames 600 and 1200.
It compares a fixed bitrate with the experimental adaptive controller. Encoded
frames are synthetic, independent byte buffers. FEC, codec reference dependencies
and Quinn congestion control are not part of this simulation. Results therefore
do not predict visual quality or end-to-end performance. Delivery percentiles
must be read alongside the dropped-frame count.

On Windows, measure native capture separately:

```sh
npm run bench:remote -- --capture
```

This acquires GPU textures for three seconds without saving screen contents.
The reported acquisition wait includes waiting for desktop updates; it is not
an encoder latency or a supported-FPS claim. In this capture-only mode, encode,
decode and input-to-photon remain `null`. Input-to-photon
requires a visible input marker and a physical display measurement.

## Remaining integration

Additional codec negotiation, cursor shape and multi-adapter handling, gamepad
support, clipboard transport and audio capture/playback remain to be implemented.
The Windows native test is H.264 only; keyboard/mouse injection has a backend, but
the automatic test injects only F24 and a pointer/click sequence into its own
foreground window. Linux now uses PipeWire/ScreenCast on Wayland, RemoteDesktop on
GNOME/KDE, native virtual input protocols on wlroots compositors, and an
XImage/XTest fallback on X11. NVENC, VA-API and QSV H.264 factories are selected when
the installed GStreamer stack and GPU expose them. Direct DMA-BUF negotiation and
hardware validation on each GPU family still need dedicated physical test machines.
Windows negotiates HEVC and AV1 preferences down to H.264 in this revision.
Preferences do not certify support for a resolution or frame rate on the current
machine.

Full ICE connectivity checks, consent freshness and TURN allocations are not
implemented. The host performs a bounded STUN lookup on the same UDP socket and
publishes local and server-reflexive candidates. STUN alone cannot ensure connectivity
through CGNAT. No TURN service is configured.

An unattended service lifecycle remains. The compact app overlay can end an active
session, and changing or revoking a grant immediately closes the current session.
No comparison with Parsec has been measured.

## References

- [QUIC DATAGRAM, RFC 9221](https://www.rfc-editor.org/rfc/rfc9221.html)
- [Quinn API](https://docs.rs/quinn/0.11.11/quinn/)
- [Desktop Duplication API](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api)
- [XDG RemoteDesktop portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)
