import { spawnSync } from 'node:child_process';
import {
  access,
  mkdtemp,
  mkdir,
  readFile,
  readdir,
  rename,
  rm,
  writeFile,
} from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'linux') {
  throw new Error('RPM packages must be built on Linux.');
}

const projectRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const packageMetadata = JSON.parse(
  await readFile(path.join(projectRoot, 'package.json'), 'utf8'),
);
const releaseRoot = path.join(projectRoot, 'src-tauri', 'target', 'release');
const bundleDirectory = path.join(releaseRoot, 'bundle', 'rpm');
const finalPackage = path.join(
  bundleDirectory,
  `Noosphere_${packageMetadata.version}_x86_64.rpm`,
);
const icon = path.join(projectRoot, 'src-tauri', 'icons', '128x128.png');
const desktopEntry = path.join(
  projectRoot,
  'packaging',
  'linux',
  'noosphere.desktop',
);
const runtimeFiles = [
  'noosphere-desktop',
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
  'resources.pak',
  'v8_context_snapshot.bin',
  'vk_swiftshader_icd.json',
];

await Promise.all([
  access(icon),
  access(desktopEntry),
  access(path.join(releaseRoot, 'locales')),
  ...runtimeFiles.map((file) => access(path.join(releaseRoot, file))),
]);

await mkdir(bundleDirectory, { recursive: true });
const workingDirectory = await mkdtemp(
  path.join(bundleDirectory, '.rpmbuild-'),
);
for (const directory of [
  'BUILD',
  'BUILDROOT',
  'RPMS',
  'SOURCES',
  'SPECS',
  'SRPMS',
]) {
  await mkdir(path.join(workingDirectory, directory), { recursive: true });
}

const specPath = path.join(workingDirectory, 'SPECS', 'noosphere.spec');
const resourceInstallLines = runtimeFiles
  .filter((file) => file !== 'noosphere-desktop')
  .map(
    (file) =>
      `install -m 0644 %{_noosphere_release}/${file} %{buildroot}/usr/lib/Noosphere/${file}`,
  )
  .join('\n');
const spec = `Name: noosphere
Version: ${packageMetadata.version}
Release: 1%{?dist}
Summary: Encrypted peer-to-peer messaging
License: AGPL-3.0-only
URL: https://github.com/Luffyxel/noosphere_client
BuildArch: x86_64
Requires: libxkbcommon-x11

%global debug_package %{nil}

%description
Noosphere is an encrypted peer-to-peer desktop messenger that uses GitHub as
its persistent journal.

%prep

%build

%install
rm -rf %{buildroot}
install -Dm0755 %{_noosphere_release}/noosphere-desktop %{buildroot}/usr/bin/noosphere-desktop
patchelf --set-rpath /usr/lib/Noosphere %{buildroot}/usr/bin/noosphere-desktop
install -d %{buildroot}/usr/lib/Noosphere
${resourceInstallLines}
cp -a %{_noosphere_release}/locales %{buildroot}/usr/lib/Noosphere/locales
install -Dm0644 %{_noosphere_desktop} %{buildroot}/usr/share/applications/noosphere.desktop
install -Dm0644 %{_noosphere_icon} %{buildroot}/usr/share/icons/hicolor/128x128/apps/noosphere.png

%files
/usr/bin/noosphere-desktop
/usr/lib/Noosphere
/usr/share/applications/noosphere.desktop
/usr/share/icons/hicolor/128x128/apps/noosphere.png
`;

try {
  await writeFile(specPath, spec);
  run('rpmbuild', [
    '-bb',
    specPath,
    '--define',
    `_topdir ${workingDirectory}`,
    '--define',
    `_noosphere_release ${releaseRoot}`,
    '--define',
    `_noosphere_desktop ${desktopEntry}`,
    '--define',
    `_noosphere_icon ${icon}`,
    '--define',
    '_binary_payload w3.zstdio',
  ]);
  const packages = await findPackages(path.join(workingDirectory, 'RPMS'));
  if (packages.length !== 1) {
    throw new Error(`Expected one RPM package, found ${packages.length}.`);
  }
  await rm(finalPackage, { force: true });
  await rename(packages[0], finalPackage);
} finally {
  await rm(workingDirectory, { force: true, recursive: true });
}

console.log(`Built ${path.basename(finalPackage)}.`);

function run(program, args) {
  const result = spawnSync(program, args, { encoding: 'utf8' });
  if (result.status !== 0) {
    throw new Error(
      `${program} failed (${result.status ?? 'unknown'}).\n${result.stderr || result.stdout}`,
    );
  }
}

async function findPackages(directory) {
  const packages = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      packages.push(...(await findPackages(entryPath)));
    } else if (entry.name.endsWith('.rpm')) {
      packages.push(entryPath);
    }
  }
  return packages;
}
