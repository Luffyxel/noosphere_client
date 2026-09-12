#!/usr/bin/env bash
set -euo pipefail

required=(pipewiresrc ximagesrc h264parse decodebin)
for plugin in "${required[@]}"; do
  gst-inspect-1.0 "${plugin}" >/dev/null
done

if ! gst-inspect-1.0 nvh264enc >/dev/null 2>&1 \
  && ! gst-inspect-1.0 vah264lpenc >/dev/null 2>&1 \
  && ! gst-inspect-1.0 vah264enc >/dev/null 2>&1 \
  && ! gst-inspect-1.0 qsvh264enc >/dev/null 2>&1 \
  && ! gst-inspect-1.0 x264enc >/dev/null 2>&1 \
  && ! gst-inspect-1.0 openh264enc >/dev/null 2>&1; then
  echo 'No H.264 encoder available' >&2
  exit 1
fi

cargo test --manifest-path src-tauri/Cargo.toml -p noosphere-remote -- --nocapture
cargo clippy --manifest-path src-tauri/Cargo.toml -p noosphere-remote \
  --all-targets -- -D warnings

if [[ -n "${WAYLAND_DISPLAY:-}" && -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]]; then
  NOOSPHERE_REMOTE_REQUIRE_PORTAL=1 \
    cargo test --manifest-path src-tauri/Cargo.toml -p noosphere-remote \
      --test linux_portal -- --nocapture
fi

if command -v Xvfb >/dev/null 2>&1; then
  Xvfb :99 -screen 0 640x360x24 -nolisten tcp >/tmp/noosphere-xvfb.log 2>&1 &
  xvfb_pid=$!
  trap 'kill "${xvfb_pid}" >/dev/null 2>&1 || true' EXIT
  sleep 1
  env -u WAYLAND_DISPLAY \
    -u NOOSPHERE_REMOTE_TEST_SOURCE \
    -u NOOSPHERE_REMOTE_TEST_SINK \
    DISPLAY=:99 \
    NOOSPHERE_REMOTE_ALLOW_SOFTWARE=1 \
    NOOSPHERE_REMOTE_REQUIRE_X11=1 \
    cargo test --manifest-path src-tauri/Cargo.toml -p noosphere-remote \
      --test linux_x11 -- --nocapture
  kill "${xvfb_pid}" >/dev/null 2>&1 || true
  trap - EXIT
fi
