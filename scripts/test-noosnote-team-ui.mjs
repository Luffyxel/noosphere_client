import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from 'playwright';
import { build, preview } from 'vite';
const root = path.resolve(import.meta.dirname, '..');
const output = path.join(root, 'remote-test-results/noosnote-team');
await mkdir(output, { recursive: true });
const workspace = '0f4fbbe5-3051-4944-a537-0342c58250ad',
  pageId = 'd02523d7-7538-4848-ae47-4baef0c97f53';
const workspacePath = `notes/${workspace}/workspace.json`,
  pagePath = `notes/${workspace}/pages/${pageId}.json`;
const files = {
  [workspacePath]: {
    version: 1,
    kind: 'workspace',
    id: workspace,
    title: 'Atelier · Projet commun',
    icon: '🧩',
    color: '#87bda4',
    pages: [
      {
        id: pageId,
        title: 'Préparer notre voyage',
        icon: '🗺️',
        parentId: null,
      },
    ],
  },
  [pagePath]: {
    version: 1,
    kind: 'page',
    id: pageId,
    workspaceId: workspace,
    title: 'Préparer notre voyage',
    icon: '🗺️',
    content: {
      type: 'doc',
      content: [
        {
          type: 'paragraph',
          attrs: { id: '0f95d3cd-126d-42b6-9fd0-dc40c5d39fb7' },
          content: [{ type: 'text', text: 'Nos idées pour le voyage.' }],
        },
      ],
    },
  },
};
function merge(left, right) {
  if (!left || !right) return right;
  if (Array.isArray(left) && Array.isArray(right)) {
    if ([...left, ...right].every((v) => v && typeof v === 'object' && v.id)) {
      const map = new Map(left.map((v) => [v.id, v]));
      for (const v of right) map.set(v.id, merge(map.get(v.id), v));
      return [...map.values()];
    }
    return right;
  }
  if (typeof left === 'object' && typeof right === 'object') {
    const value = { ...left, ...right };
    for (const key of Object.keys(left)) {
      if (key === 'collaborationUpdates') {
        value[key] = [
          ...new Set([...(left[key] || []), ...(right[key] || [])]),
        ];
      } else if (key in right) value[key] = merge(left[key], right[key]);
    }
    return value;
  }
  return right;
}
const outDir = path.join(output, 'production');
await build({
  configFile: path.join(root, 'vite.tauri.config.ts'),
  build: {
    outDir,
    rolldownOptions: { input: path.join(root, 'tests/noosnote-team-ui.html') },
  },
});
const server = await preview({
  build: { outDir },
  configFile: path.join(root, 'vite.tauri.config.ts'),
  preview: { host: '127.0.0.1', port: 5527, strictPort: true },
  plugins: [
    {
      name: 'team-test-api',
      configurePreviewServer(server) {
        server.middlewares.use('/team-test-api', async (req, res) => {
          let body = '';
          for await (const chunk of req) body += chunk;
          try {
            const { method, actor, value } = JSON.parse(body);
            if (method === 'sync') {
              if (actor === 4 && value.length) {
                res.statusCode = 403;
                res.end('Lecture seule');
                return;
              }
              for (const { path, value: next } of value) {
                files[path] = merge(files[path], next);
              }
            }
            res.setHeader('Content-Type', 'application/json');
            res.end(JSON.stringify(files));
          } catch (error) {
            res.statusCode = 500;
            res.end(String(error));
          }
        });
      },
    },
  ],
});
let browser;
const errors = [];
try {
  browser = await chromium.launch({
    headless: true,
    ...(process.env.NOOSPHERE_TEST_BROWSER
      ? { executablePath: process.env.NOOSPHERE_TEST_BROWSER }
      : {}),
  });
  const pages = await Promise.all(
    [1, 2, 3, 4].map(async (actor) => {
      const context = await browser.newContext({
        viewport: { width: 1480, height: 1020 },
      });
      const page = await context.newPage();
      page.on('pageerror', (e) => errors.push(e.message));
      await page.goto(
        `http://127.0.0.1:5527/tests/noosnote-team-ui.html?actor=${actor}`,
        { waitUntil: 'domcontentloaded', timeout: 90000 },
      );
      await page.locator('.note-prose').waitFor({ timeout: 90000 });
      return page;
    }),
  );
  await Promise.all(
    pages.slice(0, 3).map(async (page, i) => {
      await page.locator('.note-prose').click();
      await page.keyboard.press('Control+End');
      await page.keyboard.type(
        [' Alice propose Paris.', ' Bob propose Rome.', ' Clara propose Oslo.'][
          i
        ],
      );
    }),
  );
  for (let round = 0; round < 3; round++)
    for (const page of pages.slice(0, 3)) {
      await page.keyboard.press('Control+s');
      await page.waitForTimeout(650);
    }
  for (const page of pages.slice(0, 3))
    await page.waitForFunction(
      () =>
        ['Paris', 'Rome', 'Oslo'].every((word) =>
          document.querySelector('.note-prose')?.textContent?.includes(word),
        ),
      null,
      { timeout: 15000 },
    );
  for (const page of pages.slice(0, 3))
    assert.equal(
      (await page.locator('.note-prose').innerText()).match(/Nos idées/g)
        ?.length,
      1,
    );
  assert.equal(
    await pages[3].locator('.note-prose').getAttribute('contenteditable'),
    'false',
  );
  await pages[0]
    .getByRole('button', { name: 'Discussions de la page', exact: true })
    .click();
  await pages[0]
    .getByRole('textbox', { name: 'Nouveau commentaire' })
    .fill('@bo');
  await pages[0]
    .getByLabel('Suggestions de mentions')
    .getByRole('button')
    .first()
    .click();
  await pages[0]
    .getByRole('textbox', { name: 'Nouveau commentaire' })
    .press('End');
  await pages[0].keyboard.type('tu réserves le train ?');
  await pages[0].getByRole('button', { name: 'Envoyer', exact: true }).click();
  await pages[0]
    .getByRole('button', { name: 'Fermer la collaboration' })
    .click();
  await pages[0].keyboard.press('Control+s');
  await pages[0].waitForTimeout(800);
  await pages[1].keyboard.press('Control+s');
  await pages[1].waitForTimeout(800);
  await pages[1]
    .getByRole('button', { name: 'Discussions de la page', exact: true })
    .click();
  await pages[1].getByRole('button', { name: 'Pour moi', exact: true }).click();
  await pages[1]
    .getByText('tu réserves le train ?', { exact: false })
    .first()
    .waitFor();
  await pages[1].getByRole('button', { name: 'Répondre', exact: true }).click();
  await pages[1]
    .getByRole('textbox', { name: 'Répondre à la discussion' })
    .fill('Oui, je m’en occupe !');
  await pages[1].getByRole('button', { name: 'Envoyer', exact: true }).click();
  await pages[1].getByText('Oui, je m’en occupe !', { exact: true }).waitFor();
  await pages[1].getByRole('button', { name: 'Résoudre', exact: true }).click();
  await pages[1].getByRole('button', { name: 'Résolues', exact: true }).click();
  await pages[1]
    .getByRole('button', { name: 'Rouvrir', exact: true })
    .waitFor();
  await pages[1].getByRole('button', { name: 'Membres', exact: true }).click();
  await pages[1]
    .getByRole('textbox', { name: 'Ton pseudo dans cet espace' })
    .fill('Bob le voyageur');
  await pages[1].getByRole('button', { name: 'Enregistrer le pseudo' }).click();
  await pages[1]
    .getByRole('button', { name: 'Fermer la collaboration' })
    .click();
  await pages[1].keyboard.press('Control+s');
  await pages[1].waitForTimeout(900);
  await pages[0].keyboard.press('Control+s');
  await pages[0].waitForTimeout(900);
  await pages[0]
    .getByRole('button', { name: 'Discussions de la page', exact: true })
    .click();
  await pages[0].getByRole('button', { name: 'Résolues', exact: true }).click();
  await pages[0].getByText('Bob le voyageur', { exact: true }).waitFor();
  await pages[0].screenshot({
    path: path.join(output, 'discussions.png'),
    fullPage: true,
  });
  await pages[0].getByRole('button', { name: 'Membres', exact: true }).click();
  await pages[0].screenshot({
    path: path.join(output, 'members.png'),
    fullPage: true,
  });
  await pages[0]
    .getByRole('button', { name: 'Historique', exact: true })
    .click();
  await pages[0]
    .getByRole('button', { name: 'Voir les modifications' })
    .waitFor();
  await pages[2].reload();
  await pages[2].locator('.note-prose').waitFor();
  assert.match(await pages[2].locator('.note-prose').innerText(), /Paris/);
  assert.match(await pages[2].locator('.note-prose').innerText(), /Rome/);
  assert.match(await pages[2].locator('.note-prose').innerText(), /Oslo/);
  await pages[0].setViewportSize({ width: 1024, height: 768 });
  await pages[0].screenshot({
    path: path.join(output, 'compact.png'),
    fullPage: true,
  });
  assert.deepEqual(errors, []);
  await writeFile(
    path.join(output, 'result.json'),
    JSON.stringify(
      {
        clients: 4,
        concurrentEditors: 3,
        convergence: true,
        mentions: true,
        replies: true,
        resolution: true,
        pseudonyms: true,
        history: true,
        readOnly: true,
        reopen: true,
      },
      null,
      2,
    ),
  );
  console.log(
    'Three editors converge; mentions, replies, resolution, pseudonyms, history and read-only access passed.',
  );
} catch (error) {
  console.error(errors);
  throw error;
} finally {
  await browser?.close();
  await new Promise((resolve) => server.httpServer.close(resolve));
}
