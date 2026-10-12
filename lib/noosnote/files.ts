import type {
  NoteFiles,
  NotePage,
  NoteChange,
  RichNode,
  NoteAttachment,
} from './types';
import {
  applyFiles,
  change,
  createDatabase,
  createPage,
  createRow,
  deduplicateChanges,
  getWorkspace,
  id,
} from './model';

export const MAX_ATTACHMENT_BYTES = 20 * 1024 * 1024;
export function imageMediaType(bytes: Uint8Array): string | undefined {
  const starts = (...signature: number[]) =>
    signature.every((byte, index) => bytes[index] === byte);
  if (starts(137, 80, 78, 71, 13, 10, 26, 10)) return 'image/png';
  if (starts(255, 216, 255)) return 'image/jpeg';
  const header = String.fromCharCode(...bytes.subarray(0, 12));
  if (header.startsWith('GIF87a') || header.startsWith('GIF89a'))
    return 'image/gif';
  if (header.startsWith('RIFF') && header.slice(8) === 'WEBP')
    return 'image/webp';
  if (starts(66, 77)) return 'image/bmp';
  if (
    header.slice(4, 8) === 'ftyp' &&
    ['avif', 'avis'].includes(header.slice(8))
  )
    return 'image/avif';
}
export const isImage = (file: { name: string; mediaType: string }) =>
  /^(image\/(png|jpeg|gif|webp|bmp|avif))$/.test(file.mediaType) ||
  (file.mediaType === 'application/octet-stream' &&
    /\.(png|jpe?g|gif|webp|bmp|avif)$/i.test(file.name));
export function transferFiles(transfer: DataTransfer): File[] {
  const files = [...transfer.files];
  if (files.length) return files;
  return [...transfer.items].flatMap((item) => {
    const file = item.kind === 'file' ? item.getAsFile() : null;
    return file ? [file] : [];
  });
}
export const isWord = (name: string) => /\.docx?$/i.test(name);
export const fileSize = (bytes: number) =>
  bytes < 1024 * 1024
    ? `${Math.max(1, Math.round(bytes / 1024))} Ko`
    : `${(bytes / (1024 * 1024)).toLocaleString('fr-FR', { maximumFractionDigits: 1 })} Mo`;

export function base64Bytes(content: string): Uint8Array {
  return Uint8Array.from(atob(content), (character) => character.charCodeAt(0));
}
export function bytesBase64(bytes: Uint8Array): string {
  const parts: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += 32768)
    parts.push(String.fromCharCode(...bytes.subarray(offset, offset + 32768)));
  return btoa(parts.join(''));
}
export function saveBlob(name: string, blob: Blob) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

export function convertWord(
  file: File,
): Promise<{ html?: string; text?: string; notice?: string }> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('./word-worker.ts', import.meta.url), {
      type: 'module',
    });
    const timer = setTimeout(() => {
      worker.terminate();
      reject(
        new Error(
          'La conversion a pris trop de temps. Essaie une copie plus légère du document.',
        ),
      );
    }, 30_000);
    const finish = () => {
      clearTimeout(timer);
      worker.terminate();
    };
    worker.onmessage = (
      event: MessageEvent<{
        html?: string;
        text?: string;
        notice?: string;
        error?: string;
      }>,
    ) => {
      finish();
      if (event.data.error) reject(new Error(event.data.error));
      else resolve(event.data);
    };
    worker.onerror = () => {
      finish();
      reject(new Error('Impossible de convertir ce document Word.'));
    };
    file
      .arrayBuffer()
      .then((bytes) =>
        worker.postMessage({ bytes, legacy: /\.doc$/i.test(file.name) }, [
          bytes,
        ]),
      )
      .catch((error: unknown) => {
        finish();
        reject(error);
      });
  });
}

function boundedText(value: string, maximum: number, label: string) {
  if (new TextEncoder().encode(value).length > maximum)
    throw new Error(
      `${label} est trop long pour une table Noosnote. L’original reste intact.`,
    );
  return value;
}

