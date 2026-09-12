#!/usr/bin/env bash
set -euo pipefail

binary="${1:-/workspace/src-tauri/target/release/noosphere-remote-bench}"
output="${2:-/opt/noosphere-remote-probe}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
rm -rf "${output}"
mkdir -p "${output}/bin" "${output}/lib" "${output}/gstreamer-1.0" \
  "${output}/pipewire-0.3" "${output}/spa-0.2" \
  "${output}/share/X11" "${output}/share/pipewire"
cp "${binary}" "${output}/bin/noosphere-remote-bench"
cp -a /usr/share/X11/xkb "${output}/share/X11/xkb"

declare -A copied=()
declare -a pending=()

queue_dependencies() {
  local file="$1"
  while IFS= read -r dependency; do
    [[ -n "${dependency}" && -f "${dependency}" ]] || continue
    if [[ -z "${copied[${dependency}]:-}" ]]; then
      copied["${dependency}"]=1
      pending+=("${dependency}")
    fi
  done < <(ldd "${file}" 2>/dev/null | awk '/=> \/.* \(/ { print $3 } /^\// { print $1 }')
}

queue_dependencies "${binary}"
for loader in /lib64/ld-linux-x86-64.so.2 /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2; do
  if [[ -e "${loader}" ]]; then
    cp -L "${loader}" "${output}/lib/ld-linux-x86-64.so.2"
    break
  fi
done

for plugin in /usr/lib/x86_64-linux-gnu/gstreamer-1.0/*.so; do
  cp -L "${plugin}" "${output}/gstreamer-1.0/$(basename "${plugin}")"
  queue_dependencies "${plugin}"
done

# libpipewire loads these modules at runtime, so ldd cannot discover them.
# Keep them beside the probe to avoid depending on the target distro layout.
cp -a /usr/lib/x86_64-linux-gnu/pipewire-0.3/. "${output}/pipewire-0.3/"
cp -a /usr/lib/x86_64-linux-gnu/spa-0.2/. "${output}/spa-0.2/"
cp -a /usr/share/pipewire/. "${output}/share/pipewire/"
while IFS= read -r module; do
  queue_dependencies "${module}"
done < <(find "${output}/pipewire-0.3" "${output}/spa-0.2" -type f -name '*.so')

scanner=''
for candidate in /usr/lib/*/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner; do
  if [[ -x "${candidate}" ]]; then
    scanner="${candidate}"
    break
  fi
done
if [[ -z "${scanner}" ]]; then
  echo 'GStreamer plugin scanner not found' >&2
  exit 1
fi
cp "${scanner}" "${output}/bin/gst-plugin-scanner.real"
queue_dependencies "${scanner}"

cc -O2 -static "${script_dir}/linux-probe-scanner-launcher.c" \
  -o "${output}/bin/gst-plugin-scanner"

index=0
while (( index < ${#pending[@]} )); do
  dependency="${pending[${index}]}"
  index=$((index + 1))
  cp -L "${dependency}" "${output}/lib/$(basename "${dependency}")"
  queue_dependencies "${dependency}"
done

cat > "${output}/run-hyprland-probe.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export GST_PLUGIN_PATH="${root}/gstreamer-1.0"
export GST_PLUGIN_SYSTEM_PATH_1_0="${root}/gstreamer-1.0"
export GST_PLUGIN_SCANNER="${root}/bin/gst-plugin-scanner"
export GST_REGISTRY="${root}/gst-registry.bin"
export XKB_CONFIG_ROOT="${root}/share/X11/xkb"
export SPA_PLUGIN_DIR="${root}/spa-0.2"
export PIPEWIRE_MODULE_DIR="${root}/pipewire-0.3"
export PIPEWIRE_CONFIG_DIR="${root}/share/pipewire"
export NOOSPHERE_REMOTE_TEST_SOURCE=1
export NOOSPHERE_REMOTE_TEST_SINK=1
unset GIO_EXTRA_MODULES GIO_MODULE_DIR GTK_PATH
exec "${root}/lib/ld-linux-x86-64.so.2" \
  --library-path "${root}/lib" \
  "${root}/bin/noosphere-remote-bench" --linux-probe
EOF
chmod +x "${output}/run-hyprland-probe.sh"

cat > "${output}/run-hyprland-capture-probe.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export GST_PLUGIN_PATH="${root}/gstreamer-1.0"
export GST_PLUGIN_SYSTEM_PATH_1_0="${root}/gstreamer-1.0"
export GST_PLUGIN_SCANNER="${root}/bin/gst-plugin-scanner"
export GST_REGISTRY="${root}/gst-registry-capture.bin"
export XKB_CONFIG_ROOT="${root}/share/X11/xkb"
export SPA_PLUGIN_DIR="${root}/spa-0.2"
export PIPEWIRE_MODULE_DIR="${root}/pipewire-0.3"
export PIPEWIRE_CONFIG_DIR="${root}/share/pipewire"
export NOOSPHERE_REMOTE_TEST_SINK=1
export NOOSPHERE_REMOTE_ALLOW_SOFTWARE=1
unset NOOSPHERE_REMOTE_TEST_SOURCE GIO_EXTRA_MODULES GIO_MODULE_DIR GTK_PATH
exec "${root}/lib/ld-linux-x86-64.so.2" \
  --library-path "${root}/lib" \
  "${root}/bin/noosphere-remote-bench" --linux-capture-probe
EOF
chmod +x "${output}/run-hyprland-capture-probe.sh"

cat > "${output}/run-hyprland-pointer-click.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export XKB_CONFIG_ROOT="${root}/share/X11/xkb"
unset GIO_EXTRA_MODULES GIO_MODULE_DIR GTK_PATH
exec "${root}/lib/ld-linux-x86-64.so.2" \
  --library-path "${root}/lib" \
  "${root}/bin/noosphere-remote-bench" --linux-pointer-click "${1}" "${2}"
EOF
chmod +x "${output}/run-hyprland-pointer-click.sh"

cat > "${output}/run-hyprland-activate.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export XKB_CONFIG_ROOT="${root}/share/X11/xkb"
unset GIO_EXTRA_MODULES GIO_MODULE_DIR GTK_PATH
exec "${root}/lib/ld-linux-x86-64.so.2" \
  --library-path "${root}/lib" \
  "${root}/bin/noosphere-remote-bench" --linux-activate
EOF
chmod +x "${output}/run-hyprland-activate.sh"
tar -C "$(dirname "${output}")" -czf "${output}.tar.gz" "$(basename "${output}")"
