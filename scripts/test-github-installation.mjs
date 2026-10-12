import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const github = await readFile(
  new URL('../src-tauri/src/github.rs', import.meta.url),
  'utf8',
);
const notes = await readFile(
  new URL('../src-tauri/src/noosnote.rs', import.meta.url),
  'utf8',
);
const manifest = await readFile(
  new URL('../src-tauri/Cargo.toml', import.meta.url),
  'utf8',
);
function extract(source, name, indentation = '') {
  const pattern = new RegExp(
    `^${indentation}(?:pub )?fn ${name}\\b[\\s\\S]*?^${indentation}}`,
    'm',
  );
  const result = source.match(pattern)?.[0];
  if (!result) throw new Error(`Rust function not found: ${name}`);
  return result;
}
const jsonVersion = manifest.match(/^serde_json = "([^"\r\n]+)"/m)?.[1];
const appSlug = github.match(/^pub const APP_SLUG:[^\r\n]+/m)?.[0];
if (!jsonVersion || !appSlug)
  throw new Error('Missing installation test dependency.');
const temporaryRoot = path.resolve(os.tmpdir());
const directory = await mkdtemp(
  path.join(temporaryRoot, 'noosphere-installation-test-'),
);
const relative = path.relative(temporaryRoot, directory);
if (!relative || relative.startsWith('..') || path.isAbsolute(relative))
  throw new Error('Invalid test directory.');
try {
  await writeFile(
    path.join(directory, 'Cargo.toml'),
    `[package]\nname = "noosphere-installation-test"\nversion = "0.0.0"\nedition = "2024"\n[dependencies]\nserde_json = "${jsonVersion}"\n[[test]]\nname = "installation"\npath = "installation.rs"\n`,
  );
  await writeFile(
    path.join(directory, 'installation.rs'),
    [
      'use serde_json::{Map, Value};',
      '#[derive(Debug)] enum Error { InvalidInstallation, InvalidGitHubResponse }',
      'type Result<T> = std::result::Result<T, Error>;',
      appSlug,
      `mod noosnote { ${extract(notes, 'repository_name')} }`,
      extract(github, 'required_string'),
      extract(github, 'positive_u64'),
      extract(github, 'validate_installation'),
      '#[test]',
      extract(
        github,
        'installation_is_limited_to_dedicated_repositories_and_exact_permissions',
        '    ',
      ),
    ].join('\n'),
  );
  const result = spawnSync(
    'cargo',
    [
      'test',
      '--manifest-path',
      path.join(directory, 'Cargo.toml'),
      '--test',
      'installation',
    ],
    {
      stdio: 'inherit',
      windowsHide: true,
      env: { ...process.env, CARGO_TARGET_DIR: path.join(directory, 'target') },
    },
  );
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(`Installation test exited with ${result.status}.`);
} finally {
  await rm(directory, { recursive: true, force: true });
}
