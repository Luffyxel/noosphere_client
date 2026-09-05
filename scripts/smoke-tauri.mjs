import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { readFile, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptsDirectory = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(scriptsDirectory, '..');
const releaseRoot = path.join(projectRoot, 'src-tauri', 'target', 'release');
const executable = process.env.NOOSPHERE_SMOKE_EXECUTABLE
  ? path.resolve(process.env.NOOSPHERE_SMOKE_EXECUTABLE)
  : path.join(
      releaseRoot,
      process.platform === 'win32'
        ? 'noosphere-desktop.exe'
        : 'noosphere-desktop',
    );
const smokeUserDataPath = path.join(releaseRoot, 'smoke-user-data');

function startInstance(resultPath, holdMilliseconds) {
  const child = spawn(executable, [], {
    env: {
      ...process.env,
      NOOSPHERE_SMOKE_TEST: '1',
      NOOSPHERE_SMOKE_HOLD_MS: String(holdMilliseconds),
      NOOSPHERE_SMOKE_RESULT_PATH: resultPath,
      NOOSPHERE_SMOKE_USER_DATA: smokeUserDataPath,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:
        '--use-fake-device-for-media-stream',
      ...(process.platform === 'linux'
        ? { WEBKIT_DISABLE_COMPOSITING_MODE: '1' }
        : {}),
    },
    windowsHide: true,
  });
  let output = '';
  child.stdout.on('data', (chunk) => {
    output += chunk.toString();
  });
  child.stderr.on('data', (chunk) => {
    output += chunk.toString();
  });
  return { child, output: () => output, startedAt: Date.now() };
}

async function waitForResult(instance, resultPath) {
  for (let attempt = 0; attempt < 600; attempt += 1) {
    try {
      return {
        result: JSON.parse(await readFile(resultPath, 'utf8')),
        elapsedMilliseconds: Date.now() - instance.startedAt,
      };
    } catch (error) {
      if (error?.code !== 'ENOENT') throw error;
    }
    if (instance.child.exitCode !== null) break;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(
    instance.child.exitCode === null
      ? `Le smoke test Tauri a dépassé 60 secondes.\n${instance.output()}`
      : `Aucun résultat Tauri produit (code ${instance.child.exitCode}).\n${instance.output()}`,
  );
}

async function stopInstance(instance) {
  if (!instance || instance.child.exitCode !== null) return;
  instance.child.kill();
  await Promise.race([
    once(instance.child, 'exit'),
    new Promise((resolve) => setTimeout(resolve, 5_000)),
  ]);
  if (instance.child.exitCode === null && process.platform === 'win32') {
    const terminator = spawn(
      'taskkill.exe',
      ['/pid', String(instance.child.pid), '/t', '/f'],
      { windowsHide: true, stdio: 'ignore' },
    );
    await Promise.race([
      once(terminator, 'exit'),
      new Promise((resolve) => setTimeout(resolve, 5_000)),
    ]);
  }
}

function validResult(result) {
  return Boolean(
    result?.version === 1 &&
    result.nativeBridge &&
    result.socialBridge &&
    result.webRtcDataChannel &&
    result.webRtcMedia &&
    result.mediaPermission &&
    result.brandAssets &&
    Number.isSafeInteger(result.instanceProfileSlot),
  );
}

const firstResultPath = path.join(releaseRoot, 'smoke-result-first.json');
const secondResultPath = path.join(releaseRoot, 'smoke-result-second.json');
await Promise.all([
  rm(firstResultPath, { force: true }),
  rm(secondResultPath, { force: true }),
  rm(smokeUserDataPath, { force: true, recursive: true }),
]);

const first = startInstance(firstResultPath, 20_000);
let second = null;
try {
  const firstMeasurement = await waitForResult(first, firstResultPath);
  const firstResult = firstMeasurement.result;
  second = startInstance(secondResultPath, 0);
  const secondMeasurement = await waitForResult(second, secondResultPath);
  const secondResult = secondMeasurement.result;
  if (
    !validResult(firstResult) ||
    !validResult(secondResult) ||
    firstResult.instanceProfileSlot === secondResult.instanceProfileSlot
  ) {
    throw new Error(
      `Le smoke test Tauri a échoué.\n${JSON.stringify({ firstResult, secondResult })}`,
    );
  }
  console.log(
    `NOOSPHERE_TAURI_SMOKE=${JSON.stringify({
      firstProfile: firstResult.instanceProfileSlot,
      secondProfile: secondResult.instanceProfileSlot,
      webRtcDataChannel: true,
      webRtcMedia: true,
      mediaPermission: true,
      brandAssets: true,
      isolated: true,
      firstReadyMilliseconds: firstMeasurement.elapsedMilliseconds,
      secondReadyMilliseconds: secondMeasurement.elapsedMilliseconds,
    })}`,
  );
} finally {
  await Promise.all([stopInstance(first), stopInstance(second)]);
  await Promise.all([
    rm(firstResultPath, { force: true }),
    rm(secondResultPath, { force: true }),
    rm(smokeUserDataPath, { force: true, recursive: true }),
  ]);
}
