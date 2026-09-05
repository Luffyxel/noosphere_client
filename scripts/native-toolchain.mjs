import { existsSync, readdirSync } from 'node:fs';
import { delimiter, dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const [tool, ...args] = process.argv.slice(2);

if (tool !== 'cargo' && tool !== 'tauri') {
  console.error(
    'Usage: node scripts/native-toolchain.mjs <cargo|tauri> [...args]',
  );
  process.exit(2);
}

function commandWorks(command, commandArgs) {
  const result = spawnSync(command, commandArgs, {
    env: process.env,
    stdio: 'ignore',
    windowsHide: true,
  });
  return result.status === 0;
}

function findWindowsProtoc() {
  const packageRoot = process.env.LOCALAPPDATA
    ? join(process.env.LOCALAPPDATA, 'Microsoft', 'WinGet', 'Packages')
    : null;
  if (!packageRoot || !existsSync(packageRoot)) return null;

  const packages = readdirSync(packageRoot, { withFileTypes: true })
    .filter(
      (entry) =>
        entry.isDirectory() && entry.name.startsWith('Google.Protobuf_'),
    )
    .map((entry) => join(packageRoot, entry.name, 'bin', 'protoc.exe'))
    .filter(existsSync)
    .sort();
  return packages.at(-1) ?? null;
}

const environment = { ...process.env };
const pathKey =
  Object.keys(environment).find((name) => name.toLowerCase() === 'path') ??
  'PATH';
const userCargoDirectory = process.env.USERPROFILE
  ? join(process.env.USERPROFILE, '.cargo', 'bin')
  : null;
if (userCargoDirectory && existsSync(userCargoDirectory)) {
  environment[pathKey] =
    `${userCargoDirectory}${delimiter}${environment[pathKey] ?? ''}`;
}
const cargoCommandWithoutBuildScripts = new Set([
  'deny',
  'fmt',
  'metadata',
  'tree',
]);
const needsProtoc =
  tool === 'tauri' ||
  (tool === 'cargo' && !cargoCommandWithoutBuildScripts.has(args[0]));
if (
  needsProtoc &&
  !environment.PROTOC &&
  !commandWorks('protoc', ['--version'])
) {
  const protoc = process.platform === 'win32' ? findWindowsProtoc() : null;
  if (!protoc) {
    console.error(
      'protoc est requis pour compiler la révision épinglée de libsignal.',
    );
    process.exit(1);
  }
  environment.PROTOC = protoc;
  environment[pathKey] =
    `${dirname(protoc)}${delimiter}${environment[pathKey] ?? ''}`;
}

let command;
let commandArgs;
if (tool === 'cargo') {
  const userCargo = userCargoDirectory
    ? join(userCargoDirectory, 'cargo.exe')
    : null;
  command =
    process.platform === 'win32' && userCargo && existsSync(userCargo)
      ? userCargo
      : 'cargo';
  commandArgs = args;
} else {
  command = process.execPath;
  commandArgs = [resolve('node_modules/@tauri-apps/cli/tauri.js'), ...args];
}

const result = spawnSync(command, commandArgs, {
  env: environment,
  stdio: 'inherit',
  windowsHide: true,
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
