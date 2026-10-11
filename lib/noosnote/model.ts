import type {
  Column,
  DatabaseRow,
  DatabaseView,
  NoteChange,
  NoteDatabase,
  NoteDocument,
  NoteFiles,
  NotePage,
  PageEntry,
  RichNode,
  Workspace,
} from './types';

export const NOTE_COLORS = [
  '#78b89b',
  '#87a9da',
  '#bb9cd9',
  '#d9ac7e',
  '#dc939e',
  '#a7b5bb',
  '#ff453a',
  '#ff9f0a',
  '#ffd60a',
  '#32d74b',
  '#63e6be',
  '#64d2ff',
  '#0a84ff',
  '#5e5ce6',
  '#bf5af2',
  '#ff375f',
  '#f783ac',
  '#e599f7',
  '#b197fc',
  '#91a7ff',
  '#74c0fc',
  '#66d9e8',
  '#8ce99a',
  '#c0eb75',
  '#ffe066',
  '#ffc078',
  '#ffa8a8',
  '#e8c9a3',
  '#a68a64',
  '#868e96',
  '#ced4da',
  '#ffffff',
  '#343a40',
  '#000000',
];
export const id = () => crypto.randomUUID();
export const noteIcon = (value?: { icon: string; emoji?: string }) =>
  value?.emoji ?? value?.icon ?? '';
export function iconFields(emoji: string) {
  if (new TextEncoder().encode(emoji).length > 32) return { icon: '📄', emoji };
  return { icon: emoji, emoji: undefined };
}
export const workspacePath = (workspaceId: string) =>
  `notes/${workspaceId}/workspace.json`;
export const pagePath = (workspaceId: string, pageId: string) =>
  `notes/${workspaceId}/pages/${pageId}.json`;
export const databasePath = (workspaceId: string, databaseId: string) =>
  `notes/${workspaceId}/databases/${databaseId}.json`;
export const emptyContent = (): RichNode => ({
  type: 'doc',
  content: [{ type: 'paragraph', attrs: { id: id() } }],
});

