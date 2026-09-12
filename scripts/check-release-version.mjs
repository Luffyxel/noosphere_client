import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

function cargoPackageVersion(source) {
  const packageSection = source.match(/\[package\]([\s\S]*?)(?:\n\[|$)/);
  const version = packageSection?.[1].match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version)
    throw new Error('Unable to read src-tauri/Cargo.toml package version');
  return version;
}

function cargoLockPackageVersion(source) {
  const packageBlock = source.match(
    /\[\[package\]\]\r?\nname = "noosphere-desktop"\r?\nversion = "([^"]+)"/,
  );
  if (!packageBlock)
    throw new Error('Unable to read noosphere-desktop version from Cargo.lock');
  return packageBlock[1];
}

export async function releaseVersions(rootDirectory = process.cwd()) {
  const [packageSource, lockSource, tauriSource, cargoSource, cargoLockSource] =
    await Promise.all([
      fs.readFile(path.join(rootDirectory, 'package.json'), 'utf8'),
      fs.readFile(path.join(rootDirectory, 'package-lock.json'), 'utf8'),
      fs.readFile(
        path.join(rootDirectory, 'src-tauri', 'tauri.conf.json'),
        'utf8',
      ),
      fs.readFile(path.join(rootDirectory, 'src-tauri', 'Cargo.toml'), 'utf8'),
      fs.readFile(path.join(rootDirectory, 'src-tauri', 'Cargo.lock'), 'utf8'),
    ]);

  const packageJson = JSON.parse(packageSource);
  const packageLock = JSON.parse(lockSource);
  const tauriConfig = JSON.parse(tauriSource);
  return {
    package: packageJson.version,
    packageLock: packageLock.version,
    packageLockRoot: packageLock.packages?.['']?.version,
    tauri: tauriConfig.version,
    cargo: cargoPackageVersion(cargoSource),
    cargoLock: cargoLockPackageVersion(cargoLockSource),
  };
}

export async function checkReleaseVersion(tag, rootDirectory = process.cwd()) {
  const versions = await releaseVersions(rootDirectory);
  const expected = tag ? tag.replace(/^v/i, '') : versions.package;
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(expected)) {
    throw new Error(`Invalid release version: ${expected}`);
  }

  const mismatches = Object.entries(versions).filter(
    ([, version]) => version !== expected,
  );
  if (mismatches.length > 0) {
    const details = mismatches
      .map(([name, version]) => `${name}=${version}`)
      .join(', ');
    throw new Error(`Release version must be ${expected}; found ${details}`);
  }
  return expected;
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  const version = await checkReleaseVersion(process.argv[2]);
  console.log(`Release version ${version} is synchronized`);
}
