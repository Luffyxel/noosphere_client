# Linux remote desktop validation

The Linux engine is tested through the same `noosphere-remote` Rust crate used by
the host daemon and Tauri IPC. The tests do not use the React WebRTC path.

## NixOS and Hyprland

The session probe was executed on 11 September 2026 in a VMware guest running
NixOS 26.05, Hyprland 0.55.4, PipeWire 1.6.6, xdg-desktop-portal 1.20.4 and
xdg-desktop-portal-hyprland 1.3.12.

The live session exposed ScreenCast portal version 5 with monitor, window and
virtual-source capture. Hyprland exposed `virtual-keyboard` and
`wlr-virtual-pointer` for input. Four complementary probes passed:

- the real portal/PipeWire capture selected the VM's `Virtual-1` monitor,
  encoded 70 H.264 frames and decoded 65 frames in 3.26 seconds;
- the authenticated loopback session encoded, packetized over real QUIC
  datagrams, reassembled and decoded 90 of 90 synthetic frames in 3 seconds.
  Keyboard and pointer packets arrived, and revocation blocked the next input
  event.
- the native Hyprland input backend moved the real compositor cursor from
  `608, 320` to `959, 399` and injected a click through
  `wlr-virtual-pointer`.
- the final `Noosphere_0.1.17_amd64.AppImage` repeated the real portal capture
  from its packaged Tauri binary: 75 H.264 frames encoded and 47 decoded in
  3.23 seconds. Its SHA-256 is
  `8bc203db440c741e36db3554e81d7eecb27acd5de92f952ef5ab29e23262997b`.

The real capture exposed a variable-rate PipeWire stream (`0/1`, maximum 120
Hz). This found and fixed a negotiation bug caused by treating every compositor
stream as fixed-rate. The pipeline now normalizes variable-rate desktop frames
to the requested 30/60/120/144/240 Hz rate while retaining a one-frame leaky
queue. The automated VM picker only replaces the interactive screen choice; all
capture frames still come from the real Hyprland compositor through the real
portal and PipeWire remote.

The VMware SVGA device exposed `/dev/dri/card0` and `/dev/dri/renderD128` but no
H.264 hardware encoder. The test therefore used the explicit diagnostic-only
OpenH264 fallback. This validates the compositor, portal, media, transport and
permission paths; it does not measure a physical NVENC, VA-API or QSV encoder.

The NixOS services used by the test are declared in
[`packaging/nixos/remote-test.nix`](../packaging/nixos/remote-test.nix). Custom
Hyprland launchers must import `WAYLAND_DISPLAY`, `XDG_CURRENT_DESKTOP` and
`HYPRLAND_INSTANCE_SIGNATURE` into the systemd user activation environment.

## X11 and headless regression tests

The Debian Bookworm container in
[`packaging/linux/Dockerfile.remote-test`](../packaging/linux/Dockerfile.remote-test)
runs the full Rust suite. It also starts Xvfb at 640×360 and verifies that the X11
fallback captures and decodes 12 H.264 frames, injects an `A` key through XTest,
observes the pressed key through X11, releases it, and moves the pointer to the
requested absolute coordinate.

The same suite checks:

- encrypted access grants for different machines on the same GitHub account;
- exact machine and Noosphere identity binding;
- authenticated QUIC endpoints and media datagrams;
- loss-tolerant frame assembly, frame expiry and single-erasure FEC recovery;
- bitrate reduction on bandwidth collapse and deadline-aware packet pacing;
- daemon IPC authentication, input ordering and immediate revocation;
- synthetic Linux H.264 encode and actual decoded output.
- a static Wayland desktop that emits only its first frame; the viewer retries
  that initial access unit locally until the decoder presents it, without adding
  network traffic or replaying it once the live stream advances.

## Desktop coverage

| Environment                   | Capture                   | Input                                  | Validation                                                                   |
| ----------------------------- | ------------------------- | -------------------------------------- | ---------------------------------------------------------------------------- |
| NixOS 26.05 + Hyprland 0.55.4 | XDG ScreenCast + PipeWire | virtual-keyboard + wlr-virtual-pointer | Real capture, input injection, authenticated session and final AppImage      |
| GNOME Wayland                 | XDG ScreenCast + PipeWire | XDG RemoteDesktop                      | Portal capability and permission-routing tests; uses the standard portal API |
| KDE Plasma Wayland            | XDG ScreenCast + PipeWire | XDG RemoteDesktop                      | Portal capability and permission-routing tests; uses the standard portal API |
| Sway/wlroots                  | XDG ScreenCast + PipeWire | virtual-keyboard + wlr-virtual-pointer | Same protocol backend exercised on Hyprland                                  |
| X11 desktops                  | ximagesrc                 | XTest                                  | Real capture, decode, key and pointer assertions under Xvfb                  |

GNOME, KDE and Sway have not been run in separate graphical VMs yet. Their
shared portal and wlroots paths are covered in the automated suite, while the
full compositor test currently runs on Hyprland. A new compositor-specific
failure must remain visible as a backend error instead of silently falling back
to the React/WebRTC path.

Run the container suite with:

```bash
docker build --progress=plain \
  -f packaging/linux/Dockerfile.remote-test \
  -t noosphere-remote-linux-test .
```

The Linux job in `.github/workflows/ci.yml` also invokes
`scripts/test-linux-remote.sh`, including its Xvfb capture/input test, on every
push and pull request.
