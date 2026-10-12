import mammoth from 'mammoth';
import { legacyWordText } from './legacy-word';

function checkArchive(bytes: ArrayBuffer) {
  const data = new DataView(bytes);
  let end = bytes.byteLength - 22;
  const minimum = Math.max(0, end - 65535);
  while (end >= minimum && data.getUint32(end, true) !== 0x06054b50) end -= 1;
  if (end < minimum)
    throw new Error('Ce document Word est chiffré ou incomplet.');
  const count = data.getUint16(end + 10, true);
  let offset = data.getUint32(end + 16, true);
  let total = 0;
  if (count > 8192)
    throw new Error('Ce document Word contient trop de fichiers.');
  for (let index = 0; index < count; index += 1) {
    if (
      offset + 46 > bytes.byteLength ||
      data.getUint32(offset, true) !== 0x02014b50
    )
      throw new Error('Ce document Word est incomplet.');
    total += data.getUint32(offset + 24, true);
    if (total > 64 * 1024 * 1024)
      throw new Error(
        'Le contenu décompressé de ce document Word est trop volumineux.',
      );
    offset +=
      46 +
      data.getUint16(offset + 28, true) +
      data.getUint16(offset + 30, true) +
      data.getUint16(offset + 32, true);
  }
}

self.onmessage = async (
  event: MessageEvent<{ bytes: ArrayBuffer; legacy: boolean }>,
) => {
  try {
    if (event.data.legacy) {
      self.postMessage({
        text: legacyWordText(new Uint8Array(event.data.bytes)),
        notice:
          'Ancien document Word converti en texte modifiable. L’original est conservé.',
      });
      return;
    }
    let images = 0;
    checkArchive(event.data.bytes);
    const result = await mammoth.convertToHtml(
      { arrayBuffer: event.data.bytes },
      {
        includeEmbeddedStyleMap: false,
        externalFileAccess: false,
        styleMap: ['u => u', 'strike => s'],
        convertImage: mammoth.images.imgElement(async () => {
          images += 1;
          return { src: '' };
        }),
      },
    );
    self.postMessage({
      html: result.value,
      notice: images
        ? `${images} image${images > 1 ? 's' : ''} reste${images > 1 ? 'nt' : ''} disponible${images > 1 ? 's' : ''} dans le fichier original.`
        : undefined,
    });
  } catch (error) {
    self.postMessage({
      error:
        error instanceof Error
          ? error.message
          : 'Ce document Word est illisible.',
    });
  }
};
