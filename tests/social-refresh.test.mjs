import assert from 'node:assert/strict';
import test from 'node:test';

import {
  SOCIAL_REFRESH_INTERVAL_MS,
  SOCIAL_REFRESH_RETRY_MS,
  startSocialRefreshLoop,
} from '../lib/social-refresh.ts';

function fakeScheduler() {
  const pending = [];
  return {
    pending,
    scheduler: {
      schedule(callback, delay) {
        const task = { callback, delay };
        pending.push(task);
        return task;
      },
      cancel(handle) {
        const index = pending.indexOf(handle);
        if (index >= 0) pending.splice(index, 1);
      },
    },
  };
}

async function settle() {
  await new Promise((resolve) => setImmediate(resolve));
}

void test('deux clients vérifient indépendamment leurs amis au démarrage', async () => {
  const first = fakeScheduler();
  const second = fakeScheduler();
  const firstAnnouncements = [];
  const secondAnnouncements = [];

  const stopFirst = startSocialRefreshLoop(async (announceNew) => {
    firstAnnouncements.push(announceNew);
    return firstAnnouncements.length > 1;
  }, first.scheduler);
  const stopSecond = startSocialRefreshLoop(async (announceNew) => {
    secondAnnouncements.push(announceNew);
    return true;
  }, second.scheduler);

  await settle();
  assert.deepEqual(firstAnnouncements, [false]);
  assert.deepEqual(secondAnnouncements, [false]);
  assert.equal(first.pending[0].delay, SOCIAL_REFRESH_RETRY_MS);
  assert.equal(second.pending[0].delay, SOCIAL_REFRESH_INTERVAL_MS);

  first.pending.shift().callback();
  await settle();
  assert.deepEqual(firstAnnouncements, [false, false]);
  assert.equal(first.pending[0].delay, SOCIAL_REFRESH_INTERVAL_MS);

  second.pending.shift().callback();
  await settle();
  assert.deepEqual(secondAnnouncements, [false, true]);

  stopFirst();
  stopSecond();
  assert.equal(first.pending.length, 0);
  assert.equal(second.pending.length, 0);
});
