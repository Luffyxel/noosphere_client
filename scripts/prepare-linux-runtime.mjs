import { access, chmod, readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

if (process.platform !== 'linux') {
  throw new Error('Linux runtime preparation must run on Linux.');
}

const projectRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const releaseRoot = path.join(projectRoot, 'src-tauri', 'target', 'release');
const executable = path.join(releaseRoot, 'noosphere-desktop');
const libraries = [
  'libcef.so',
  'libEGL.so',
  'libGLESv2.so',
  'libvk_swiftshader.so',
  'libvulkan.so.1',
].map((name) => path.join(releaseRoot, name));
const resources = [
  executable,
  ...libraries,
  ...[
    'CREDITS.html',
    'chrome-sandbox',
    'chrome_100_percent.pak',
    'chrome_200_percent.pak',
    'icudtl.dat',
    'resources.pak',
    'v8_context_snapshot.bin',
    'vk_swiftshader_icd.json',
  ].map((name) => path.join(releaseRoot, name)),
];

await Promise.all(resources.map((resource) => access(resource)));
if ((await readdir(path.join(releaseRoot, 'locales'))).length === 0) {
  throw new Error('CEF locale resources are missing.');
}

function run(program, args) {
  const result = spawnSync(program, args, { encoding: 'utf8' });
  if (result.status !== 0) {
    throw new Error(
      `${program} failed (${result.status ?? 'unknown'}).\n${result.stderr || result.stdout}`,
    );
  }
}

run('strip', ['--strip-unneeded', ...libraries]);
run('patchelf', [
  '--set-rpath',
  '$ORIGIN:$ORIGIN/../lib/Noosphere',
  executable,
]);
await chmod(path.join(releaseRoot, 'chrome-sandbox'), 0o755);

console.log('Linux Chromium runtime prepared.');
