import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { chromium } from 'playwright';
import {
  Document,
  HeadingLevel,
  Packer,
  Paragraph,
  Table,
  TableCell,
  TableRow,
  TextRun,
} from 'docx';
import mammoth from 'mammoth';
import { build, preview } from 'vite';

const root = path.resolve(import.meta.dirname, '..');
const output = path.join(root, 'remote-test-results/noosnote-files');
await mkdir(output, { recursive: true });
const original = await Packer.toBuffer(
  new Document({
    sections: [
      {
        children: [
          new Paragraph({
            text: 'Cours de français',
            heading: HeadingLevel.HEADING_1,
          }),
          new Paragraph({
            children: [new TextRun({ text: 'Un texte important', bold: true })],
          }),
          new Paragraph({
            text: 'Réviser le vocabulaire',
            bullet: { level: 0 },
          }),
          new Paragraph({
            text: 'Pratiquer chaque jour',
            bullet: { level: 1 },
          }),
          new Table({
            rows: [
              'Discipline|Durée',
              'Anglais|45 minutes',
              'Français|1 heure',
            ].map(
              (row) =>
                new TableRow({
                  children: row
                    .split('|')
                    .map(
                      (text) =>
                        new TableCell({ children: [new Paragraph(text)] }),
                    ),
                }),
            ),
          }),
          new Table({
            rows: [
              new TableRow({
                children: [
                  new TableCell({
                    columnSpan: 3,
                    children: [new Paragraph('Programme du jour')],
                  }),
                ],
              }),
              new TableRow({
                children: ['Anglais', '45 minutes', 'Salle 12'].map(
                  (text) => new TableCell({ children: [new Paragraph(text)] }),
                ),
              }),
            ],
          }),
          new Paragraph('Fin du document'),
        ],
      },
    ],
  }),
);
function pdfBytes() {
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
  ];
  for (const [index, title] of [
    'Cours de francais',
    'Deuxieme page',
  ].entries()) {
    const stream = `BT /F1 24 Tf 50 730 Td (${title}) Tj ET`;
    if (index)
      objects.push(
        '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 7 0 R >>',
      );
    objects.push(
      `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`,
    );
  }
  let pdf = '%PDF-1.4\n';
  const offsets = [0];
  objects.forEach((object, index) => {
    offsets.push(Buffer.byteLength(pdf));
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`;
  });
  const crossReference = Buffer.byteLength(pdf);
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n${offsets
    .slice(1)
    .map((offset) => `${String(offset).padStart(10, '0')} 00000 n \n`)
    .join(
      '',
    )}trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${crossReference}\n%%EOF\n`;
  return Buffer.from(pdf);
}
const pdf = pdfBytes();
const port = 5516;
const base = process.env.NOOSPHERE_TEST_URL ?? `http://127.0.0.1:${port}`;
let server;
if (!process.env.NOOSPHERE_TEST_URL) {
  const outDir = path.join(output, 'production');
  const configFile = path.join(root, 'vite.tauri.config.ts');
  await build({
    configFile,
    build: {
      outDir,
      rolldownOptions: { input: path.join(root, 'tests/noosnote-ui.html') },
    },
  });
  const csp = JSON.parse(
    await readFile(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'),
  ).app.security.csp;
  server = await preview({
    configFile,
    build: { outDir },
    preview: {
      port,
      host: '127.0.0.1',
      strictPort: true,
      headers: { 'Content-Security-Policy': csp },
    },
  });
}
const diagnostics = '';
let browser;
let page;
try {
  let ready = false;
  for (let attempt = 0; attempt < 240; attempt += 1) {
    try {
      if ((await fetch(`${base}/tests/noosnote-ui.html`)).ok) {
        ready = true;
        break;
      }
    } catch {}
    await delay(250);
  }
  assert.ok(ready, diagnostics);
  browser = await chromium.launch({
    headless: true,
    ...(process.env.NOOSPHERE_TEST_BROWSER
      ? { executablePath: process.env.NOOSPHERE_TEST_BROWSER }
      : {}),
  });
  page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(`${base}/tests/noosnote-ui.html`, {
    waitUntil: 'domcontentloaded',
    timeout: 90000,
  });
  await page
    .getByRole('button', { name: 'Ouvrir Anglais', exact: true })
    .click();
  await page
    .getByRole('textbox', { name: 'Titre de la page', exact: true })
    .waitFor();
  await page
    .getByRole('button', { name: 'Insérer un bloc', exact: true })
    .click();
  const chooser = page.waitForEvent('filechooser');
  await page
    .locator('.note-insert-menu')
    .getByRole('button', { name: /Fichier/ })
    .click();
  await (
    await chooser
  ).setFiles({
    name: 'Mon cours.docx',
    mimeType:
      'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    buffer: original,
  });
  await page
    .getByRole('button', { name: 'Modifier Mon cours.docx', exact: true })
    .waitFor({ timeout: 45000 });
  await page
    .getByRole('button', { name: 'Modifier Mon cours.docx', exact: true })
    .click();
  const editor = page.locator('.note-prose[contenteditable=true]');
  await editor.getByRole('heading', { name: 'Cours de français' }).waitFor();
  assert.equal(
    await editor.locator('strong').first().innerText(),
    'Un texte important',
  );
  await page.locator('.note-table').first().waitFor();
  await editor.locator('p').last().click();
  await page.keyboard.press('Control+End');
  await page.keyboard.press('Enter');
  await page.keyboard.type('Une modification enregistrée.');
  await page.waitForTimeout(700);
  const exportedEvent = page.waitForEvent('download');
  await page
    .getByRole('button', { name: 'Exporter en Word', exact: true })
    .click();
  const exported = await exportedEvent;
  const exportedPath = path.join(output, exported.suggestedFilename());
  await exported.saveAs(exportedPath);
  const converted = await mammoth.convertToHtml(
    { buffer: await readFile(exportedPath) },
    { includeEmbeddedStyleMap: false },
  );
  assert.match(converted.value, /Une modification enregistrée/);
  assert.match(converted.value, /<table>/);
  assert.match(converted.value, /45 minutes/);
  assert.match(converted.value, /Salle 12/);
  assert.match(converted.value, /Réviser le vocabulaire/);
  assert.match(converted.value, /Pratiquer chaque jour/);
  const originalEvent = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Original', exact: true }).click();
  const sourceDownload = await originalEvent;
  const sourcePath = path.join(output, 'original.docx');
  await sourceDownload.saveAs(sourcePath);
  assert.deepEqual(await readFile(sourcePath), original);
  await page.screenshot({
    path: path.join(output, 'word-editor.png'),
    fullPage: false,
  });
  await page
    .locator('.note-topbar')
    .getByRole('button', { name: 'Anglais', exact: true })
    .click();
  await page.waitForFunction(
    () => document.querySelector('.note-page-title')?.value === 'Anglais',
  );
  await page.evaluate(
    ({ name, data }) => {
      const transfer = new DataTransfer();
      transfer.items.add(
        new File(
          [Uint8Array.from(atob(data), (character) => character.charCodeAt(0))],
          name,
          { type: 'application/pdf' },
        ),
      );
      document.querySelector('.note-editor').dispatchEvent(
        new DragEvent('drop', {
          bubbles: true,
          cancelable: true,
          dataTransfer: transfer,
        }),
      );
    },
    { name: 'Le cours.pdf', data: pdf.toString('base64') },
  );
  const card = page.locator('.note-file-card').filter({
    has: page.getByRole('button', {
      name: 'Ouvrir Le cours.pdf',
      exact: true,
    }),
  });
  await card.waitFor();
  await card.getByRole('button', { name: 'Aperçu', exact: true }).click();
  await page
    .locator('.note-pdf-reader .textLayer')
    .getByText('Cours de francais', { exact: true })
    .waitFor({ timeout: 30000 });
  const width = await page
    .locator('.note-pdf-reader canvas')
    .evaluate((canvas) => canvas.width);
  await page
    .getByRole('button', { name: 'Agrandir le PDF', exact: true })
    .click();
  await page.waitForFunction(
    (previous) =>
      document.querySelector('.note-pdf-reader canvas').width > previous,
    width,
  );
  await page
    .getByRole('button', { name: 'Page suivante', exact: true })
    .click();
  await page
    .locator('.note-pdf-reader .textLayer')
    .getByText('Deuxieme page', { exact: true })
    .waitFor();
  await page.screenshot({
    path: path.join(output, 'pdf-preview.png'),
    fullPage: false,
  });
  const pdfEvent = page.waitForEvent('download');
  await page
    .getByRole('button', { name: 'Télécharger le PDF', exact: true })
    .click();
  const pdfDownload = await pdfEvent;
  const pdfPath = path.join(output, 'download.pdf');
  await pdfDownload.saveAs(pdfPath);
  assert.deepEqual(await readFile(pdfPath), pdf);
  assert.deepEqual(errors, []);
  await writeFile(
    path.join(output, 'result.json'),
    JSON.stringify(
      {
        wordImport: true,
        wordEditing: true,
        wordExport: true,
        originalPreserved: true,
        pdfDrop: true,
        pdfPreview: true,
        pdfPagination: true,
        pdfZoom: true,
        pdfDownload: true,
      },
      null,
      2,
    ),
  );
  console.log('Noosnote file import, editing, export and PDF preview passed.');
} catch (error) {
  console.error(diagnostics);
  if (page) {
    console.error((await page.locator('body').innerText()).slice(-3000));
    await page.screenshot({ path: path.join(output, 'failure.png') });
  }
  throw error;
} finally {
  await browser?.close();
  if (server) await new Promise((resolve) => server.httpServer.close(resolve));
}
