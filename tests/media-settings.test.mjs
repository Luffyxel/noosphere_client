import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const root = new URL('../', import.meta.url);

void test('les paramètres proposent un test micro et trois niveaux de réduction', async () => {
  const page = await readFile(new URL('app/page.tsx', root), 'utf8');
  assert.match(page, /Test du micro/);
  assert.match(page, /Niveau du microphone/);
  assert.match(page, /<option value="off">Désactivée<\/option>/);
  assert.match(page, /<option value="standard">Standard<\/option>/);
  assert.match(page, /<option value="strong">Renforcée<\/option>/);
});

void test('un appel commence en audio et permet ensuite d’activer la caméra', async () => {
  const [page, peer] = await Promise.all([
    readFile(new URL('app/page.tsx', root), 'utf8'),
    readFile(new URL('lib/use-direct-peer.ts', root), 'utf8'),
  ]);
  assert.match(page, /startCall\('audio'\)/);
  assert.doesNotMatch(page, /Démarrer un appel vidéo/);
  assert.match(page, /aria-label="Activer la caméra"/);
  assert.match(peer, /captureCamera/);
  assert.match(peer, /videoSenderRef\.current\.replaceTrack\(track\)/);
});
