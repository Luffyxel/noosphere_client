import { spawnSync } from 'node:child_process';
import {
  chmod,
  cp,
  mkdir,
  mkdtemp,
  readFile,
  rename,
  rm,
  writeFile,
} from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'linux') {
  throw new Error('DEB finalization must run on Linux.');
}

const projectRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const packageMetadata = JSON.parse(
  await readFile(path.join(projectRoot, 'package.json'), 'utf8'),
);
const bundleDirectory = path.join(
  projectRoot,
  'src-tauri',
  'target',
  'release',
  'bundle',
  'deb',
);
const releaseRoot = path.join(projectRoot, 'src-tauri', 'target', 'release');
const packagePath = path.join(
  bundleDirectory,
  `Noosphere_${packageMetadata.version}_amd64.deb`,
);
const workingDirectory = await mkdtemp(
  path.join(bundleDirectory, '.finalize-'),
);
const unpackedDirectory = path.join(workingDirectory, 'package');
const rebuiltPackage = path.join(workingDirectory, path.basename(packagePath));

try {
  run('dpkg-deb', ['--raw-extract', packagePath, unpackedDirectory]);
  const cefResourceDirectory = path.join(
    unpackedDirectory,
    'usr',
    'lib',
    'Noosphere',
  );
  await mkdir(cefResourceDirectory, { recursive: true });
  for (const entry of [
    'CREDITS.html',
    'chrome-sandbox',
    'chrome_100_percent.pak',
    'chrome_200_percent.pak',
    'icudtl.dat',
    'libEGL.so',
    'libGLESv2.so',
    'libcef.so',
    'libvk_swiftshader.so',
    'libvulkan.so.1',
    'locales',
    'resources.pak',
    'v8_context_snapshot.bin',
    'vk_swiftshader_icd.json',
  ]) {
    await cp(
      path.join(releaseRoot, entry),
      path.join(cefResourceDirectory, entry),
      {
        force: true,
        recursive: true,
      },
    );
  }
  await chmod(path.join(cefResourceDirectory, 'chrome-sandbox'), 0o755);
  await writeFile(
    path.join(cefResourceDirectory, 'noosphere-package-kind'),
    'deb\n',
  );
  const controlPath = path.join(unpackedDirectory, 'DEBIAN', 'control');
  const control = await readFile(controlPath, 'utf8');
  const updated = control.replace(/^Depends:\s*(.+)$/m, (_, value) => {
    const remoteDesktopDependencies = [
      'gstreamer1.0-libav',
      'gstreamer1.0-pipewire',
      'gstreamer1.0-plugins-bad',
      'gstreamer1.0-plugins-base',
      'gstreamer1.0-plugins-good',
      'gstreamer1.0-x',
      'xdg-desktop-portal',
    ];
    const dependencies = value
      .split(',')
      .map((dependency) => dependency.trim())
      .filter(
        (dependency) => dependency && !dependency.startsWith('libwebkit2gtk-'),
      );
    return `Depends: ${[
      ...new Set([...dependencies, ...remoteDesktopDependencies]),
    ].join(', ')}`;
  });
  if (updated === control) {
    throw new Error('The DEB dependency list could not be updated.');
  }
  await writeFile(controlPath, updated);
  run('dpkg-deb', [
    '--build',
    '--root-owner-group',
    unpackedDirectory,
    rebuiltPackage,
  ]);
  await rm(packagePath, { force: true });
  await rename(rebuiltPackage, packagePath);
} finally {
  await rm(workingDirectory, { force: true, recursive: true });
}

console.log(`Finalized ${path.basename(packagePath)}.`);

function run(program, args) {
  const result = spawnSync(program, args, { encoding: 'utf8' });
  if (result.status !== 0) {
    throw new Error(
      `${program} failed (${result.status ?? 'unknown'}).\n${result.stderr || result.stdout}`,
    );
  }
}
