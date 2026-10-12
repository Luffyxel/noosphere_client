import assert from 'node:assert/strict';
import test from 'node:test';
import { utils, write } from 'cfb';
import { legacyWordText } from '../lib/noosnote/legacy-word.ts';

function legacyFile(text, mutate) {
  const textBytes = Buffer.from(text, 'utf16le');
  const word = Buffer.alloc(1024 + textBytes.length);
  word.writeUInt16LE(0xa5ec, 0);
  word.writeUInt16LE(14, 0x20);
  word.writeUInt16LE(0x0200, 0x0a);
  word.writeUInt32LE(text.length, 0x4c);
  word.writeUInt32LE(0, 0x1a2);
  word.writeUInt32LE(21, 0x1a6);
  textBytes.copy(word, 1024);
  const table = Buffer.alloc(21);
  table[0] = 2;
  table.writeUInt32LE(16, 1);
  table.writeUInt32LE(text.length, 9);
  table.writeUInt32LE(1024, 15);
  mutate?.(word, table);
  const container = utils.cfb_new();
  utils.cfb_add(container, 'WordDocument', word);
  utils.cfb_add(container, '1Table', table);
  return new Uint8Array(write(container, { type: 'buffer', fileType: 'cfb' }));
}

void test('legacy Word imports Unicode text and keeps visible field content', () => {
  const field =
    String.fromCharCode(19) +
    'HYPERLINK hidden' +
    String.fromCharCode(20) +
    'Lien visible' +
    String.fromCharCode(21);
  assert.equal(
    legacyWordText(legacyFile(`Français été\r${field}\rAnglais`)),
    'Français été\nLien visible\nAnglais',
  );
});

void test('legacy Word refuses protected files and broken piece offsets', () => {
  assert.throws(
    () =>
      legacyWordText(
        legacyFile('Cours', (word) => word.writeUInt16LE(0x0300, 0x0a)),
      ),
    /mot de passe/,
  );
  assert.throws(
    () =>
      legacyWordText(
        legacyFile('Cours', (_word, table) => table.writeUInt32LE(999999, 15)),
      ),
    /incomplet/,
  );
});
