import fs from 'node:fs/promises';
import path from 'node:path';

function fail(message) {
  throw new Error(message);
}

function assetUrl(repository, tag, fileName) {
  return `https://github.com/${repository}/releases/download/${tag}/${encodeURIComponent(fileName)}`;
}

async function signatureFor(asset) {
  try {
    return (await fs.readFile(`${asset.filePath}.sig`, 'utf8')).trim();
  } catch {
    return null;
  }
}

export async function generateLatestJson({
  repository,
  tag,
  releaseDirectory,
  publishedAt = new Date(),
}) {
  if (!repository) fail('GITHUB_REPOSITORY is required');
  if (!tag) fail('GITHUB_REF_NAME is required');

  const entries = await fs.readdir(releaseDirectory, {
    recursive: true,
    withFileTypes: true,
  });
  const assets = entries
    .filter((entry) => entry.isFile() && !entry.name.endsWith('.sig'))
    .map((entry) => ({
      fileName: entry.name,
      filePath: path.join(entry.parentPath, entry.name),
    }));

  const definitions = [
    {
      target: 'windows-x86_64-nsis',
      match: (name) => /_x64-setup\.exe$/i.test(name),
    },
    {
      target: 'linux-x86_64-appimage',
      match: (name) => name.endsWith('_amd64.AppImage'),
    },
    {
      target: 'linux-x86_64-deb',
      match: (name) => /_amd64\.deb$/i.test(name),
    },
    {
      target: 'linux-x86_64-rpm',
      match: (name) => /_x86_64\.rpm$/i.test(name),
    },
  ];

  const platforms = {};
  for (const definition of definitions) {
    const matches = assets.filter((asset) => definition.match(asset.fileName));
    if (matches.length !== 1) {
      fail(`Expected one ${definition.target} asset, found ${matches.length}`);
    }
    const asset = matches[0];
    const signature = await signatureFor(asset);
    if (!signature) fail(`Missing signature for ${asset.fileName}`);
    platforms[definition.target] = {
      signature,
      url: assetUrl(repository, tag, asset.fileName),
    };
  }

  const manifest = {
    version: tag.replace(/^v/i, ''),
    pub_date: publishedAt.toISOString(),
    platforms,
  };
  const output = path.join(releaseDirectory, 'latest.json');
  await fs.writeFile(output, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
  return { manifest, output };
}

if (import.meta.filename === process.argv[1]) {
  const { output } = await generateLatestJson({
    repository: process.env.GITHUB_REPOSITORY,
    tag: process.env.GITHUB_REF_NAME,
    releaseDirectory: path.resolve('release-assets'),
  });
  console.log(`Generated ${output}`);
}