export function workspaces(files: NoteFiles): Workspace[] {
  return Object.values(files).filter(
    (document): document is Workspace => document.kind === 'workspace',
  );
}
export function getWorkspace(
  files: NoteFiles,
  workspaceId: string,
): Workspace | undefined {
  const value = files[workspacePath(workspaceId)];
  return value?.kind === 'workspace' ? value : undefined;
}
export function getPage(
  files: NoteFiles,
  workspaceId: string,
  pageId: string,
): NotePage | undefined {
  const value = files[pagePath(workspaceId, pageId)];
  return value?.kind === 'page' ? value : undefined;
}
export function getDatabase(
  files: NoteFiles,
  workspaceId: string,
  databaseId: string,
): NoteDatabase | undefined {
  const value = files[databasePath(workspaceId, databaseId)];
  return value?.kind === 'database' ? value : undefined;
}
export function databases(
  files: NoteFiles,
  workspaceId?: string,
): NoteDatabase[] {
  return Object.values(files).filter(
    (value): value is NoteDatabase =>
      value.kind === 'database' &&
      (!workspaceId || value.workspaceId === workspaceId),
  );
}
export function applyFiles(files: NoteFiles, changes: NoteChange[]): NoteFiles {
  const result = { ...files };
  for (const change of changes) {
    if (change.value) result[change.path] = change.value;
    else delete result[change.path];
  }
  return result;
}
export function change(value: NoteDocument): NoteChange {
  return {
    path:
      value.kind === 'workspace'
        ? workspacePath(value.id)
        : value.kind === 'page'
          ? pagePath(value.workspaceId, value.id)
          : databasePath(value.workspaceId, value.id),
    value,
  };
}
export function createPage(
  files: NoteFiles,
  workspaceId: string,
  parentId: string | null,
  title = 'Sans titre',
  databaseId?: string,
) {
  const workspace = getWorkspace(files, workspaceId);
  if (!workspace) throw new Error('Noosnote introuvable.');
  if (
    parentId &&
    !workspace.pages.some((page) => page.id === parentId && !page.trashedAt)
  )
    throw new Error('Page parente introuvable.');
  if (workspace.pages.length >= 4096)
    throw new Error('Ce Noosnote contient déjà 4 096 pages.');
  const page: NotePage = {
    version: 1,
    kind: 'page',
    id: id(),
    workspaceId,
    title,
    icon: '',
    content: emptyContent(),
  };
  const entry: PageEntry = {
    id: page.id,
    title,
    icon: '',
    parentId,
    ...(databaseId ? { databaseId } : {}),
  };
  return {
    page,
    changes: [
      change({ ...workspace, pages: [...workspace.pages, entry] }),
      change(page),
    ],
  };
}
export function createView(
  name = 'Table',
  layout: DatabaseView['layout'] = 'table',
): DatabaseView {
  return { id: id(), name, layout, hidden: [], filters: [], sorts: [] };
}
export function createDatabase(
  files: NoteFiles,
  workspaceId: string,
  pageId: string,
  title = 'Nouvelle table',
) {
  const page = getPage(files, workspaceId, pageId);
  if (!page) throw new Error('Page introuvable.');
  const database: NoteDatabase = {
    version: 1,
    kind: 'database',
    id: id(),
    workspaceId,
    pageId,
    title,
    columns: [
      { id: id(), name: 'Nom', type: 'title' },
      { id: id(), name: 'Catégorie', type: 'select', options: [] },
    ],
    rows: [],
    views: [createView()],
  };
  return { database, changes: [change(database)] };
}
export function createRow(
  files: NoteFiles,
  database: NoteDatabase,
  title = 'Sans titre',
  values: DatabaseRow['values'] = {},
) {
  if (database.rows.length >= 5000)
    throw new Error('Cette table contient déjà 5 000 éléments.');
  const result = createPage(
    files,
    database.workspaceId,
    database.pageId,
    title,
    database.id,
  );
  const row: DatabaseRow = { id: id(), pageId: result.page.id, values };
  return {
    ...result,
    row,
    changes: [
      ...result.changes,
      change({ ...database, rows: [...database.rows, row] }),
    ],
  };
}
export function createWorkspace(
  title: string,
  icon = '📝',
  color = NOTE_COLORS[0],
  template: 'blank' | 'school' = 'blank',
) {
  const workspace: Workspace = {
    version: 1,
    kind: 'workspace',
    id: id(),
    title: title.trim() || 'Mon Noosnote',
    ...iconFields(icon),
    color,
    pages: [],
  };
  const created = createPage(
    { [workspacePath(workspace.id)]: workspace },
    workspace.id,
    null,
    template === 'school' ? 'École' : 'Bienvenue',
  );
  const changes = created.changes;
  if (template === 'school') {
    let files = applyFiles({}, changes);
    const { database } = createDatabase(
      files,
      workspace.id,
      created.page.id,
      'École',
    );
    const category = database.columns[1];
    const option = { id: id(), name: 'Cours', color: NOTE_COLORS[1] };
    category.options = [option];
    database.views[0].groupBy = category.id;
    created.page.icon = '📚';
    const currentWorkspace = getWorkspace(files, workspace.id)!;
    changes.push(
      change({
        ...currentWorkspace,
        pages: currentWorkspace.pages.map((page) =>
          page.id === created.page.id ? { ...page, icon: '📚' } : page,
        ),
      }),
    );
    created.page.content = {
      type: 'doc',
      content: [
        { type: 'noosDatabase', attrs: { id: id(), databaseId: database.id } },
        { type: 'paragraph', attrs: { id: id() } },
      ],
    };
    changes.push(change(database), change(created.page));
    files = applyFiles(files, changes);
    for (const title of ['Anglais', 'Français']) {
      const current = getDatabase(files, workspace.id, database.id)!;
      const row = createRow(files, current, title, {
        [category.id]: option.id,
      });
      changes.push(...row.changes);
      files = applyFiles(files, row.changes);
    }
  }
  return {
    workspaceId: workspace.id,
    pageId: created.page.id,
    changes: deduplicateChanges(changes),
  };
}
export function deduplicateChanges(changes: NoteChange[]): NoteChange[] {
  return [...new Map(changes.map((change) => [change.path, change])).values()];
}
export function renamePage(
  files: NoteFiles,
  workspaceId: string,
  pageId: string,
  title: string,
  icon?: string,
): NoteChange[] {
  const workspace = getWorkspace(files, workspaceId);
  const page = getPage(files, workspaceId, pageId);
  if (!workspace || !page) return [];
  return [
    change({
      ...workspace,
      pages: workspace.pages.map((entry) =>
        entry.id === pageId
          ? { ...entry, title, ...(icon !== undefined ? iconFields(icon) : {}) }
          : entry,
      ),
    }),
    change({ ...page, title, ...(icon !== undefined ? iconFields(icon) : {}) }),
  ];
}
export function descendants(workspace: Workspace, pageId: string): Set<string> {
  const children = new Map<string, string[]>();
  for (const page of workspace.pages)
    if (page.parentId)
      children.set(page.parentId, [
        ...(children.get(page.parentId) ?? []),
        page.id,
      ]);
  const result = new Set<string>();
  const pending = [pageId];
  while (pending.length) {
    const next = pending.pop()!;
    if (result.has(next)) continue;
    result.add(next);
    pending.push(...(children.get(next) ?? []));
  }
  return result;
}
export function movePage(
  workspace: Workspace,
  pageId: string,
  parentId: string | null,
): NoteChange[] {
  if (
    parentId &&
    (!workspace.pages.some((page) => page.id === parentId && !page.trashedAt) ||
      descendants(workspace, pageId).has(parentId))
  )
    throw new Error(
      'Une page ne peut pas être déplacée dans ses propres sous-pages.',
    );
  const source = workspace.pages.find((page) => page.id === pageId);
  if (!source || source.databaseId)
    throw new Error('Déplace les éléments de table depuis leur table.');
  return [
    change({
      ...workspace,
      pages: workspace.pages.map((page) =>
        page.id === pageId ? { ...page, parentId } : page,
      ),
    }),
  ];
}
export function trashPage(workspace: Workspace, pageId: string): NoteChange[] {
  const ids = descendants(workspace, pageId);
  const trashedAt = new Date().toISOString();
  return [
    change({
      ...workspace,
      pages: workspace.pages.map((page) =>
        ids.has(page.id) && !page.trashedAt ? { ...page, trashedAt } : page,
      ),
    }),
  ];
}
export function restorePage(
  workspace: Workspace,
  pageId: string,
): NoteChange[] {
  const target = workspace.pages.find((page) => page.id === pageId);
  if (!target) return [];
  const children = descendants(workspace, pageId);
  const restore = new Set(children);
  let parent = target.parentId;
  while (parent) {
    if (restore.has(parent)) break;
    restore.add(parent);
    parent =
      workspace.pages.find((page) => page.id === parent)?.parentId ?? null;
  }
  return [
    change({
      ...workspace,
      pages: workspace.pages.map((page) =>
        restore.has(page.id) &&
        (page.trashedAt === target.trashedAt || !children.has(page.id))
          ? (({ trashedAt: _trashedAt, ...rest }) => rest)(page)
          : page,
      ),
    }),
  ];
}
export function breadcrumbs(workspace: Workspace, pageId: string): PageEntry[] {
  const result: PageEntry[] = [];
  const seen = new Set<string>();
  let page = workspace.pages.find((page) => page.id === pageId);
  while (page && !seen.has(page.id)) {
    result.unshift(page);
    seen.add(page.id);
    page = workspace.pages.find((candidate) => candidate.id === page!.parentId);
  }
  return result;
}
export function activeRows(
  files: NoteFiles,
  database: NoteDatabase,
): DatabaseRow[] {
  const pages = new Set(
    getWorkspace(files, database.workspaceId)
      ?.pages.filter((page) => !page.trashedAt)
      .map((page) => page.id),
  );
  return database.rows.filter((row) => pages.has(row.pageId));
}
export function duplicatePage(
  files: NoteFiles,
  workspaceId: string,
  pageId: string,
) {
  const workspace = getWorkspace(files, workspaceId);
  const source = getPage(files, workspaceId, pageId);
  if (!workspace || !source) throw new Error('Page introuvable.');
  const copied = descendants(workspace, pageId);
  if (workspace.pages.length + copied.size > 4096)
    throw new Error(
      'Cette copie dépasse la limite de 4 096 pages du Noosnote.',
    );
  const pageIds = new Map([...copied].map((pageId) => [pageId, id()]));
  const sourceDatabases = databases(files, workspaceId).filter((database) =>
    copied.has(database.pageId),
  );
  const databaseIds = new Map(
    sourceDatabases.map((database) => [database.id, id()]),
  );
  const rewrite = (node: RichNode): RichNode => ({
    ...node,
    ...(node.attrs
      ? {
          attrs: {
            ...node.attrs,
            ...(node.attrs.id ? { id: id() } : {}),
            ...(typeof node.attrs.databaseId === 'string'
              ? {
                  databaseId:
                    databaseIds.get(node.attrs.databaseId) ??
                    node.attrs.databaseId,
                }
              : {}),
            ...(typeof node.attrs.pageId === 'string'
              ? { pageId: pageIds.get(node.attrs.pageId) ?? node.attrs.pageId }
              : {}),
          },
        }
      : {}),
    ...(node.content ? { content: node.content.map(rewrite) } : {}),
  });
  const entries: PageEntry[] = [];
  const changes: NoteChange[] = [];
  for (const entry of workspace.pages.filter((page) => copied.has(page.id))) {
    const page = getPage(files, workspaceId, entry.id);
    if (!page)
      throw new Error('Charge les sous-pages avant de dupliquer cette page.');
    const copiedId = pageIds.get(entry.id)!;
    const title =
      entry.id === pageId
        ? `${entry.title || 'Sans titre'} — copie`
        : entry.title;
    entries.push({
      ...entry,
      id: copiedId,
      title,
      parentId: entry.parentId
        ? (pageIds.get(entry.parentId) ?? entry.parentId)
        : null,
      ...(entry.databaseId
        ? { databaseId: databaseIds.get(entry.databaseId) ?? entry.databaseId }
        : {}),
    });
    changes.push(
      change({ ...page, id: copiedId, title, content: rewrite(page.content) }),
    );
  }
  for (const database of sourceDatabases) {
    changes.push(
      change({
        ...database,
        id: databaseIds.get(database.id)!,
        pageId: pageIds.get(database.pageId)!,
        columns: database.columns.map((column) => ({
          ...column,
          ...(column.databaseId
            ? {
                databaseId:
                  databaseIds.get(column.databaseId) ?? column.databaseId,
              }
            : {}),
        })),
        rows: database.rows.map((row) => ({
          ...row,
          id: id(),
          pageId: pageIds.get(row.pageId) ?? row.pageId,
          values: Object.fromEntries(
            Object.entries(row.values).map(([key, value]) => [
              key,
              Array.isArray(value)
                ? value.map((value) => pageIds.get(value) ?? value)
                : value,
            ]),
          ),
        })),
        views: database.views.map((view) => ({ ...view, id: id() })),
      }),
    );
  }
  const rowParent = workspace.pages.find(
    (page) => page.id === pageId,
  )?.databaseId;
  if (rowParent) {
    const parent = getDatabase(files, workspaceId, rowParent);
    const row = parent?.rows.find((row) => row.pageId === pageId);
    if (parent && row)
      changes.push(
        change({
          ...parent,
          rows: [
            ...parent.rows,
            { ...row, id: id(), pageId: pageIds.get(pageId)! },
          ],
        }),
      );
  }
  changes.push(
    change({ ...workspace, pages: [...workspace.pages, ...entries] }),
  );
  return { pageId: pageIds.get(pageId)!, changes };
}