function tableGrid(table: HTMLTableElement) {
  const rows = [...table.rows];
  if (rows.length > 1000)
    throw new Error('Un tableau importé peut contenir jusqu’à 1 000 lignes.');
  let spanning: number[] = [];
  const values = rows.map((row) => {
    const result: string[] = [];
    const next = spanning.map((remaining) => Math.max(0, remaining - 1));
    let column = 0;
    for (const cell of row.cells) {
      while (spanning[column]) {
        result[column] = '';
        column += 1;
      }
      const span = Math.max(1, cell.colSpan);
      if (column + span > 64)
        throw new Error(
          'Un tableau importé peut contenir jusqu’à 64 colonnes.',
        );
      const text = [...cell.childNodes]
        .map((node) => node.textContent?.trim() ?? '')
        .filter(Boolean)
        .join('\n');
      for (let offset = 0; offset < span; offset += 1) {
        result[column + offset] = offset ? '' : text;
        next[column + offset] = Math.max(0, cell.rowSpan - 1);
      }
      column += span;
    }
    spanning = next;
    return result;
  });
  const columns = Math.max(0, ...values.map((row) => row.length));
  const header =
    !!rows[0]?.cells.length &&
    [...rows[0].cells].every(
      (cell) => cell.tagName === 'TH' && cell.colSpan === 1,
    );
  return { values, columns, header };
}

function richText(
  node: globalThis.Node,
  marks: NonNullable<RichNode['marks']> = [],
): RichNode[] {
  if (node.nodeType === 3)
    return node.textContent
      ? [
          {
            type: 'text',
            text: node.textContent,
            ...(marks.length ? { marks } : {}),
          },
        ]
      : [];
  if (!(node instanceof Element)) return [];
  const tag = node.tagName.toLowerCase();
  if (['script', 'style', 'iframe', 'object'].includes(tag)) return [];
  if (tag === 'br') return [{ type: 'hardBreak' }];
  if (tag === 'img')
    return [{ type: 'text', text: '[Image dans le fichier original]' }];
  const types: Record<string, string> = {
    b: 'bold',
    strong: 'bold',
    i: 'italic',
    em: 'italic',
    u: 'underline',
    s: 'strike',
    del: 'strike',
    code: 'code',
  };
  const next = [...marks];
  if (types[tag] && !next.some((mark) => mark.type === types[tag]))
    next.push({ type: types[tag] });
  if (tag === 'a') {
    const href = node.getAttribute('href') ?? '';
    if (/^(https?:\/\/|mailto:|tel:)/i.test(href) && href.length <= 2048)
      next.push({ type: 'link', attrs: { href } });
  }
  return [...node.childNodes].flatMap((child) => richText(child, next));
}

