import { readFile, writeFile, mkdir, copyFile } from 'node:fs/promises';

const packageRoot = new URL('../node_modules/emojibase-data/', import.meta.url);
const data = JSON.parse(
  await readFile(new URL('fr/compact.json', packageRoot), 'utf8'),
);
const emojis = data
  .filter((emoji) => emoji.group !== undefined && emoji.group !== 2)
  .sort((a, b) => a.order - b.order)
  .map((emoji) => ({
    unicode: emoji.unicode,
    label: emoji.label,
    group: emoji.group,
    tags: emoji.tags ?? [],
    skins: (emoji.skins ?? []).map((skin) => ({
      unicode: skin.unicode,
      label: skin.label,
    })),
  }));
await writeFile(
  new URL('../lib/noosnote/emoji-data.json', import.meta.url),
  `${JSON.stringify(emojis)}\n`,
);
const licenses = new URL('../docs/licenses/', import.meta.url);
await mkdir(licenses, { recursive: true });
await copyFile(
  new URL('LICENSE', packageRoot),
  new URL('emojibase-data.txt', licenses),
);
console.log(`${emojis.length} emojis français générés.`);
