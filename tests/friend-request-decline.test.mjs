import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const root = new URL('../', import.meta.url);

void test('une demande d’ami peut être refusée sans réapparaître', async () => {
  const [page, bridge, runtime, commands, state] = await Promise.all([
    readFile(new URL('app/page.tsx', root), 'utf8'),
    readFile(new URL('lib/tauri-bridge.ts', root), 'utf8'),
    readFile(new URL('src-tauri/src/lib.rs', root), 'utf8'),
    readFile(new URL('src-tauri/src/commands.rs', root), 'utf8'),
    readFile(new URL('src-tauri/src/state.rs', root), 'utf8'),
  ]);

  assert.match(page, /Refuser/);
  assert.match(page, /desktop\.noosphere\.declineFriendRequest\(request\.id\)/);
  assert.match(
    bridge,
    /invoke<\{ declined: true \}>\('noosphere_decline_friend_request'/,
  );
  assert.match(runtime, /commands::noosphere_decline_friend_request/);
  assert.match(commands, /declined_request_ids\.contains\(id\)/);
  assert.match(state, /pub declined_request_ids: Vec<String>/);
});
