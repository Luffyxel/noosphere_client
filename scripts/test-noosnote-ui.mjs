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
  for (let i = 0; i < 240; i += 1) {
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
  assert.ok(ready, `Le serveur de test ne répond pas.\n${diagnostics}`);
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
    .getByRole('button', { name: 'Icône du Noosnote', exact: true })
    .click();
  const emojiPicker = page.getByRole('dialog', {
    name: 'Icône du Noosnote',
    exact: true,
  });
  await emojiPicker
    .getByRole('textbox', { name: 'Rechercher un emoji', exact: true })
    .fill('loutre');
  await emojiPicker
    .getByRole('button', { name: 'loutre', exact: true })
    .click();
  assert.ok(
    await dialog
      .getByRole('button', { name: 'Icône du Noosnote', exact: true })
      .innerText()
      .then((text) => text.includes('🦦')),
  );
  assert.ok(
    (await dialog
      .getByRole('group', { name: 'Palette de couleurs' })
      .getByRole('button')
      .count()) >= 30,
  );
  await dialog
    .getByRole('textbox', { name: 'Code couleur hexadécimal', exact: true })
    .fill('#123abc');
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
  const projectRail = page.getByRole('button', {
    name: 'Noosnote Projets',
    exact: true,
  });
  assert.match(await projectRail.innerText(), /🦦/);
  assert.equal(
    await projectRail.evaluate((element) => element.style.borderColor),
    'rgb(18, 58, 188)',
  );
  await page
    .getByRole('button', { name: 'Réglages du Noosnote', exact: true })
    .click();
  const settings = page.getByRole('dialog', {
    name: 'Personnaliser le Noosnote',
    exact: true,
  });
  await settings
    .getByRole('button', { name: 'Icône du Noosnote', exact: true })
    .click();
  await emojiPicker
    .getByRole('textbox', { name: 'Emoji du système', exact: true })
    .fill('👩🏽‍❤️‍💋‍👩🏾');
  await settings
    .getByRole('button', { name: 'Enregistrer', exact: true })
    .click();
  assert.match(await projectRail.innerText(), /👩🏽‍❤️‍💋‍👩🏾/);
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
  const french = page
    .locator('.note-tree-page')
    .filter({ hasText: /Français$/ });
  await french.click({ button: 'right' });
  const menu = page.getByRole('menu');
  await menu.getByRole('menuitem', { name: 'Renommer', exact: true }).click();
  const rename = page.getByRole('dialog', {
    name: 'Personnaliser la page',
    exact: true,
  });
  await rename
    .getByRole('textbox', { name: 'Nom de la page', exact: true })
    .fill('Français révisé');
  await rename
    .getByRole('button', { name: 'Enregistrer', exact: true })
    .click();
  assert.equal(
    await page.locator('.note-page-title').inputValue(),
    'Lundi 12 octobre',
  );
  const renamed = page
    .locator('.note-tree-page')
    .filter({ hasText: /Français révisé$/ });
  await renamed.click({ button: 'right' });
  await menu
    .getByRole('menuitem', { name: 'Mettre à la corbeille', exact: true })
    .click();
  await renamed.waitFor({ state: 'hidden' });
  assert.equal(
    await page.locator('.note-page-title').inputValue(),
    'Lundi 12 octobre',
  );
  await page.getByRole('button', { name: /^Corbeille/ }).click();
  await page
    .locator('.note-trash')
    .getByRole('button', { name: 'Restaurer', exact: true })
    .click();
  await renamed.waitFor();
  await renamed.click({ button: 'right' });
  await menu
    .getByRole('menuitem', {
      name: 'Dupliquer avec les sous-pages',
      exact: true,
    })
    .click();
  await page.waitForFunction(
    () =>
      document.querySelector('.note-page-title')?.value ===
      'Français révisé — copie',
  );
  const copy = page
    .locator('.note-tree-page')
    .filter({ hasText: /Français révisé — copie$/ });
  await copy.click({ button: 'right' });
  assert.equal(
    await menu
      .getByRole('menuitem', { name: 'Déplacer', exact: true })
      .isEnabled(),
    false,
  );
  await menu
    .getByRole('menuitem', { name: 'Mettre à la corbeille', exact: true })
    .click();
  await copy.waitFor({ state: 'hidden' });
  await page
    .getByRole('button', { name: 'Nouvelle page', exact: true })
    .click();
  await page
    .getByRole('textbox', { name: 'Titre de la page', exact: true })
    .fill('À déplacer');
  const movable = page
    .locator('.note-tree-page')
    .filter({ hasText: /À déplacer$/ });
  await movable.click({ button: 'right' });
  await menu.getByRole('menuitem', { name: 'Déplacer', exact: true }).click();
  await page
    .getByRole('dialog', { name: 'Déplacer la page', exact: true })
    .getByRole('button', { name: 'Anglais', exact: false })
    .click();
  assert.match(
    await page.getByRole('navigation', { name: 'Fil d’Ariane' }).innerText(),
    /Anglais/,
  );
  await movable.click({ button: 'right' });
  await page.screenshot({
    path: path.join(output, 'context-menu.png'),
    fullPage: true,
  });
  await page.keyboard.press('Escape');
  await menu.waitFor({ state: 'hidden' });
  await movable.focus();
  await page.keyboard.press('Shift+F10');
  await menu
    .getByRole('menuitem', { name: 'Ajouter une sous-page', exact: true })
    .waitFor();
  await page.keyboard.press('Escape');
  await page
    .getByRole('navigation', { name: 'Fil d’Ariane' })
    .getByRole('button', { name: 'École', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Ouvrir Français révisé', exact: true })
    .click({ button: 'right' });
  await menu
    .getByRole('menuitem', { name: 'Changer l’icône', exact: true })
    .click();
  await rename
    .getByRole('button', { name: 'Icône de la page', exact: true })
    .click();
  const pageEmoji = page.getByRole('dialog', {
    name: 'Icône de la page',
    exact: true,
  });
  await pageEmoji
    .getByRole('textbox', { name: 'Rechercher un emoji', exact: true })
    .fill('pouce vers le haut');
  await pageEmoji
    .getByLabel('Teinte de peau', { exact: true })
    .selectOption('2');
  await pageEmoji
    .getByRole('button', {
      name: 'pouce vers le haut : peau légèrement mate',
      exact: true,
    })
    .click();
  await rename
    .getByRole('button', { name: 'Enregistrer', exact: true })
    .click();
  assert.match(await renamed.innerText(), /👍🏽/);
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
        contextMenu: true,
        inactivePageRenameAndTrash: true,
        restorePage: true,
        duplicateAndMove: true,
        systemEmojis: true,
        customColor: true,
        keyboardContextMenu: true,
        databaseContextMenu: true,
        emojiSkinTone: true,
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
