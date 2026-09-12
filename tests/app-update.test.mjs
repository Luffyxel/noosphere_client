import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import { updatePercentage } from '../lib/app-update.ts';
import { checkReleaseVersion } from '../scripts/check-release-version.mjs';
import { createReleaseAliases } from '../scripts/create-release-aliases.mjs';
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

test('keeps every application version synchronized', async () => {
  assert.match(await checkReleaseVersion(), /^\d+\.\d+\.\d+/);
  await assert.rejects(
    checkReleaseVersion('v999.999.999'),
    /must be 999\.999\.999/,
  );
});

test('creates stable aliases for the latest release links', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'noosphere-aliases-'));
  const windowsDirectory = path.join(directory, 'noosphere-windows');
  const linuxDirectory = path.join(directory, 'noosphere-linux');
  try {
    await Promise.all([
      mkdir(windowsDirectory, { recursive: true }),
      mkdir(linuxDirectory, { recursive: true }),
    ]);
    const assets = [
      [windowsDirectory, 'Noosphere_0.1.17_x64-setup.exe'],
      [windowsDirectory, 'Noosphere_0.1.17_portable_x64.exe'],
      [linuxDirectory, 'Noosphere_0.1.17_amd64.AppImage'],
      [linuxDirectory, 'Noosphere_0.1.17_amd64.deb'],
      [linuxDirectory, 'Noosphere_0.1.17_x86_64.rpm'],
    ];
    await Promise.all(
      assets.map(([assetDirectory, name]) =>
        writeFile(path.join(assetDirectory, name), name),
      ),
    );

    const created = await createReleaseAliases(directory);
    assert.equal(created.length, 5);
    assert.equal(
      await readFile(
        path.join(directory, 'Noosphere-Windows-Setup-x64.exe'),
        'utf8',
      ),
      'Noosphere_0.1.17_x64-setup.exe',
    );
    assert.equal(
      await readFile(
        path.join(directory, 'Noosphere-Linux-AppImage-x64.AppImage'),
        'utf8',
      ),
      'Noosphere_0.1.17_amd64.AppImage',
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
