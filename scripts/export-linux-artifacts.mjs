import { createHash } from 'node:crypto';
import {
  copyFile,
  mkdir,
  readdir,
  readFile,
  writeFile,
} from 'node:fs/promises';
import path from 'node:path';

const outputDirectory = path.resolve(process.argv[2] ?? 'release/linux');
const bundleDirectory = path.resolve('src-tauri/target/release/bundle');
const extensions = new Set(['.AppImage', '.deb', '.rpm']);

async function findPackages(directory) {
  const found = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) found.push(...(await findPackages(entryPath)));
    else if (extensions.has(path.extname(entry.name))) found.push(entryPath);
  }
  return found;
}

await mkdir(outputDirectory, { recursive: true });
const packages = (await findPackages(bundleDirectory)).sort();
if (packages.length !== extensions.size) {
  throw new Error(
    `Expected ${extensions.size} Linux packages, found ${packages.length}`,
  );
}

const checksums = [];
for (const source of packages) {
  const name = path.basename(source);
  const content = await readFile(source);
  await copyFile(source, path.join(outputDirectory, name));
  checksums.push(
    `${createHash('sha256').update(content).digest('hex')}  ${name}`,
  );
}
await writeFile(
  path.join(outputDirectory, 'SHA256SUMS.txt'),
  `${checksums.join('\n')}\n`,
);
