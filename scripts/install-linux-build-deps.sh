#!/usr/bin/env bash
set -euo pipefail

if [[ "${EUID}" -eq 0 ]]; then
  elevate=()
else
  elevate=(sudo)
fi

# GitHub's Ubuntu 22.04 image can temporarily expose mismatched package versions
# through its Azure mirror. The canonical Ubuntu mirror carries matching runtime
# and development packages, including libunwind8/libunwind-dev.
mirror_list=/etc/apt/apt-mirrors.txt
if [[ -f "${mirror_list}" ]]; then
  "${elevate[@]}" sed -i \
    's|http://azure.archive.ubuntu.com/ubuntu|http://archive.ubuntu.com/ubuntu|g' \
    "${mirror_list}"
fi

"${elevate[@]}" rm -rf /var/lib/apt/lists/*
"${elevate[@]}" apt-get -o Acquire::Retries=5 update
"${elevate[@]}" env DEBIAN_FRONTEND=noninteractive \
  apt-get -o Acquire::Retries=5 install --no-install-recommends -y \
  libunwind-dev \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libwayland-dev \
  libxkbcommon-dev \
  libxkbcommon-x11-0 \
  libgstreamer1.0-dev \
  libgstreamer-plugins-base1.0-dev \
  patchelf \
  protobuf-compiler \
  xvfb \
  gstreamer1.0-libav \
  gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-base \
  gstreamer1.0-plugins-good \
  gstreamer1.0-tools \
  gstreamer1.0-nice \
  gstreamer1.0-pipewire \
  gstreamer1.0-vaapi \
  gstreamer1.0-x \
  rpm \
  dbus-x11
