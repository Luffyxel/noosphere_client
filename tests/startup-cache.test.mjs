import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const root = new URL('../', import.meta.url);

void test('les contacts locaux sont affichés avant la synchronisation réseau', async () => {
  const [page, bridge, commands] = await Promise.all([
    readFile(new URL('app/page.tsx', root), 'utf8'),
    readFile(new URL('lib/tauri-bridge.ts', root), 'utf8'),
    readFile(new URL('src-tauri/src/commands.rs', root), 'utf8'),
  ]);

  assert.match(page, /social = await desktop\.noosphere\.cachedState\(\)/);
  assert.match(page, /initialSocialState\.conversations/);
  assert.match(bridge, /invoke<SocialState>\('noosphere_cached_state'\)/);
  assert.match(commands, /pub async fn noosphere_cached_state/);
});

void test('la restauration charge aussi le cache social chiffré', async () => {
  const state = await readFile(new URL('src-tauri/src/state.rs', root), 'utf8');

  const restore = state.slice(
    state.indexOf('pub async fn restore_session'),
    state.indexOf('pub async fn save_session'),
  );
  assert.match(
    restore,
    /load_or_create_identity\(returned\.id, returned\.repository\.id\)/,
  );
});
