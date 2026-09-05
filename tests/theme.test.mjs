import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const root = new URL('../', import.meta.url);

void test('la palette principale utilise le vert sauge sans ancien bleu', async () => {
  const sources = await Promise.all(
    ['app/globals.css', 'app/page.tsx', 'public/favicon.svg'].map((path) =>
      readFile(new URL(path, root), 'utf8'),
    ),
  );
  const combined = sources.join('\n').toLowerCase();
  assert.match(combined, /--noosphere-accent:\s*#6fb98f/);
  assert.match(combined, /--noosphere-accent-strong:\s*#3f795c/);
  for (const previousBlue of ['#0a84ff', '#2894ff', '#68c4ff', '#0c79d8']) {
    assert.equal(combined.includes(previousBlue), false);
  }
});
