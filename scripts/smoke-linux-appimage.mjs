import { readFile } from 'node:fs/promises';
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
await import('./smoke-tauri.mjs');
