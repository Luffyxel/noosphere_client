import { createHash } from 'node:crypto';
import { createReadStream, createWriteStream } from 'node:fs';
import { chmod, mkdir, rename, rm } from 'node:fs/promises';
import { homedir } from 'node:os';
import path from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';

if (process.platform !== 'linux') {
  throw new Error('Linux packaging tools must be prepared on Linux.');
}

const cacheDirectory = path.join(homedir(), '.cache', 'tauri');
const tools = [
  {
    name: 'linuxdeploy-x86_64.AppImage',
    sha256: 'e762bea85c8eb0d4b3508d46e5c1f037f717d0f9303ae3b4aafc8b04991fa1ef',
    url: 'https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy/linuxdeploy-x86_64.AppImage',
  },
  {
    name: 'linuxdeploy-plugin-appimage.AppImage',
    sha256: '0441769ab38009504d2678c38cd7e526955388dd30a215b4a20afaa5471652f2',
    url: 'https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage',
  },
  {
    name: 'AppRun-x86_64',
    sha256: 'f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f',
    url: 'https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64',
  },
];

await mkdir(cacheDirectory, { recursive: true });
for (const tool of tools) {
  const destination = path.join(cacheDirectory, tool.name);
  if ((await sha256(destination)) !== tool.sha256) {
    await download(tool, destination);
  }
  await chmod(destination, 0o755);
}

console.log('Linux packaging tools verified.');

async function download(tool, destination) {
  const temporary = `${destination}.download-${process.pid}`;
  for (let attempt = 1; attempt <= 6; attempt += 1) {
    try {
      const response = await fetch(tool.url, { redirect: 'follow' });
      if (!response.ok || !response.body) {
        throw new Error(`HTTP ${response.status}`);
      }
      await pipeline(
        Readable.fromWeb(response.body),
        createWriteStream(temporary),
      );
      const actual = await sha256(temporary);
      if (actual !== tool.sha256) {
        throw new Error(`SHA-256 mismatch for ${tool.name}`);
      }
      await rename(temporary, destination);
      return;
    } catch (error) {
      await rm(temporary, { force: true });
      if (attempt === 6) throw error;
      await new Promise((resolve) => setTimeout(resolve, attempt * 2000));
    }
  }
}

async function sha256(file) {
  try {
    const hash = createHash('sha256');
    for await (const chunk of createReadStream(file)) hash.update(chunk);
    return hash.digest('hex');
  } catch (error) {
    if (error?.code === 'ENOENT') return null;
    throw error;
  }
}
