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

void test('l’AppImage embarque SQLite pour le stockage NSS de Chromium', async () => {
  const finalizer = await readFile(
    new URL('scripts/finalize-linux-appimage.mjs', root),
    'utf8',
  );
  assert.match(finalizer, /libsqlite3\.so\.0/);
  assert.match(finalizer, /realpath\(sqliteLibrary\)/);
});
