import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto';
import { once } from 'node:events';
import { access, mkdtemp, rmdir, unlink } from 'node:fs/promises';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

const project = path.resolve(import.meta.dirname, '..');
const executable = path.join(
  project,
  'src-tauri',
  'target',
  process.argv.includes('--debug-host') ? 'debug' : 'release',
  `${process.argv.includes('--debug-host') ? 'noosphere-remote-host' : 'noosphere-desktop'}${process.platform === 'win32' ? '.exe' : ''}`,
);
await access(executable);
const directory = await mkdtemp(path.join(os.tmpdir(), 'noosphere-remote-'));
const hosts = [];

function proof(key, challenge, server) {
  return createHmac('sha256', key)
    .update('noosphere/remote/ipc/v1\0')
    .update(Buffer.from([Number(server)]))
    .update(challenge)
    .digest();
}

async function read(socket, length) {
  while (true) {
    const bytes = socket.read(length);
    if (bytes) return bytes;
    if (socket.destroyed || socket.readableEnded)
      throw new Error('Host closed the pipe');
    await once(socket, 'readable');
  }
}

async function request(host, command) {
  const socket = net.createConnection(host.endpoint);
  socket.setTimeout(3000, () => socket.destroy(new Error('Host IPC timeout')));
  try {
    await once(socket, 'connect');
    const challenge = await read(socket, 32);
    socket.write(proof(host.key, challenge, false));
    const clientChallenge = randomBytes(32);
    socket.write(clientChallenge);
    assert.ok(
      timingSafeEqual(
        await read(socket, 32),
        proof(host.key, clientChallenge, true),
      ),
    );
    const payload = Buffer.from(
      JSON.stringify(typeof command === 'string' ? { command } : command),
    );
    const length = Buffer.alloc(4);
    length.writeUInt32BE(payload.length);
    socket.write(Buffer.concat([length, payload]));
    const responseLength = (await read(socket, 4)).readUInt32BE();
    assert.ok(responseLength > 0 && responseLength <= 16 * 1024);
    return JSON.parse((await read(socket, responseLength)).toString());
  } finally {
    socket.destroy();
  }
}

function start() {
  const id = randomBytes(16).toString('hex');
  const endpoint =
    process.platform === 'win32'
      ? `\\\\.\\pipe\\noosphere-remote-${id}`
      : path.join(directory, `remote-${id}.sock`);
  const key = randomBytes(32);
  const child = spawn(executable, ['--remote-host'], {
    stdio: ['pipe', 'ignore', 'ignore'],
    windowsHide: true,
  });
  const host = { child, endpoint, key };
  hosts.push(host);
  const bootstrap = Buffer.from(
    JSON.stringify({
      endpoint,
      key: [...key],
      settings: {
        machineName: 'Remote smoke',
        display: 0,
        video: {
          width: 1920,
          height: 1080,
          fps: 60,
          bitrate: 20_000_000,
          codec: 'h264',
        },
        audio: false,
      },
    }),
  );
  const length = Buffer.alloc(4);
  length.writeUInt32BE(bootstrap.length);
  child.stdin.end(Buffer.concat([length, bootstrap]));
  return host;
}

async function ready(host) {
  const until = Date.now() + 10_000;
  let error;
  while (Date.now() < until && host.child.exitCode === null) {
    try {
      return await request(host, 'status');
    } catch (caught) {
      error = caught;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw error || new Error('Native host exited before startup');
}

try {
  const first = start();
  const second = start();
  const [a, b] = await Promise.all([ready(first), ready(second)]);
  assert.notEqual(a.pid, b.pid);
  assert.equal(a.pid, first.child.pid);
  assert.equal(b.pid, second.child.pid);
  for (const status of [a, b]) {
    assert.equal(status.protocolVersion, 1);
    assert.equal(status.hostEnabled, false);
    assert.equal(typeof status.streamingReady, 'boolean');
    assert.equal(status.settings.machineName, 'Remote smoke');
  }
  if (process.argv.includes('--gpu')) {
    assert.equal(
      process.platform,
      'win32',
      'The native GPU smoke currently requires Windows',
    );
    assert.equal(a.streamingReady, true);
    const privateKey = randomBytes(32);
    const command = {
      command: 'startLocalTest',
      settings: a.settings,
      credentials: {
        githubUserId: 1,
        privateKey: [...privateKey],
        machineId: [...randomBytes(16)],
      },
    };
    try {
      assert.deepEqual(await request(first, command), { Ok: null });
    } finally {
      privateKey.fill(0);
      command.credentials.privateKey.fill(0);
    }
    let result;
    const deadline = Date.now() + 35_000;
    while (Date.now() < deadline) {
      result = (await request(first, 'status')).diagnostic;
      if (result.state !== 'running') break;
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    assert.equal(result?.state, 'complete', JSON.stringify(result));
    const report = result.report;
    console.log(
      JSON.stringify(
        {
          kind: process.argv.includes('--debug-host')
            ? 'native-debug-host-gpu-session'
            : 'packaged-native-gpu-session',
          ...report,
        },
        null,
        2,
      ),
    );
    assert.ok(
      report.presentedFrames >= 60,
      'Expected at least sixty real GPU frames',
    );
    assert.ok(
      report.deliveredFrames / report.encodedFrames >= 0.9,
      'Excessive local datagram loss',
    );
    assert.equal(report.inputEventsReceived, 5);
    assert.equal(
      report.inputKeyObserved,
      true,
      'Keep the native viewer in the foreground for the F24 injection check',
    );
    assert.equal(report.revocationVerified, true);
    assert.equal(
      report.inputMouseObserved,
      true,
      'Expected a scoped mouse click in the native viewer',
    );
    assert.equal(report.inputToPhotonUs, null);
  }
  for (const host of hosts) {
    const exited = once(host.child, 'exit', {
      signal: AbortSignal.timeout(5000),
    });
    assert.equal(await request(host, 'stop'), true);
    const [code] = await exited;
    assert.equal(code, 0);
  }
  console.log(
    'Native remote host smoke passed: two isolated processes, authenticated IPC, clean shutdown.',
  );
} finally {
  for (const host of hosts) {
    if (host.child.exitCode === null) host.child.kill();
    host.key.fill(0);
    if (process.platform !== 'win32')
      await unlink(host.endpoint).catch((error) => {
        if (error.code !== 'ENOENT') throw error;
      });
  }
  await rmdir(directory);
}
