export type RichNode = {
  type: string;
  attrs?: Record<string, unknown>;
  text?: string;
  marks?: { type: string; attrs?: Record<string, unknown> }[];
  content?: RichNode[];
};

export type PageEntry = {
  id: string;
  title: string;
  icon: string;
  parentId: string | null;
  databaseId?: string;
  trashedAt?: string;
};

export type Workspace = {
  version: 1;
  kind: 'workspace';
  id: string;
  title: string;
  icon: string;
  color: string;
  pages: PageEntry[];
};

export type NotePage = {
  version: 1;
  kind: 'page';
  id: string;
  workspaceId: string;
  title: string;
  icon: string;
  content: RichNode;
};

export type PropertyType =
  | 'title'
  | 'text'
  | 'number'
  | 'select'
  | 'multiSelect'
  | 'date'
  | 'checkbox'
  | 'url'
  | 'email'
  | 'phone'
  | 'relation'
  | 'formula'
  | 'rollup';

export type PropertyOption = { id: string; name: string; color: string };
export type Column = {
  id: string;
  name: string;
  type: PropertyType;
  width?: number;
  options?: PropertyOption[];
  databaseId?: string;
  expression?: string;
  relationId?: string;
  propertyId?: string;
  aggregate?: 'count' | 'sum' | 'average' | 'min' | 'max' | 'show';
};

export type CellValue = string | number | boolean | string[] | null;
export type DatabaseRow = {
  id: string;
  pageId: string;
  values: Record<string, CellValue>;
};
export type FilterOperator =
  | 'contains'
  | 'notContains'
  | 'equals'
  | 'notEquals'
  | 'greater'
  | 'less'
  | 'empty'
  | 'notEmpty';
export type DatabaseView = {
  id: string;
  name: string;
  layout: 'table' | 'board' | 'list' | 'gallery' | 'calendar';
  groupBy?: string;
  dateProperty?: string;
  hidden: string[];
  filters: {
    id: string;
    propertyId: string;
    operator: FilterOperator;
    value: string;
  }[];
  sorts: { propertyId: string; direction: 'asc' | 'desc' }[];
};
export type NoteDatabase = {
  version: 1;
  kind: 'database';
  id: string;
  workspaceId: string;
  pageId: string;
  title: string;
  columns: Column[];
  rows: DatabaseRow[];
  views: DatabaseView[];
};

export type NoteDocument = Workspace | NotePage | NoteDatabase;
export type NoteFiles = Record<string, NoteDocument>;
export type NoteChange = { path: string; value: NoteDocument | null };
export type NoteConflict = {
  id: string;
  path: string;
  local: NoteDocument | null;
  remote: NoteDocument | null;
};
export type NoteRepository = {
  id: number;
  name: string;
  url: string;
  branch: string;
};
export type NoteSnapshot = {
  files: NoteFiles;
  incremental: boolean;
  deletedPaths: string[];
  repository: NoteRepository | null;
  dirtyCount: number;
  dirtyPaths: string[];
  conflicts: NoteConflict[];
  lastSyncedAt: string | null;
  setup: 'repository' | 'installation' | 'ready';
};
export type NoosnoteApi = {
  cached: () => Promise<NoteSnapshot>;
  discover: () => Promise<NoteSnapshot>;
  page: (workspaceId: string, pageId: string) => Promise<NoteSnapshot>;
  setup: () => Promise<NoteSnapshot>;
  openSetup: (mode: 'repository' | 'installation') => Promise<void>;
  save: (changes: NoteChange[]) => Promise<NoteSnapshot>;
  sync: () => Promise<NoteSnapshot>;
  resolve: (id: string, choice: 'local' | 'remote') => Promise<NoteSnapshot>;
};
