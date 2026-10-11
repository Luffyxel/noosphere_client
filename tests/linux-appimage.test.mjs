import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { removeHostLibraries } from '../scripts/linux-appimage-libraries.mjs';

void test('AppImage keeps CEF and media libraries without shadowing desktop drivers', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'noosphere-appimage-'));
  try {
    const directories = [
      path.join(root, 'lib'),
      path.join(root, 'lib', 'Noosphere'),
    ];
    const host = [
      'libstdc++.so.6',
      'libgcc_s.so.1',
      'libGL.so.1',
      'libEGL.so.1',
      'libGLX_mesa.so.0',
      'libgbm.so.1',
      'libdrm_amdgpu.so.1',
      'libva.so.2',
      'libva-x11.so.2',
      'libfontconfig.so.1',
      'libfreetype.so.6',
      'libharfbuzz.so.0',
      'libxkbcommon.so.0',
      'libxkbcommon-x11.so.0',
      'libssl.so.3',
      'libcrypto.so.3',
      'liblzma.so.5',
    ];
    const bundled = [
      'libcef.so',
      'libEGL.so',
      'libGLESv2.so',
      'libvk_swiftshader.so',
      'libvulkan.so.1',
      'libsqlite3.so.0',
      'libgstreamer-1.0.so.0',
      'libpipewire-0.3.so.0',
    ];
    for (const directory of directories) {
      await mkdir(directory, { recursive: true });
      for (const name of [...host, ...bundled])
        await writeFile(path.join(directory, name), name);
    }
    await removeHostLibraries(directories);
    for (const directory of directories) {
      assert.deepEqual(
        (await readdir(directory))
          .filter((name) => name !== 'Noosphere')
          .sort(),
        bundled.toSorted(),
      );
    }
    await removeHostLibraries(directories);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
