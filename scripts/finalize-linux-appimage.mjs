import { spawnSync } from 'node:child_process';
import {
  access,
  chmod,
  cp,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  realpath,
  rename,
  rm,
  unlink,
} from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'linux') {
  throw new Error('AppImage finalization must run on Linux.');
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
  'appimage',
);
const appDirectory = path.join(bundleDirectory, 'Noosphere.AppDir');
const appLibraryDirectory = path.join(appDirectory, 'usr', 'lib');
const cefResourceDirectory = path.join(appLibraryDirectory, 'Noosphere');
const releaseRoot = path.join(projectRoot, 'src-tauri', 'target', 'release');
const finalAppImage = path.join(
  bundleDirectory,
  `Noosphere_${packageMetadata.version}_amd64.AppImage`,
);
const stagedIcon = path.join(bundleDirectory, 'noosphere.png');
const executable = path.join(releaseRoot, 'noosphere-desktop');
const bundledExecutable = path.join(
  appDirectory,
  'usr',
  'bin',
  'noosphere-desktop',
);
const linuxDeploy =
  process.env.LINUXDEPLOY_PATH ??
  path.join(
    process.env.HOME ?? '/root',
    '.cache',
    'tauri',
    'linuxdeploy-x86_64.AppImage',
  );
const appImagePlugin =
  process.env.LINUXDEPLOY_PLUGIN_APPIMAGE_PATH ??
  path.join(
    process.env.HOME ?? '/root',
    '.cache',
    'tauri',
    'linuxdeploy-plugin-appimage.AppImage',
  );
const appRun =
  process.env.APP_RUN_PATH ??
  path.join(process.env.HOME ?? '/root', '.cache', 'tauri', 'AppRun-x86_64');

const nssCandidates = [
  '/usr/lib/x86_64-linux-gnu/nss',
  '/usr/lib64/nss',
  '/usr/lib/nss',
];
const nssDirectory = await firstExistingDirectory(nssCandidates);
if (!nssDirectory) {
  throw new Error('NSS runtime modules were not found on the build system.');
}
const sqliteLibrary = await firstExistingFile([
  '/usr/lib/x86_64-linux-gnu/libsqlite3.so.0',
  '/usr/lib64/libsqlite3.so.0',
  '/usr/lib/libsqlite3.so.0',
]);
if (!sqliteLibrary) {
  throw new Error('SQLite runtime library was not found on the build system.');
}

await rm(appDirectory, { force: true, recursive: true });
await mkdir(path.dirname(bundledExecutable), { recursive: true });
await mkdir(cefResourceDirectory, { recursive: true });
await cp(executable, bundledExecutable);
await chmod(bundledExecutable, 0o755);
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

await Promise.all([
  access(linuxDeploy),
  access(appImagePlugin),
  access(appRun),
]);
await cp(
  path.join(projectRoot, 'src-tauri', 'icons', '128x128@2x.png'),
  stagedIcon,
);
try {
  run(
    linuxDeploy,
    [
      '--appimage-extract-and-run',
      '--appdir',
      appDirectory,
      '--executable',
      bundledExecutable,
      '--desktop-file',
      path.join(projectRoot, 'packaging', 'linux', 'noosphere.desktop'),
      '--icon-file',
      stagedIcon,
    ],
    {
      ...process.env,
      APPIMAGE_EXTRACT_AND_RUN: '1',
      NO_STRIP: 'true',
    },
  );
} finally {
  await rm(stagedIcon, { force: true });
}
await cp(appRun, path.join(appDirectory, 'AppRun'), { force: true });
await chmod(path.join(appDirectory, 'AppRun'), 0o755);

for (const entry of await readdir(cefResourceDirectory, {
  withFileTypes: true,
})) {
  if (entry.name === 'libcef.so') continue;
  await cp(
    path.join(cefResourceDirectory, entry.name),
    path.join(appLibraryDirectory, entry.name),
    { force: true, recursive: true },
  );
}
for (const entry of await readdir(nssDirectory, { withFileTypes: true })) {
  if (!entry.name.endsWith('.so')) continue;
  await cp(
    path.join(nssDirectory, entry.name),
    path.join(appLibraryDirectory, entry.name),
    { force: true },
  );
}
await cp(
  await realpath(sqliteLibrary),
  path.join(appLibraryDirectory, 'libsqlite3.so.0'),
  { force: true },
);

await unlink(path.join(cefResourceDirectory, 'libcef.so'));
await chmod(path.join(appLibraryDirectory, 'chrome-sandbox'), 0o755);

const outputDirectory = await mkdtemp(path.join(bundleDirectory, '.finalize-'));
try {
  const result = spawnSync(
    appImagePlugin,
    ['--appimage-extract-and-run', `--appdir=${appDirectory}`],
    {
      cwd: outputDirectory,
      encoding: 'utf8',
      env: {
        ...process.env,
        APPIMAGE_EXTRACT_AND_RUN: '1',
        ARCH: 'x86_64',
        LINUXDEPLOY_OUTPUT_VERSION: packageMetadata.version,
      },
    },
  );
  if (result.status !== 0) {
    throw new Error(
      `AppImage rebuild failed (${result.status ?? 'unknown'}).\n${result.stderr || result.stdout}`,
    );
  }
  const outputs = (await readdir(outputDirectory)).filter((name) =>
    name.endsWith('.AppImage'),
  );
  if (outputs.length !== 1) {
    throw new Error(`Expected one rebuilt AppImage, found ${outputs.length}.`);
  }
  await rm(finalAppImage, { force: true });
  await rename(path.join(outputDirectory, outputs[0]), finalAppImage);
} finally {
  await rm(outputDirectory, { force: true, recursive: true });
}

console.log(`Finalized ${path.basename(finalAppImage)}.`);

function run(program, args, env = process.env) {
  const result = spawnSync(program, args, { encoding: 'utf8', env });
  if (result.status !== 0) {
    throw new Error(
      `${path.basename(program)} failed (${result.status ?? 'unknown'}).\n${result.stdout}\n${result.stderr}`,
    );
  }
}

async function firstExistingDirectory(candidates) {
  for (const candidate of candidates) {
    try {
      if ((await readdir(candidate)).length > 0) return candidate;
    } catch (error) {
      if (error?.code !== 'ENOENT') throw error;
    }
  }
  return null;
}

async function firstExistingFile(candidates) {
  for (const candidate of candidates) {
    try {
      await access(candidate);
      return candidate;
    } catch (error) {
      if (error?.code !== 'ENOENT') throw error;
    }
  }
  return null;
}
