import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const aliases = [
  {
    name: 'Windows installer',
    match: (fileName) =>
      /^Noosphere_\d+\.\d+\.\d+_x64-setup\.exe$/i.test(fileName),
    target: 'Noosphere-Windows-Setup-x64.exe',
  },
  {
    name: 'Windows portable',
    match: (fileName) =>
      /^Noosphere_\d+\.\d+\.\d+_portable_x64\.exe$/i.test(fileName),
    target: 'Noosphere-Windows-Portable-x64.exe',
  },
  {
    name: 'Linux AppImage',
    match: (fileName) =>
      /^Noosphere_\d+\.\d+\.\d+_amd64\.AppImage$/.test(fileName),
    target: 'Noosphere-Linux-AppImage-x64.AppImage',
  },
  {
    name: 'Linux DEB',
    match: (fileName) => /^Noosphere_\d+\.\d+\.\d+_amd64\.deb$/i.test(fileName),
    target: 'Noosphere-Linux-DEB-x64.deb',
  },
  {
    name: 'Linux RPM',
    match: (fileName) =>
      /^Noosphere_\d+\.\d+\.\d+_x86_64\.rpm$/i.test(fileName),
    target: 'Noosphere-Linux-RPM-x64.rpm',
  },
];

export async function createReleaseAliases(releaseDirectory) {
  const entries = await fs.readdir(releaseDirectory, {
    recursive: true,
    withFileTypes: true,
  });
  const files = entries
    .filter((entry) => entry.isFile())
    .map((entry) => ({
      fileName: entry.name,
      filePath: path.join(entry.parentPath, entry.name),
    }));

  const created = [];
  for (const alias of aliases) {
    const matches = files.filter((file) => alias.match(file.fileName));
    if (matches.length !== 1) {
      throw new Error(
        `Expected one ${alias.name} asset, found ${matches.length}`,
      );
    }
    const destination = path.join(releaseDirectory, alias.target);
    await fs.copyFile(matches[0].filePath, destination);
    created.push(destination);
  }
  return created;
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  const releaseDirectory = path.resolve(process.argv[2] ?? 'release-assets');
  const created = await createReleaseAliases(releaseDirectory);
  console.log(`Created ${created.length} stable release assets`);
}
