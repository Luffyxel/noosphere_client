import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const root = new URL('../', import.meta.url);

void test('la version Windows est une application graphique sans console', async () => {
  const entrypoint = await readFile(
    new URL('src-tauri/src/main.rs', root),
    'utf8',
  );
  assert.match(
    entrypoint,
    /cfg_attr\(not\(debug_assertions\), windows_subsystem = "windows"\)/,
  );
});

void test('la version distribuée embarque son interface sans serveur web', async () => {
  const configuration = JSON.parse(
    await readFile(new URL('src-tauri/tauri.conf.json', root), 'utf8'),
  );
  assert.equal(configuration.build.frontendDist, '../dist-tauri');
  assert.equal(configuration.bundle.active, true);
  assert.equal(typeof configuration.build.devUrl, 'string');
});

void test('WebView2 accorde uniquement les permissions micro et caméra', async () => {
  const runtime = await readFile(new URL('src-tauri/src/lib.rs', root), 'utf8');
  assert.match(runtime, /COREWEBVIEW2_PERMISSION_KIND_MICROPHONE/);
  assert.match(runtime, /COREWEBVIEW2_PERMISSION_KIND_CAMERA/);
  assert.match(runtime, /COREWEBVIEW2_PERMISSION_STATE_ALLOW/);
  assert.doesNotMatch(runtime, /COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION/);
});

void test('le test Windows reste exécutable sans périphérique média', async () => {
  const [smoke, ci, release] = await Promise.all([
    readFile(new URL('lib/tauri-smoke.ts', root), 'utf8'),
    readFile(new URL('.github/workflows/ci.yml', root), 'utf8'),
    readFile(new URL('.github/workflows/release.yml', root), 'utf8'),
  ]);
  assert.match(smoke, /createSyntheticMedia/);
  for (const workflow of [ci, release]) {
    assert.match(workflow, /NOOSPHERE_SMOKE_SYNTHETIC_MEDIA: '1'/);
    assert.match(workflow, /NOOSPHERE_SMOKE_REQUIRE_DEVICE_CAPTURE: '0'/);
  }
});

void test('l’AppImage embarque SQLite pour le stockage NSS de Chromium', async () => {
  const finalizer = await readFile(
    new URL('scripts/finalize-linux-appimage.mjs', root),
    'utf8',
  );
  assert.match(finalizer, /libsqlite3\.so\.0/);
  assert.match(finalizer, /realpath\(sqliteLibrary\)/);
});

void test('les téléchargements Linux pointent vers les fichiers de la version', async () => {
  const packageMetadata = JSON.parse(
    await readFile(new URL('package.json', root), 'utf8'),
  );
  const readmes = await Promise.all(
    ['README.md', 'README.fr.md'].map((path) =>
      readFile(new URL(path, root), 'utf8'),
    ),
  );
  const version = packageMetadata.version;
  const assets = [
    `Noosphere_${version}_amd64.AppImage`,
    `Noosphere_${version}_amd64.deb`,
    `Noosphere_${version}_x86_64.rpm`,
    'SHA256SUMS.txt',
  ];

  for (const readme of readmes) {
    for (const asset of assets) {
      assert.ok(
        readme.includes(
          `https://github.com/Luffyxel/noosphere_client/releases/download/v${version}/${asset}`,
        ),
      );
    }
  }
});
