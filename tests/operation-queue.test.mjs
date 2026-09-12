import assert from 'node:assert/strict';
import test from 'node:test';

import { createOperationQueue } from '../lib/operation-queue.ts';

test('attend la fin de l’actualisation avant de lancer une action', async () => {
  const queue = createOperationQueue();
  const events = [];
  let finishRefresh;
  const refreshFinished = new Promise((resolve) => {
    finishRefresh = resolve;
  });

  const refresh = queue.enqueue(async () => {
    events.push('refresh-start');
    await refreshFinished;
    events.push('refresh-end');
  });
  const action = queue.enqueue(async () => {
    events.push('action');
  });

  await Promise.resolve();
  assert.deepEqual(events, ['refresh-start']);
  finishRefresh();
  await Promise.all([refresh, action]);
  assert.deepEqual(events, ['refresh-start', 'refresh-end', 'action']);
});

test('une erreur ne bloque pas les opérations suivantes', async () => {
  const queue = createOperationQueue();
  const failed = queue.enqueue(async () => {
    throw new Error('sync failed');
  });
  const next = queue.enqueue(async () => 'done');

  await assert.rejects(failed, /sync failed/);
  assert.equal(await next, 'done');
});
