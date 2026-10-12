import { find, read } from 'cfb';

export function legacyWordText(bytes: Uint8Array): string {
  const container = read(bytes, { type: 'array' });
  const main = find(container, 'WordDocument')?.content;
  if (!main || main.length < 0x1aa)
    throw new Error('Ce document Word est illisible.');
  const word = Uint8Array.from(main);
  const fib = new DataView(word.buffer);
  if (fib.getUint16(0, true) !== 0xa5ec || fib.getUint16(0x20, true) !== 14) {
    throw new Error(
      'Ce format Word est trop ancien. Enregistre-le en .docx pour l’importer.',
    );
  }
  const flags = fib.getUint16(0x0a, true);
  if (flags & 0x0100)
    throw new Error('Ce document Word est protégé par un mot de passe.');
  const tableEntry = find(
    container,
    flags & 0x0200 ? '1Table' : '0Table',
  )?.content;
  if (!tableEntry)
    throw new Error('Le contenu de ce document Word est introuvable.');
  const table = Uint8Array.from(tableEntry);
  const offset = fib.getUint32(0x1a2, true);
  const length = fib.getUint32(0x1a6, true);
  const bodyLength = fib.getUint32(0x4c, true);
  if (offset + length > table.length || bodyLength > 1_000_000)
    throw new Error('Ce document Word est trop volumineux ou incomplet.');
  const clx = new DataView(table.buffer, offset, length);
  let position = 0;
  while (position + 3 <= length && clx.getUint8(position) === 1)
    position += 3 + clx.getUint16(position + 1, true);
  if (position + 5 > length || clx.getUint8(position) !== 2)
    throw new Error('Ce document Word est illisible.');
  const pieceLength = clx.getUint32(position + 1, true);
  position += 5;
  const count = (pieceLength - 4) / 12;
  if (
    !Number.isInteger(count) ||
    count < 0 ||
    count > 100_000 ||
    position + pieceLength > length
  )
    throw new Error('Ce document Word est incomplet.');
  const pieces = new DataView(table.buffer, offset + position, pieceLength);
  const output: string[] = [];
  for (let i = 0; i < count; i += 1) {
    const start = pieces.getUint32(i * 4, true);
    const end = Math.min(pieces.getUint32((i + 1) * 4, true), bodyLength);
    if (start >= bodyLength) break;
    if (end < start) throw new Error('Ce document Word est illisible.');
    const encoded = pieces.getUint32((count + 1) * 4 + i * 8 + 2, true);
    const compressed = !!(encoded & 0x40000000);
    const source = (encoded & 0x3fffffff) / (compressed ? 2 : 1);
    const size = (end - start) * (compressed ? 1 : 2);
    if (!Number.isInteger(source) || source + size > word.length)
      throw new Error('Le texte de ce document Word est incomplet.');
    output.push(
      new TextDecoder(compressed ? 'windows-1252' : 'utf-16le').decode(
        word.subarray(source, source + size),
      ),
    );
  }
  const fields: boolean[] = [];
  const cleaned: string[] = [];
  for (const character of output.join('')) {
    const code = character.charCodeAt(0);
    if (code === 19) {
      fields.push(false);
      continue;
    }
    if (code === 20) {
      if (fields.length) fields[fields.length - 1] = true;
      continue;
    }
    if (code === 21) {
      fields.pop();
      continue;
    }
    if (fields.some((visible) => !visible)) continue;
    if ([10, 11, 12, 13].includes(code)) cleaned.push('\n');
    else if (code === 7 || code === 9) cleaned.push('\t');
    else if (code >= 32) cleaned.push(character);
  }
  return cleaned.join('').trim();
}
