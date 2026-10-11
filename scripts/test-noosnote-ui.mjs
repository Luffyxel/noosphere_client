import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { chromium } from 'playwright';

const root = path.resolve(import.meta.dirname, '..');
const output = path.resolve(root, 'remote-test-results/noosnote-ui');
await mkdir(output, { recursive: true });
const port = 5515;
const base = process.env.NOOSPHERE_TEST_URL ?? `http://127.0.0.1:${port}`;
const server = process.env.NOOSPHERE_TEST_URL
  ? null
  : spawn(
      process.execPath,
      [
        'node_modules/vite/bin/vite.js',
        '--config',
        'vite.tauri.config.ts',
        '--host',
        '127.0.0.1',
        '--port',
        String(port),
        '--strictPort',
      ],
      { cwd: root, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true },
    );
let diagnostics = '';
server?.stdout.on('data', (bytes) => {
  diagnostics += bytes;
});
server?.stderr.on('data', (bytes) => {
  diagnostics += bytes;
});
let browser;
const lifetime = setInterval(() => {}, 1000);
try {
  let ready = false;
  for (let i = 0; i < 80; i += 1) {
    try {
      if ((await fetch(`${base}/tests/noosnote-ui.html`)).ok) {
        ready = true;
        break;
      }
    } catch {}
    if (server?.exitCode !== null && server?.exitCode !== undefined)
      throw new Error(diagnostics);
    await delay(250);
  }
  assert.ok(ready, 'Le serveur de test ne répond pas.');
  browser = await chromium.launch({
    headless: true,
    ...(process.env.NOOSPHERE_TEST_BROWSER
      ? { executablePath: process.env.NOOSPHERE_TEST_BROWSER }
      : {}),
  });
  const page = await browser.newPage({
    viewport: { width: 1440, height: 1000 },
  });
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(`${base}/tests/noosnote-ui.html`);
  await page
    .getByRole('button', { name: 'Ouvrir Anglais', exact: true })
    .waitFor();
  assert.equal(
    await page.locator('.note-group-heading').first().innerText(),
    'Cours\n2',
  );
  await page.screenshot({
    path: path.join(output, 'school.png'),
    fullPage: true,
  });
  await page
    .locator('.note-table th')
    .getByRole('button', { name: 'Catégorie', exact: true })
    .click();
  await page
    .getByRole('dialog')
    .getByRole('combobox', { name: 'Type', exact: true })
    .selectOption('multiSelect');
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Enregistrer', exact: true })
    .click();
  await page.locator('.note-cell-picker').first().click();
  const choices = page.getByRole('dialog', { name: 'Catégorie', exact: true });
  await choices.getByRole('checkbox', { name: 'Cours', exact: true }).waitFor();
  assert.equal(
    await choices
      .getByRole('checkbox', { name: 'Cours', exact: true })
      .isChecked(),
    true,
  );
  assert.equal(
    await page.locator('.note-table-scroll .note-cell-options').count(),
    0,
  );
  await page.keyboard.press('Escape');
  await choices.waitFor({ state: 'hidden' });
  await page
    .locator('.note-table th')
    .getByRole('button', { name: 'Catégorie', exact: true })
    .click();
  await page
    .getByRole('dialog')
    .getByRole('combobox', { name: 'Type', exact: true })
    .selectOption('select');
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Enregistrer', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Ouvrir Anglais', exact: true })
    .click();
  await page
    .getByRole('textbox', { name: 'Titre de la page', exact: true })
    .waitFor();
  await page.waitForFunction(
    () => document.querySelector('.note-page-title')?.value === 'Anglais',
  );
  const editor = page.locator('.note-prose[contenteditable=true]');
  await editor.waitFor();
  assert.equal((await editor.innerText()).trim(), '');
  await editor.fill('Notes d’anglais');
  await page
    .getByRole('button', { name: 'Insérer un bloc', exact: true })
    .click();
  await page
    .locator('.note-insert-menu')
    .getByRole('button', { name: /Table/ })
    .click();
  await page
    .getByRole('textbox', { name: 'Nom de la table', exact: true })
    .fill('Journées de cours');
  await page
    .locator('.note-table th')
    .getByRole('button', { name: 'Catégorie', exact: true })
    .click();
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Ajouter une catégorie', exact: true })
    .click();
  await page
    .getByRole('textbox', { name: 'Nom de la catégorie', exact: true })
    .fill('Cours');
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Enregistrer', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Ajouter une propriété', exact: true })
    .click();
  const dialog = page.getByRole('dialog');
  await dialog.getByRole('textbox', { name: 'Nom', exact: true }).fill('Jour');
  await dialog
    .getByRole('combobox', { name: 'Type', exact: true })
    .selectOption('date');
  await dialog
    .getByRole('button', { name: 'Enregistrer', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Nouvel élément', exact: true })
    .first()
    .click();
  await page.waitForFunction(
    () => document.querySelector('.note-page-title')?.value === 'Sans titre',
  );
  await page
    .getByRole('textbox', { name: 'Titre de la page', exact: true })
    .fill('Lundi 12 octobre');
  await editor.fill('Present perfect\nI have learned something new.');
  await page.screenshot({
    path: path.join(output, 'lesson.png'),
    fullPage: true,
  });
  await page
    .getByRole('navigation', { name: 'Fil d’Ariane' })
    .getByRole('button', { name: 'Anglais', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Ouvrir Lundi 12 octobre', exact: true })
    .waitFor();
  const today = new Date();
  const lessonDate = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
  await page.getByLabel('Jour', { exact: true }).fill(lessonDate);
  await page
    .getByRole('button', { name: 'Ajouter une vue', exact: true })
    .click();
  await dialog
    .getByRole('textbox', { name: 'Nom', exact: true })
    .fill('Calendrier');
  await dialog
    .getByRole('combobox', { name: 'Présentation', exact: true })
    .selectOption('calendar');
  await dialog
    .getByRole('combobox', { name: 'Date du calendrier', exact: true })
    .selectOption({ label: 'Jour' });
  await dialog
    .getByRole('button', { name: 'Enregistrer', exact: true })
    .click();
  await page
    .locator('.note-view-tabs')
    .getByRole('button', { name: 'Calendrier', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Lundi 12 octobre', exact: true })
    .waitFor();
  await page.screenshot({
    path: path.join(output, 'calendar.png'),
    fullPage: true,
  });
  await page
    .getByRole('button', { name: 'Créer un Noosnote', exact: true })
    .click();
  await dialog
    .getByRole('textbox', { name: 'Nom', exact: true })
    .fill('Projets');
  await dialog
    .getByRole('button', { name: 'Créer le Noosnote', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Noosnote Projets', exact: true })
    .waitFor();
  await page.waitForFunction(
    () => document.querySelector('.note-page-title')?.value === 'Bienvenue',
  );
  assert.equal(await page.locator('.note-rail-icon').count(), 2);
  await page
    .getByRole('button', { name: 'Noosnote École', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Ouvrir Anglais', exact: true })
    .click();
  await page
    .locator('.note-view-tabs')
    .getByRole('button', { name: 'Table', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Ouvrir Lundi 12 octobre', exact: true })
    .click();
  await editor.waitFor();
  assert.match(await editor.innerText(), /Present perfect/);
  assert.deepEqual(errors, []);
  await writeFile(
    path.join(output, 'validation.json'),
    JSON.stringify(
      {
        schoolTemplate: true,
        nestedDatabase: true,
        lessonText: true,
        editableCategories: true,
        multiSelectOverlay: true,
        dateProperty: true,
        calendar: true,
        multipleWorkspaces: true,
        pageReopen: true,
        javascriptErrors: errors,
      },
      null,
      2,
    ),
  );
  console.log(
    'Noosnote UI: pages, tables imbriquées, catégories, calendrier et navigation validés.',
  );
} finally {
  await browser?.close();
  server?.kill();
  clearInterval(lifetime);
}
