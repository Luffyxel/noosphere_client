import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const root = new URL('../', import.meta.url);

void test('le profil d’un ami permet de le retirer de la liste locale', async () => {
  const [page, bridge, runtime] = await Promise.all([
    readFile(new URL('app/page.tsx', root), 'utf8'),
    readFile(new URL('lib/tauri-bridge.ts', root), 'utf8'),
    readFile(new URL('src-tauri/src/lib.rs', root), 'utf8'),
  ]);

  assert.match(page, /function FriendProfileDialog/);
  assert.match(page, /Supprimer de mes amis/);
  assert.match(page, /desktop\.noosphere\.removeFriend\(conversation\.id\)/);
  assert.match(
    bridge,
    /invoke<\{ removed: true \}>\('noosphere_remove_friend'/,
  );
  assert.match(runtime, /commands::noosphere_remove_friend/);
});
