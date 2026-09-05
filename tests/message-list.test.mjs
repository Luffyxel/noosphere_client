import assert from 'node:assert/strict';
import test from 'node:test';

import { mergeMessages } from '../lib/message-list.ts';

function message(id, sentAt, own = false) {
  return {
    version: 1,
    id,
    conversationId: 'dm-0123456789abcdef0123456789abcdef',
    sentAt,
    text: id,
    senderId: own ? 1 : 2,
    own,
  };
}

void test('une synchronisation conserve la clé de rendu des messages connus', () => {
  const known = {
    ...message('msg-11111111111111111111111111111111', '2026-09-04T12:00:00Z'),
    renderKey: 'stable-key',
  };
  const synchronized = message(
    'msg-11111111111111111111111111111111',
    '2026-09-04T12:00:00Z',
  );
  synchronized.text = 'contenu synchronisé';

  assert.deepEqual(mergeMessages([known], [synchronized]), [
    { ...synchronized, renderKey: 'stable-key' },
  ]);
});

void test('un rafraîchissement conserve un envoi en cours', () => {
  const received = message(
    'msg-22222222222222222222222222222222',
    '2026-09-04T12:00:00Z',
  );
  const pending = {
    ...message('pending-local', '2026-09-04T12:00:01Z', true),
    renderKey: 'pending-local',
    pending: true,
  };

  assert.deepEqual(mergeMessages([pending], [received]), [
    { ...received, renderKey: received.id },
    pending,
  ]);
});
