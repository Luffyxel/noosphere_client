import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const root = new URL('../', import.meta.url);

void test('les chargements compacts utilisent tous l’animation du logo', async () => {
  const [brand, page] = await Promise.all([
    readFile(new URL('components/brand.tsx', root), 'utf8'),
    readFile(new URL('app/page.tsx', root), 'utf8'),
  ]);

  assert.match(brand, /src="\/brand\/loading\.mp4"/);
  assert.match(brand, /motion-reduce:hidden/);
  assert.match(brand, /motion-reduce:block/);
  assert.match(page, /<BrandLoader className="size-4"/);
  assert.doesNotMatch(page, /LoaderCircle/);
});
