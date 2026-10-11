import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import ts from 'typescript';
import {
  applyFiles,
  breadcrumbs,
  change,
  createDatabase,
  createPage,
  createRow,
  createWorkspace,
  databasePath,
  duplicatePage,
  getDatabase,
  getPage,
  getWorkspace,
  id,
  iconFields,
  movePage,
  noteIcon,
  normalizeColumnValues,
  pagePath,
  renamePage,
  restorePage,
  searchPages,
  trashPage,
  workspacePath,
} from '../lib/noosnote/model.ts';

const source = (
  await readFile(new URL('../lib/noosnote/data.ts', import.meta.url), 'utf8')
).replace(
  "from './model'",
  `from '${new URL('../lib/noosnote/model.ts', import.meta.url).href}'`,
);
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    target: ts.ScriptTarget.ES2021,
    module: ts.ModuleKind.ESNext,
  },
}).outputText;
const { createCellReader, evaluateFormula, queryRows, rowGroups } =
  await import(
    `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`
  );

function school() {
  const created = createWorkspace('École', '📚', '#78b89b', 'school');
  const files = applyFiles({}, created.changes);
  const workspace = getWorkspace(files, created.workspaceId);
  const database = Object.values(files).find(
    (value) => value.kind === 'database',
  );
  return { ...created, files, workspace, database };
}

void test('les notes de la 0.2.0 restent éditables sans changer leur format', async () => {
  const files = JSON.parse(
    await readFile(
      new URL('./fixtures/noosnote-school.json', import.meta.url),
      'utf8',
    ),
  );
  const workspace = Object.values(files).find(
    (value) => value.kind === 'workspace',
  );
  const page = Object.values(files).find((value) => value.kind === 'page');
  const extra = { applicationVersion: '0.2.0', preferences: { folded: true } };
  files[pagePath(workspace.id, page.id)] = { ...page, ...extra };
  const updated = applyFiles(
    files,
    renamePage(files, workspace.id, page.id, 'Cours mis à jour', '🦦'),
  );
  assert.equal(getPage(updated, workspace.id, page.id).version, 1);
  assert.equal(
    getPage(updated, workspace.id, page.id).applicationVersion,
    extra.applicationVersion,
  );
  assert.deepEqual(
    getPage(updated, workspace.id, page.id).preferences,
    extra.preferences,
  );
  assert.deepEqual(
    getPage(updated, workspace.id, page.id).content,
    page.content,
  );
  assert.ok(Object.values(updated).every((document) => document.version === 1));
});

void test('les emojis longs conservent une icône compatible avec la 0.2.0', () => {
  const emoji = '👩🏽‍❤️‍💋‍👩🏾';
  assert.ok(Buffer.byteLength(emoji) > 32);
  const created = createWorkspace('Voyages', emoji, '#123abc');
  let files = applyFiles({}, created.changes);
  let workspace = getWorkspace(files, created.workspaceId);
  assert.equal(noteIcon(workspace), emoji);
  assert.ok(Buffer.byteLength(workspace.icon) <= 32);
  files = applyFiles(
    files,
    renamePage(files, workspace.id, created.pageId, 'Journal', emoji),
  );
  workspace = getWorkspace(files, workspace.id);
  const page = getPage(files, workspace.id, created.pageId);
  assert.equal(noteIcon(page), emoji);
  assert.equal(noteIcon(workspace.pages[0]), emoji);
  assert.ok(Buffer.byteLength(page.icon) <= 32);
  files = applyFiles(
    files,
    renamePage(files, workspace.id, page.id, page.title, '🦦'),
  );
  assert.equal(noteIcon(getPage(files, workspace.id, page.id)), '🦦');
  assert.deepEqual(iconFields(''), { icon: '', emoji: undefined });
});

void test('École contient une catégorie Cours et deux pages vierges indépendantes', () => {
  const { files, workspace, database, pageId } = school();
  assert.deepEqual(
    database.rows.map((row) => getPage(files, workspace.id, row.pageId).title),
    ['Anglais', 'Français'],
  );
  assert.equal(workspace.pages.find((page) => page.id === pageId).icon, '📚');
  const groups = rowGroups(files, database, database.views[0], database.rows);
  assert.equal(groups[0].name, 'Cours');
  assert.equal(groups[0].rows.length, 2);
  for (const row of database.rows) {
    const page = getPage(files, workspace.id, row.pageId);
    assert.equal(page.content.content[0].type, 'paragraph');
    assert.equal(page.content.content[0].content, undefined);
    assert.deepEqual(
      breadcrumbs(workspace, row.pageId).map((page) => page.title),
      ['École', page.title],
    );
  }
});

