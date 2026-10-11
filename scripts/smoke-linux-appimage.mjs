import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'linux') {
  throw new Error('The AppImage smoke test must run on Linux.');
}
const root = new URL('../', import.meta.url);
const { version } = JSON.parse(
  await readFile(new URL('package.json', root), 'utf8'),
);
process.env.NOOSPHERE_SMOKE_EXECUTABLE = fileURLToPath(
  new URL(
    `src-tauri/target/release/bundle/appimage/Noosphere_${version}_amd64.AppImage`,
    root,
  ),
);
process.env.NOOSPHERE_SMOKE_EXECUTABLE_ARGUMENT = '--appimage-extract-and-run';
const legacy = await mkdtemp(path.join(os.tmpdir(), 'noosphere-relaunch-'));
const legacyApp = path.join(legacy, '.mount_noosphereOld');
const legacyLibraries = path.join(legacyApp, 'usr', 'lib');
await mkdir(legacyLibraries, { recursive: true });
for (const name of ['libstdc++.so.6', 'libfontconfig.so.1', 'libGLX.so.0']) {
  await writeFile(
    path.join(legacyLibraries, name),
    'obsolete AppImage library',
  );
}
const previous = {
  APPDIR: process.env.APPDIR,
  LD_LIBRARY_PATH: process.env.LD_LIBRARY_PATH,
};
process.env.APPDIR = legacyApp;
process.env.LD_LIBRARY_PATH = [legacyLibraries, previous.LD_LIBRARY_PATH]
  .filter(Boolean)
  .join(':');
process.env.NOOSPHERE_VERBOSE = '1';
process.env.NOOSPHERE_SMOKE_EXPECT_VERSION = version;
try {
  await import('./smoke-tauri.mjs');
} finally {
  for (const [name, value] of Object.entries(previous)) {
    if (value === undefined) delete process.env[name];
    else process.env[name] = value;
  }
  await rm(legacy, { recursive: true, force: true });
}
