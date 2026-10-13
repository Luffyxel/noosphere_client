import assert from 'node:assert/strict';
import test from 'node:test';
import { noteCloseHandler } from '../lib/noosnote/window-close.ts';

function request(handler, forbiddenDestroy) {
  let prevented = false;
  return handler({
    preventDefault() {
      prevented = true;
    },
  }).then(() => {
    if (!prevented) forbiddenDestroy();
  });
}

void test('a clean window closes without the restricted default destroy command', async () => {
  let closed = 0;
  const handler = noteCloseHandler(
    async () => {},
    async () => {
      closed += 1;
    },
    (error) => {
      throw error;
    },
  );
  await request(handler, () => {
    throw new Error('window.destroy permission denied');
  });
  assert.equal(closed, 1);
});

void test('close waits for an in-flight save and merges repeated close requests', async () => {
  let persisted = false,
    closed = 0,
    release;
  const saving = new Promise((resolve) => {
    release = () => {
      persisted = true;
      resolve();
    };
  });
  const handler = noteCloseHandler(
    () => saving,
    async () => {
      assert.equal(persisted, true);
      closed += 1;
    },
    (error) => {
      throw error;
    },
  );
  const denied = () => {
    throw new Error('restricted window command');
  };
  const first = request(handler, denied);
  await request(handler, denied);
  assert.equal(closed, 0);
  release();
  await first;
  assert.equal(closed, 1);
});

void test('a failed save keeps the window open and permits retry', async () => {
  let fail = true,
    closed = false;
  const errors = [];
  const failure = new Error('disk full');
  const handler = noteCloseHandler(
    async () => {
      if (fail) throw failure;
    },
    async () => {
      closed = true;
    },
    (error) => errors.push(error),
  );
  await request(handler, () => {
    throw new Error('unsaved window destroyed');
  });
  assert.equal(closed, false);
  assert.deepEqual(errors, [failure]);
  fail = false;
  await request(handler, () => {
    throw new Error('restricted window command');
  });
  assert.equal(closed, true);
});

void test('native close errors are reported and can be retried', async () => {
  const errors = [];
  const failure = new Error('native window unavailable');
  let fail = true,
    closed = false;
  const handler = noteCloseHandler(
    async () => {},
    async () => {
      if (fail) throw failure;
      closed = true;
    },
    (error) => errors.push(error),
  );
  await request(handler, () => {
    throw new Error('restricted window command');
  });
  assert.equal(closed, false);
  assert.deepEqual(errors, [failure]);
  fail = false;
  await request(handler, () => {
    throw new Error('restricted window command');
  });
  assert.equal(closed, true);
});