void test('une table Journées peut être imbriquée dans Anglais avec une page par cours', () => {
  const { workspace, database, files: initialFiles } = school();
  let files = initialFiles;
  const anglais = database.rows[0].pageId;
  const nested = createDatabase(
    files,
    workspace.id,
    anglais,
    'Journées de cours',
  );
  files = applyFiles(files, nested.changes);
  const row = createRow(files, nested.database, 'Lundi 12 octobre');
  files = applyFiles(files, row.changes);
  const text = {
    type: 'doc',
    content: [
      {
        type: 'paragraph',
        attrs: { id: id() },
        content: [{ type: 'text', text: 'Present perfect' }],
      },
    ],
  };
  files = applyFiles(files, [change({ ...row.page, content: text })]);
  const updated = getWorkspace(files, workspace.id);
  assert.deepEqual(
    breadcrumbs(updated, row.page.id).map((page) => page.title),
    ['École', 'Anglais', 'Lundi 12 octobre'],
  );
  assert.equal(
    getPage(files, workspace.id, row.page.id).content.content[0].content[0]
      .text,
    'Present perfect',
  );
  assert.equal(
    getDatabase(files, workspace.id, nested.database.id).rows[0].pageId,
    row.page.id,
  );
});

void test('renommer une page conserve les chemins et les références', () => {
  const { files, workspace, database } = school();
  const pageId = database.rows[0].pageId;
  const renamed = applyFiles(
    files,
    renamePage(files, workspace.id, pageId, 'English 🇬🇧'),
  );
  assert.deepEqual(Object.keys(renamed).sort(), Object.keys(files).sort());
  assert.equal(
    getWorkspace(renamed, workspace.id).pages.find((page) => page.id === pageId)
      .title,
    'English 🇬🇧',
  );
  assert.equal(getPage(renamed, workspace.id, pageId).title, 'English 🇬🇧');
  assert.equal(
    getDatabase(renamed, workspace.id, database.id).rows[0].pageId,
    pageId,
  );
});

void test('la duplication réécrit pages, tables et blocs sans partager le contenu', () => {
  const { files, workspace, pageId, database } = school();
  const copy = duplicatePage(files, workspace.id, pageId);
  const copied = applyFiles(files, copy.changes);
  const page = getPage(copied, workspace.id, copy.pageId);
  const newDatabase = getDatabase(
    copied,
    workspace.id,
    page.content.content[0].attrs.databaseId,
  );
  assert.notEqual(newDatabase.id, database.id);
  assert.equal(newDatabase.pageId, copy.pageId);
  assert.equal(newDatabase.rows.length, 2);
  for (const row of newDatabase.rows)
    assert.ok(getPage(copied, workspace.id, row.pageId));
  assert.equal(getWorkspace(copied, workspace.id).pages.length, 6);
  const lazy = { ...files };
  delete lazy[pagePath(workspace.id, database.rows[0].pageId)];
  assert.throws(() => duplicatePage(lazy, workspace.id, pageId), /sous-pages/);
});

void test('corbeille et restauration conservent les cours et empêchent les cycles', () => {
  const { files, workspace, pageId, database } = school();
  assert.throws(
    () => movePage(workspace, pageId, database.rows[0].pageId),
    /sous-pages/,
  );
  const trashed = applyFiles(files, trashPage(workspace, pageId));
  const trash = getWorkspace(trashed, workspace.id);
  assert.ok(trash.pages.every((page) => page.trashedAt));
  assert.equal(queryRows(trashed, database, database.views[0]).length, 0);
  const restored = getWorkspace(
    applyFiles(trashed, restorePage(trash, pageId)),
    workspace.id,
  );
  assert.ok(restored.pages.every((page) => !page.trashedAt));
  assert.equal(Object.keys(trashed).length, Object.keys(files).length);
});

void test('recherche, filtres et tri utilisent les noms des catégories', () => {
  const { files, workspace, database } = school();
  const view = {
    ...database.views[0],
    filters: [
      {
        id: id(),
        propertyId: database.columns[1].id,
        operator: 'equals',
        value: 'Cours',
      },
    ],
    sorts: [{ propertyId: database.columns[0].id, direction: 'desc' }],
  };
  assert.deepEqual(
    queryRows(files, database, view).map((row) => row.pageId),
    [...database.rows].reverse().map((row) => row.pageId),
  );
  assert.equal(queryRows(files, database, view, 'anglais').length, 1);
  assert.equal(searchPages(files, workspace, 'ecole')[0].title, 'École');
});

