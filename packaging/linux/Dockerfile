FROM node:22.12.0-bookworm-slim AS node
FROM rust:1.98-bookworm AS rust

FROM ubuntu:22.04 AS build

ARG DEBIAN_FRONTEND=noninteractive
ARG APPIMAGE_RUNTIME_VERSION=20251108
ARG APPIMAGE_RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

COPY --from=node /usr/local/ /usr/local/
COPY --from=rust /usr/local/cargo/ /usr/local/cargo/
COPY --from=rust /usr/local/rustup/ /usr/local/rustup/

RUN apt-get update \
    && apt-get install --no-install-recommends -y \
      build-essential \
      ca-certificates \
      curl \
      dbus-x11 \
      file \
      gstreamer1.0-libav \
      gstreamer1.0-plugins-bad \
      gstreamer1.0-plugins-base \
      gstreamer1.0-plugins-good \
      gstreamer1.0-nice \
      gstreamer1.0-pipewire \
      gstreamer1.0-vaapi \
      gstreamer1.0-x \
      libayatana-appindicator3-dev \
      libfuse2 \
      librsvg2-dev \
      libgstreamer1.0-dev \
      libgstreamer-plugins-base1.0-dev \
      libssl-dev \
      libwebkit2gtk-4.1-dev \
      libwayland-dev \
      libxkbcommon-dev \
      libxkbcommon-x11-0 \
      libxdo-dev \
      patchelf \
      protobuf-compiler \
      rpm \
      wget \
      xvfb \
    && rm -rf /var/lib/apt/lists/*

RUN curl --fail --location --retry 5 --retry-all-errors \
      --output /usr/local/lib/appimage-runtime \
      "https://github.com/AppImage/type2-runtime/releases/download/${APPIMAGE_RUNTIME_VERSION}/runtime-x86_64" \
    && echo "${APPIMAGE_RUNTIME_SHA256}  /usr/local/lib/appimage-runtime" | sha256sum --check

RUN apt-get update \
    && apt-get install --no-install-recommends -y xdg-utils \
    && rm -rf /var/lib/apt/lists/*

ENV CARGO_HOME=/usr/local/cargo
ENV RUSTUP_HOME=/usr/local/rustup
ENV RUSTUP_TOOLCHAIN=1.98.0-x86_64-unknown-linux-gnu
ENV PATH="/usr/local/cargo/bin:${PATH}"
ENV NO_STRIP=true
ENV LDAI_RUNTIME_FILE=/usr/local/lib/appimage-runtime
ENV APPIMAGETOOL_RUNTIME_FILE=/usr/local/lib/appimage-runtime
ENV CEF_PATH=/var/cache/noosphere-cef

RUN rustup component add rustfmt clippy

WORKDIR /workspace

COPY package.json package-lock.json ./
RUN npm ci

COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/var/cache/noosphere-cef \
    npm run build:linux:binary

FROM build AS package
ARG LINUXDEPLOY_SHA256=e762bea85c8eb0d4b3508d46e5c1f037f717d0f9303ae3b4aafc8b04991fa1ef
ARG APPIMAGE_PLUGIN_SHA256=0441769ab38009504d2678c38cd7e526955388dd30a215b4a20afaa5471652f2
ARG APP_RUN_SHA256=f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f
ENV LINUXDEPLOY_PATH=/root/.cache/tauri/linuxdeploy-x86_64.AppImage
ENV LINUXDEPLOY_PLUGIN_APPIMAGE_PATH=/root/.cache/tauri/linuxdeploy-plugin-appimage.AppImage
ENV APP_RUN_PATH=/root/.cache/tauri/AppRun-x86_64
RUN --mount=type=cache,target=/root/.cache/tauri \
    curl --fail --location --retry 5 --retry-all-errors \
      --output "${LINUXDEPLOY_PATH}" \
      https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy/linuxdeploy-x86_64.AppImage \
    && curl --fail --location --retry 5 --retry-all-errors \
      --output "${LINUXDEPLOY_PLUGIN_APPIMAGE_PATH}" \
      https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage \
    && curl --fail --location --retry 5 --retry-all-errors \
      --output "${APP_RUN_PATH}" \
      https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64 \
    && echo "${LINUXDEPLOY_SHA256}  ${LINUXDEPLOY_PATH}" | sha256sum --check \
    && echo "${APPIMAGE_PLUGIN_SHA256}  ${LINUXDEPLOY_PLUGIN_APPIMAGE_PATH}" | sha256sum --check \
    && echo "${APP_RUN_SHA256}  ${APP_RUN_PATH}" | sha256sum --check \
    && chmod 0755 "${LINUXDEPLOY_PATH}" "${LINUXDEPLOY_PLUGIN_APPIMAGE_PATH}" "${APP_RUN_PATH}"
RUN --mount=type=cache,target=/root/.cache/tauri npm run bundle:linux

FROM package AS test
RUN RUST_BACKTRACE=full dbus-run-session -- xvfb-run -a npm run smoke:tauri

FROM package AS export
CMD ["node", "scripts/export-linux-artifacts.mjs", "/output"]
