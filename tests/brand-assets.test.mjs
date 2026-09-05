import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const root = new URL('../', import.meta.url);

function pngSize(buffer) {
  assert.equal(buffer.toString('ascii', 1, 4), 'PNG');
  return {
    width: buffer.readUInt32BE(16),
    height: buffer.readUInt32BE(20),
  };
}

void test('les deux variantes carrées du logo sont embarquées', async () => {
  const [white, black] = await Promise.all([
    readFile(new URL('public/brand/logo-white.png', root)),
    readFile(new URL('public/brand/logo-black.png', root)),
  ]);
  assert.deepEqual(pngSize(white), { width: 512, height: 512 });
  assert.deepEqual(pngSize(black), { width: 512, height: 512 });
});

void test('l’animation de chargement est un MP4 embarqué et borné', async () => {
  const animation = await readFile(new URL('public/brand/loading.mp4', root));
  assert.equal(animation.toString('ascii', 4, 8), 'ftyp');
  assert.ok(animation.length > 1_000);
  assert.ok(animation.length < 1_000_000);
});

void test('la source de l’icône native conserve un fond transparent', async () => {
  const generator = await readFile(
    new URL('scripts/prepare-brand-assets.ps1', root),
    'utf8',
  );
  assert.match(
    generator,
    /\$iconGraphics\.Clear\(\[System\.Drawing\.Color\]::Transparent\)/,
  );
});
