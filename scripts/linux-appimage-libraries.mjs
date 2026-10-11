import { readdir, rm } from 'node:fs/promises';
import path from 'node:path';

const hostLibraries = new Set([
  'libstdc++.so.6',
  'libgcc_s.so.1',
  'libGL.so.1',
  'libGLX.so.0',
  'libGLdispatch.so.0',
  'libOpenGL.so.0',
  'libEGL.so.1',
  'libGLX_mesa.so.0',
  'libEGL_mesa.so.0',
  'libgbm.so.1',
  'libglapi.so.0',
  'libdrm.so.2',
  'libdrm_amdgpu.so.1',
  'libdrm_radeon.so.1',
  'libdrm_nouveau.so.2',
  'libdrm_intel.so.1',
  'libva.so.2',
  'libva-drm.so.2',
  'libva-x11.so.2',
  'libva-wayland.so.2',
  'libva-glx.so.2',
  'libfontconfig.so.1',
  'libfreetype.so.6',
  'libharfbuzz.so.0',
  'libxkbcommon.so.0',
  'libxkbcommon-x11.so.0',
  'libssl.so.3',
  'libcrypto.so.3',
  'liblzma.so.5',
  'libexpat.so.1',
  'libX11.so.6',
  'libX11-xcb.so.1',
  'libxcb.so.1',
  'libwayland-client.so.0',
  'libwayland-server.so.0',
  'libwayland-cursor.so.0',
  'libwayland-egl.so.1',
]);

export function usesHostLibrary(name) {
  return hostLibraries.has(name);
}

export async function removeHostLibraries(directories) {
  for (const directory of directories) {
    for (const name of await readdir(directory)) {
      if (usesHostLibrary(name)) {
        await rm(path.join(directory, name));
      }
    }
  }
}
