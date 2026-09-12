import assert from 'node:assert/strict';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import { updatePercentage } from '../lib/app-update.ts';
import { generateLatestJson } from '../scripts/generate-latest-json.mjs';

test('computes bounded updater progress', () => {
  assert.equal(updatePercentage(25, 100), 25);
  assert.equal(updatePercentage(125, 100), 100);
  assert.equal(updatePercentage(1, undefined), null);
});

test('creates a signed manifest for every supported desktop package', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'noosphere-update-'));
  const releaseDirectory = path.join(directory, 'release-assets');
  const windowsDirectory = path.join(releaseDirectory, 'noosphere-windows');
  const linuxDirectory = path.join(releaseDirectory, 'noosphere-linux');

  try {
    await Promise.all([
      mkdir(windowsDirectory, { recursive: true }),
      mkdir(linuxDirectory, { recursive: true }),
    ]);
    const assets = [
      [windowsDirectory, 'Noosphere_0.1.18_x64-setup.exe'],
      [linuxDirectory, 'Noosphere_0.1.18_amd64.AppImage'],
      [linuxDirectory, 'Noosphere_0.1.18_amd64.deb'],
      [linuxDirectory, 'Noosphere_0.1.18_x86_64.rpm'],
    ];
    await Promise.all(
      assets.flatMap(([assetDirectory, name]) => [
        writeFile(path.join(assetDirectory, name), name),
        writeFile(path.join(assetDirectory, `${name}.sig`), `${name}-sig`),
      ]),
    );

    const { manifest } = await generateLatestJson({
      repository: 'Luffyxel/noosphere_client',
      tag: 'v0.1.18',
      releaseDirectory,
      publishedAt: new Date('2026-09-12T08:00:00.000Z'),
    });

    assert.equal(manifest.version, '0.1.18');
    assert.deepEqual(Object.keys(manifest.platforms).sort(), [
      'linux-x86_64-appimage',
      'linux-x86_64-deb',
      'linux-x86_64-rpm',
      'windows-x86_64-nsis',
    ]);
    for (const [target, platform] of Object.entries(manifest.platforms)) {
      assert.match(platform.url, /releases\/download\/v0\.1\.18\/Noosphere_/);
      assert.match(platform.signature, /-sig$/);
      assert.ok(target.includes('x86_64'));
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
