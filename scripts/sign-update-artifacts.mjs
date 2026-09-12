import { spawnSync } from 'node:child_process';
import { access, readFile, readdir, rm } from 'node:fs/promises';
import path from 'node:path';

const version = JSON.parse(await readFile('package.json', 'utf8')).version;
const bundleRoot = path.resolve('src-tauri', 'target', 'release', 'bundle');
const expectedNames = new Set([
  `Noosphere_${version}_amd64.AppImage`,
  `Noosphere_${version}_amd64.deb`,
  `Noosphere_${version}_x86_64.rpm`,
]);
const artifacts = [];

await access(bundleRoot);
for (const entry of await readdir(bundleRoot, {
  recursive: true,
  withFileTypes: true,
})) {
  if (entry.isFile() && expectedNames.has(entry.name)) {
    artifacts.push(path.join(entry.parentPath, entry.name));
  }
}

if (artifacts.length !== expectedNames.size) {
  const found =
    artifacts
      .map(path.basename)
      .sort((left, right) => left.localeCompare(right))
      .join(', ') || 'none';
  throw new Error(
    `Expected ${expectedNames.size} Linux packages, found: ${found}`,
  );
}

const tauriCli = path.resolve('node_modules', '@tauri-apps', 'cli', 'tauri.js');
for (const artifact of artifacts) {
  await rm(`${artifact}.sig`, { force: true });
  const result = spawnSync(
    process.execPath,
    [tauriCli, 'signer', 'sign', artifact],
    { env: process.env, stdio: 'inherit' },
  );
  if (result.status !== 0) {
    throw new Error(`Signing failed for ${path.basename(artifact)}`);
  }
}
