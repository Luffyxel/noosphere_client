import { spawn } from 'node:child_process';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptsDirectory = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(scriptsDirectory, '..');
const outputDirectory = path.join(projectRoot, 'release', 'linux');
const image = `noosphere-linux-builder:${process.arch}`;
const testImage = `noosphere-linux-test:${process.arch}`;

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: projectRoot,
      stdio: 'inherit',
      windowsHide: true,
    });
    child.once('error', reject);
    child.once('exit', (code) => {
      if (code === 0) resolve();
      else reject(new Error(`${command} exited with code ${code}`));
    });
  });
}

await mkdir(outputDirectory, { recursive: true });
await run('docker', [
  'build',
  '--network',
  'host',
  '--file',
  'packaging/linux/Dockerfile',
  '--target',
  'test',
  '--tag',
  testImage,
  '.',
]);
await run('docker', [
  'build',
  '--network',
  'host',
  '--file',
  'packaging/linux/Dockerfile',
  '--target',
  'export',
  '--tag',
  image,
  '.',
]);
await run('docker', [
  'run',
  '--rm',
  '--volume',
  `${outputDirectory}:/output`,
  image,
]);