export function documentChanges(
  files: NoteFiles,
  workspaceId: string,
  parentId: string,
  file: NoteAttachment,
  converted?: { html?: string; text?: string; notice?: string },
) {
  const created = createPage(
    files,
    workspaceId,
    parentId,
    file.name.replace(/\.(pdf|docx?)$/i, ''),
  );
  const changes: NoteChange[] = [...created.changes];
  let current = applyFiles(files, changes);
  const blocks: RichNode[] = [];
  let nodes = 0;
  const block = (element: Element): RichNode[] => {
    nodes += 1;
    if (nodes > 20_000)
      throw new Error('Ce document contient trop d’éléments.');
    const tag = element.tagName.toLowerCase();
    if (tag === 'table') {
      const grid = tableGrid(element as HTMLTableElement);
      if (!grid.columns) return [];
      const result = createDatabase(
        current,
        workspaceId,
        created.page.id,
        'Tableau',
      );
      result.database.columns = Array.from(
        { length: grid.columns },
        (_, index) => ({
          id: id(),
          type: index === 0 ? 'title' : 'text',
          name: boundedText(
            grid.header
              ? grid.values[0][index] || `Colonne ${index + 1}`
              : `Colonne ${index + 1}`,
            128,
            'Un titre de colonne',
          ),
        }),
      );
      changes.push(...result.changes);
      current = applyFiles(current, result.changes);
      let database = result.database;
      for (const values of grid.values.slice(grid.header ? 1 : 0)) {
        const result = createRow(
          current,
          database,
          boundedText(values[0] || 'Sans titre', 512, 'Un titre de ligne'),
          Object.fromEntries(
            database.columns
              .slice(1)
              .map((column, index) => [
                column.id,
                boundedText(
                  values[index + 1] || '',
                  16_384,
                  'Le contenu d’une cellule',
                ),
              ]),
          ),
        );
        changes.push(...result.changes);
        current = applyFiles(current, result.changes);
        database = { ...database, rows: [...database.rows, result.row] };
      }
      return [
        { type: 'noosDatabase', attrs: { id: id(), databaseId: database.id } },
        { type: 'paragraph', attrs: { id: id() } },
      ];
    }
    if (tag === 'ul' || tag === 'ol')
      return [
        {
          type: tag === 'ul' ? 'bulletList' : 'orderedList',
          attrs: {
            id: id(),
            ...(tag === 'ol'
              ? { start: Number(element.getAttribute('start')) || 1 }
              : {}),
          },
          content: [...element.children]
            .filter((child) => child.tagName === 'LI')
            .map((child): RichNode => {
              const content: RichNode[] = [];
              let inline: globalThis.Node[] = [];
              const flush = () => {
                if (!inline.length) return;
                const text = inline.flatMap((node) => richText(node));
                if (text.length)
                  content.push({
                    type: 'paragraph',
                    attrs: { id: id() },
                    content: text,
                  });
                inline = [];
              };
              for (const node of child.childNodes) {
                if (
                  node instanceof Element &&
                  ['P', 'UL', 'OL', 'TABLE'].includes(node.tagName)
                ) {
                  flush();
                  content.push(...block(node));
                } else inline.push(node);
              }
              flush();
              if (!content.length || content[0].type !== 'paragraph')
                content.unshift({ type: 'paragraph', attrs: { id: id() } });
              return { type: 'listItem', attrs: { id: id() }, content };
            }),
        },
      ];
    if (tag === 'blockquote')
      return [
        {
          type: 'blockquote',
          attrs: { id: id() },
          content: [...element.children].flatMap(block),
        },
      ];
    if (/^h[1-6]$/.test(tag))
      return [
        {
          type: 'heading',
          attrs: { id: id(), level: Math.min(3, Number(tag[1])) },
          content: richText(element),
        },
      ];
    if (tag === 'hr') return [{ type: 'horizontalRule', attrs: { id: id() } }];
    if (['script', 'style'].includes(tag)) return [];
    return [
      { type: 'paragraph', attrs: { id: id() }, content: richText(element) },
    ];
  };
  if (converted?.html) {
    const parsed = new DOMParser().parseFromString(converted.html, 'text/html');
    blocks.push(...[...parsed.body.children].flatMap(block));
  } else if (converted?.text) {
    blocks.push(
      ...converted.text.split('\n').map((text) => ({
        type: 'paragraph',
        attrs: { id: id() },
        ...(text ? { content: [{ type: 'text', text }] } : {}),
      })),
    );
  }
  const page: NotePage = {
    ...created.page,
    icon: isWord(file.name) ? '📝' : isImage(file) ? '🖼️' : '📎',
    attachment: file,
    ...(converted?.notice ? { importNotice: converted.notice } : {}),
    content: {
      type: 'doc',
      content: blocks.length
        ? blocks
        : [{ type: 'paragraph', attrs: { id: id() } }],
    },
  };
  if (new TextEncoder().encode(JSON.stringify(page)).length > 1024 * 1024 - 1)
    throw new Error(
      'Le texte de ce document est trop volumineux pour une seule page.',
    );
  const workspace = getWorkspace(current, workspaceId)!;
  changes.push(
    change(page),
    change({
      ...workspace,
      pages: workspace.pages.map((entry) =>
        entry.id === page.id
          ? {
              ...entry,
              icon: page.icon,
              file: {
                name: file.name,
                mediaType: file.mediaType,
                size: file.size,
              },
            }
          : entry,
      ),
    }),
  );
  const finalChanges = deduplicateChanges(changes);
  if (
    finalChanges.some(
      (entry) =>
        new TextEncoder().encode(JSON.stringify(entry.value)).length >
        1024 * 1024 - 1,
    )
  )
    throw new Error(
      'Une table de ce document est trop volumineuse. Sépare-la en plusieurs pages avant l’import.',
    );
  return { page, changes: finalChanges };
}