void test('les formules calculent les propriétés sans exécuter du JavaScript', () => {
  assert.equal(
    evaluateFormula(
      'if(prop("Terminé"), round(prop("Note") * 2, 1), "À faire")',
      (name) => ({ Terminé: true, Note: 8.25 })[name],
    ),
    16.5,
  );
  assert.equal(
    evaluateFormula(
      'concat(upper("cours"), " ", toString(3 + 2 * 4))',
      () => null,
    ),
    'COURS 11',
  );
  assert.equal(
    evaluateFormula('if(true, 42, 1 / 0)', () => null),
    42,
  );
  assert.throws(() => evaluateFormula('1 / 0', () => null));
  assert.throws(() => evaluateFormula('globalThis.process.exit()', () => null));
  assert.throws(() =>
    evaluateFormula('"x".constructor("return process")()', () => null),
  );
  assert.throws(() =>
    evaluateFormula('('.repeat(80) + '1' + ')'.repeat(80), () => null),
  );
});

void test('relations et agrégations restent stables après un changement de titre', () => {
  const {
    workspace,
    files: initialFiles,
    database: initialDatabase,
  } = school();
  let files = initialFiles;
  let database = initialDatabase;
  const number = { id: id(), name: 'Note', type: 'number' };
  const relation = {
    id: id(),
    name: 'Autres cours',
    type: 'relation',
    databaseId: database.id,
  };
  const rollup = {
    id: id(),
    name: 'Moyenne',
    type: 'rollup',
    relationId: relation.id,
    propertyId: number.id,
    aggregate: 'average',
  };
  const formula = {
    id: id(),
    name: 'Double',
    type: 'formula',
    expression: 'prop("Note") * 2',
  };
  database = {
    ...database,
    columns: [...database.columns, number, relation, rollup, formula],
    rows: database.rows.map((row, index) => ({
      ...row,
      values: {
        ...row.values,
        [number.id]: index ? 18 : 14,
        [relation.id]: database.rows.map((row) => row.pageId),
      },
    })),
  };
  files = applyFiles(files, [
    change(database),
    ...renamePage(files, workspace.id, database.rows[1].pageId, 'Littérature'),
  ]);
  const reader = createCellReader(files);
  assert.equal(reader.read(database, database.rows[0], rollup).value, 16);
  assert.equal(reader.read(database, database.rows[0], formula).value, 28);
  assert.deepEqual(
    reader.label(
      relation,
      reader.read(database, database.rows[0], relation).value,
    ),
    ['Anglais', 'Littérature'],
  );
  const circular = { ...formula, expression: 'prop("Double")' };
  database = {
    ...database,
    columns: database.columns.map((column) =>
      column.id === formula.id ? circular : column,
    ),
  };
  files = applyFiles(files, [change(database)]);
  assert.match(
    createCellReader(files).read(database, database.rows[0], circular).error,
    /circulaire/,
  );
});

void test('chaque Noosnote garde son dossier et ses identifiants propres', () => {
  const first = school();
  const second = createWorkspace('Projets');
  const files = applyFiles(first.files, second.changes);
  assert.ok(files[workspacePath(first.workspaceId)]);
  assert.ok(files[workspacePath(second.workspaceId)]);
  assert.ok(files[databasePath(first.workspaceId, first.database.id)]);
  const page = createPage(files, second.workspaceId, second.pageId, 'Plan');
  assert.equal(page.page.workspaceId, second.workspaceId);
  assert.throws(
    () => createPage(files, second.workspaceId, first.pageId),
    /parente/,
  );
});

void test('les conversions compatibles conservent nombres et catégories', () => {
  const { database } = school();
  const column = database.columns[1];
  let converted = normalizeColumnValues(database, {
    ...column,
    type: 'multiSelect',
  });
  assert.deepEqual(converted.rows[0].values[column.id], [column.options[0].id]);
  converted = normalizeColumnValues(converted, { ...column, type: 'select' });
  assert.equal(converted.rows[0].values[column.id], column.options[0].id);
  converted = normalizeColumnValues(converted, {
    ...column,
    type: 'multiSelect',
    options: [],
  });
  assert.deepEqual(converted.rows[0].values[column.id], []);
  const number = { id: id(), name: 'Note', type: 'number' };
  const graded = {
    ...database,
    columns: [...database.columns, number],
    rows: database.rows.map((row) => ({
      ...row,
      values: { ...row.values, [number.id]: 18 },
    })),
  };
  converted = normalizeColumnValues(graded, { ...number, type: 'text' });
  assert.equal(converted.rows[0].values[number.id], '18');
});