export function pageText(node: RichNode): string {
  if (node.type === 'text') return node.text ?? '';
  return (node.content ?? [])
    .map(pageText)
    .join(node.type === 'paragraph' || node.type === 'heading' ? '' : '\n');
}
export function searchPages(
  files: NoteFiles,
  workspace: Workspace,
  query: string,
): PageEntry[] {
  const normalized = query
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLocaleLowerCase();
  const normalize = (value: string) =>
    value
      .normalize('NFKD')
      .replace(/[\u0300-\u036f]/g, '')
      .toLocaleLowerCase();
  return workspace.pages.filter(
    (page) =>
      !page.trashedAt &&
      (normalize(page.title).includes(normalized) ||
        normalize(
          pageText(
            getPage(files, workspace.id, page.id)?.content ?? { type: 'doc' },
          ),
        ).includes(normalized)),
  );
}
export function normalizeColumnValues(
  database: NoteDatabase,
  column: Column,
): NoteDatabase {
  return {
    ...database,
    columns: database.columns.map((current) =>
      current.id === column.id ? column : current,
    ),
    rows: database.rows.map((row) => {
      const values = { ...row.values };
      const old = values[column.id];
      if (
        column.type === 'title' ||
        column.type === 'formula' ||
        column.type === 'rollup'
      )
        delete values[column.id];
      else if (column.type === 'number')
        values[column.id] =
          old === null || old === undefined || old === ''
            ? null
            : Number.isFinite(Number(old))
              ? Number(old)
              : null;
      else if (column.type === 'checkbox') values[column.id] = old === true;
      else if (column.type === 'multiSelect')
        values[column.id] = (
          Array.isArray(old) ? old : typeof old === 'string' ? [old] : []
        ).filter((value) =>
          column.options?.some((option) => option.id === value),
        );
      else if (column.type === 'relation') {
        const previous = database.columns.find(
          (current) => current.id === column.id,
        );
        values[column.id] =
          previous?.type === 'relation' &&
          previous.databaseId === column.databaseId &&
          Array.isArray(old)
            ? old
            : [];
      } else if (column.type === 'select') {
        const selected = Array.isArray(old) ? old[0] : old;
        values[column.id] = column.options?.some(
          (option) => option.id === selected,
        )
          ? (selected ?? null)
          : null;
      } else
        values[column.id] = Array.isArray(old)
          ? old
              .map(
                (value) =>
                  database.columns
                    .find((current) => current.id === column.id)
                    ?.options?.find((option) => option.id === value)?.name ??
                  value,
              )
              .join(', ')
          : old === null || old === undefined
            ? ''
            : String(old);
      return { ...row, values };
    }),
  };
}
