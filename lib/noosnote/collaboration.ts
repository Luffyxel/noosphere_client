import * as Y from 'yjs';
import {
  prosemirrorJSONToYXmlFragment,
  yXmlFragmentToProsemirrorJSON,
} from '@tiptap/y-tiptap';
import type { Schema } from '@tiptap/pm/model';
import type { NotePage } from './types';

export function decodeUpdate(value: string) {
  if (value.length > 700_000)
    throw new Error('Page collaborative trop volumineuse.');
  return Uint8Array.from(atob(value), (character) => character.charCodeAt(0));
}
export function documentUpdate(document: Y.Doc) {
  const update = Y.encodeStateAsUpdate(document);
  if (update.length > 500_000)
    throw new Error(
      'Cette page atteint la taille maximale. Crée une sous-page pour continuer.',
    );
  let binary = '';
  for (let start = 0; start < update.length; start += 8192)
    binary += String.fromCharCode(...update.subarray(start, start + 8192));
  return btoa(binary);
}
export function applyPageUpdates(document: Y.Doc, updates: string[] = []) {
  for (const update of updates)
    Y.applyUpdate(document, decodeUpdate(update), 'repository');
}
export function collaborativeDocument(schema: Schema, page: NotePage) {
  const document = new Y.Doc();
  if (page.collaborationUpdates?.length) {
    applyPageUpdates(document, page.collaborationUpdates);
  } else {
    const seed = new Y.Doc();
    let client = 2166136261;
    for (const character of page.id)
      client = Math.imul(client ^ character.charCodeAt(0), 16777619);
    seed.clientID = client >>> 0;
    prosemirrorJSONToYXmlFragment(
      schema,
      page.content,
      seed.getXmlFragment('default'),
    );
    Y.applyUpdate(document, Y.encodeStateAsUpdate(seed));
    seed.destroy();
  }
  return document;
}

export function materializePage(page: NotePage): NotePage {
  if (!page.collaborationUpdates?.length) return page;
  const document = new Y.Doc();
  try {
    applyPageUpdates(document, page.collaborationUpdates);
    return {
      ...page,
      content: yXmlFragmentToProsemirrorJSON(
        document.getXmlFragment('default'),
      ) as NotePage['content'],
    };
  } finally {
    document.destroy();
  }
}
