import assert from 'node:assert/strict';
import test from 'node:test';

import {
  SOCIAL_REFRESH_INTERVAL_MS,
  SOCIAL_REFRESH_MAX_RETRY_MS,
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

void test('les échecs répétés espacés ne déclenchent pas une boucle rapide', async () => {
  const { pending, scheduler } = fakeScheduler();
  let succeed = false;
  const stop = startSocialRefreshLoop(async () => succeed, scheduler);
  await settle();
  for (const delay of [
    3_000,
    6_000,
    12_000,
    24_000,
    48_000,
    96_000,
    192_000,
    SOCIAL_REFRESH_MAX_RETRY_MS,
    SOCIAL_REFRESH_MAX_RETRY_MS,
  ]) {
    const next = pending.shift();
    assert.equal(next.delay, delay);
    next.callback();
    await settle();
  }
  succeed = true;
  pending.shift().callback();
  await settle();
  assert.equal(pending[0].delay, SOCIAL_REFRESH_INTERVAL_MS);
  succeed = false;
  pending.shift().callback();
  await settle();
  assert.equal(pending[0].delay, SOCIAL_REFRESH_RETRY_MS);
  stop();
});
