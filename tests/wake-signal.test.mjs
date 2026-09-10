import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const root = new URL('../', import.meta.url);

void test('les étoiles sont analysées toutes les trois secondes', async () => {
  const page = await readFile(new URL('app/page.tsx', root), 'utf8');
  assert.match(page, /pollWakeSignals\(\)/);
  assert.match(page, /setInterval\([^]*3_000\)/);
  assert.match(page, /wake\.conversationIds\.map/);
  assert.match(page, /syncMessages\(conversationId, true\)/);
  assert.match(page, /wake\.socialChanged/);
  assert.match(page, /syncSocialState\(true, true\)/);
  assert.match(page, /desktop\.noosphere\.syncRequests\(\)/);
  assert.match(page, /socialRefreshPending/);
  assert.match(page, /socialRefreshPending = !\(await syncSocialState/);
});

void test('messages et appels produisent un changement d’étoile et une notification', async () => {
  const [commands, github, page] = await Promise.all([
    readFile(new URL('src-tauri/src/commands.rs', root), 'utf8'),
    readFile(new URL('src-tauri/src/github.rs', root), 'utf8'),
    readFile(new URL('app/page.tsx', root), 'utf8'),
  ]);
  assert.match(commands, /refresh_peer_wake_signal/);
  assert.match(commands, /Method::PUT/);
  assert.match(commands, /Method::DELETE/);
  assert.match(github, /application\/vnd\.github\.star\+json/);
  assert.match(page, /signalWake\(selectedConversation\.id\)/);
  assert.match(page, /Appel entrant/);
  assert.match(page, /startCall\('audio'\)/);
  assert.doesNotMatch(page, /Démarrer un appel vidéo/);
  assert.match(page, /notifyUser/);
  assert.match(page, /detail: 'Nouvelle activité'/);
  assert.match(page, /conversationId\?: string/);
  assert.match(page, /setSelectedId\(notification\.conversationId\)/);
  assert.match(
    page,
    /if \(!sendMessageSignal\(conversationId, message\.id\)\)/,
  );
});

void test('un appel attend la connexion directe au lieu d’échouer', async () => {
  const peer = await readFile(new URL('lib/use-direct-peer.ts', root), 'utf8');
  assert.match(peer, /callStatusRef\.current === 'outgoing'/);
  assert.match(peer, /pendingCallId/);
  assert.match(peer, /sendVoicePacket\([\s\S]*?'call-offer'/);
  assert.doesNotMatch(peer, /La connexion avec cet ami n’est pas encore prête/);
});
