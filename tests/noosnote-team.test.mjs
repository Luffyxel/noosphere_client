import assert from 'node:assert/strict';
import test from 'node:test';
import * as Y from 'yjs';
import { getSchema } from '@tiptap/core';
import StarterKit from '@tiptap/starter-kit';
import { yXmlFragmentToProsemirrorJSON } from '@tiptap/y-tiptap';
import {
  collaborativeDocument,
  documentUpdate,
  applyPageUpdates,
} from '../lib/noosnote/collaboration.ts';

const schema = getSchema([StarterKit]);
const page = {
  version: 1,
  kind: 'page',
  id: 'd02523d7-7538-4848-ae47-4baef0c97f53',
  workspaceId: '0f4fbbe5-3051-4944-a537-0342c58250ad',
  title: 'Cours',
  icon: '',
  content: {
    type: 'doc',
    content: [
      { type: 'paragraph', content: [{ type: 'text', text: 'Le cours.' }] },
    ],
  },
};
function text(doc) {
  return doc.getXmlFragment('default').get(0).get(0);
}
function json(doc) {
  return yXmlFragmentToProsemirrorJSON(doc.getXmlFragment('default'));
}
void test('three first editors share one seed and retain concurrent typing', () => {
  const documents = [0, 1, 2].map(() => collaborativeDocument(schema, page));
  text(documents[0]).insert(0, 'Alice : ');
  text(documents[1]).insert(3, 'Bob ');
  text(documents[2]).insert(text(documents[2]).length, ' Clara.');
  const snapshots = documents.map(documentUpdate);
  for (const [index, doc] of documents.entries())
    applyPageUpdates(doc, index % 2 ? [...snapshots].reverse() : snapshots);
  assert.deepEqual(json(documents[0]), json(documents[1]));
  assert.deepEqual(json(documents[0]), json(documents[2]));
  assert.equal(documents[0].getXmlFragment('default').length, 1);
  const final = text(documents[0]).toString();
  for (const word of ['Alice', 'Bob', 'Clara', 'cours'])
    assert.ok(final.includes(word), final);
  const reopened = collaborativeDocument(schema, {
    ...page,
    collaborationUpdates: [documentUpdate(documents[0])],
  });
  assert.deepEqual(json(reopened), json(documents[0]));
  for (const doc of [...documents, reopened]) doc.destroy();
});
void test('offline edits merge with deletion and repeated snapshots are idempotent', () => {
  const first = collaborativeDocument(schema, page);
  const second = collaborativeDocument(schema, {
    ...page,
    collaborationUpdates: [documentUpdate(first)],
  });
  text(first).delete(0, 3);
  text(second).insert(text(second).length, ' Suite hors ligne.');
  const a = documentUpdate(first),
    b = documentUpdate(second);
  applyPageUpdates(first, [a, b, b, a]);
  applyPageUpdates(second, [b, a, a, b]);
  assert.deepEqual(json(first), json(second));
  assert.equal(text(first).toString(), 'cours. Suite hors ligne.');
  first.destroy();
  second.destroy();
});
void test('collaboration rejects oversized or malformed persisted states', () => {
  const document = new Y.Doc();
  assert.throws(() => applyPageUpdates(document, ['x'.repeat(700001)]));
  assert.throws(() => applyPageUpdates(document, ['invalid@']));
  document.destroy();
});
