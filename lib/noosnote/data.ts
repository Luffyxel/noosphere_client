import type {
  CellValue,
  Column,
  DatabaseRow,
  DatabaseView,
  NoteDatabase,
  NoteFiles,
} from './types';
import { activeRows, databases, getWorkspace } from './model';

type Expression =
  | { kind: 'literal'; value: CellValue }
  | { kind: 'call'; name: string; arguments: Expression[] }
  | { kind: 'unary'; operator: string; value: Expression }
  | { kind: 'binary'; operator: string; left: Expression; right: Expression };
type Token = { kind: 'number' | 'string' | 'word' | 'operator'; value: string };
const expressions = new Map<string, Expression>();
const precedence: Record<string, number> = {
  '||': 1,
  '&&': 2,
  '==': 3,
  '!=': 3,
  '<': 4,
  '>': 4,
  '<=': 4,
  '>=': 4,
  '+': 5,
  '-': 5,
  '*': 6,
  '/': 6,
  '%': 6,
};

function tokenize(source: string): Token[] {
  if (source.length > 4096) throw new Error('Formule trop longue.');
  const tokens: Token[] = [];
  let offset = 0;
  while (offset < source.length) {
    const remaining = source.slice(offset);
    const whitespace = /^\s+/.exec(remaining);
    if (whitespace) {
      offset += whitespace[0].length;
      continue;
    }
    const number = /^(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?/.exec(remaining);
    if (number) {
      tokens.push({ kind: 'number', value: number[0] });
      offset += number[0].length;
    } else if (remaining[0] === '"' || remaining[0] === "'") {
      const quote = remaining[0];
      let value = '';
      offset += 1;
      while (offset < source.length && source[offset] !== quote) {
        if (source[offset] === '\\') {
          offset += 1;
          const escape = source[offset];
          value +=
            escape === 'n' ? '\n' : escape === 't' ? '\t' : (escape ?? '');
        } else value += source[offset];
        offset += 1;
      }
      if (source[offset] !== quote) throw new Error('Guillemets non fermés.');
      offset += 1;
      tokens.push({ kind: 'string', value });
    } else {
      const word = /^[a-zA-Z_][a-zA-Z_0-9]*/.exec(remaining);
      const operator = /^(?:==|!=|<=|>=|&&|\|\||[+\-*/%<>()!,])/.exec(
        remaining,
      );
      if (word) {
        tokens.push({ kind: 'word', value: word[0] });
        offset += word[0].length;
      } else if (operator) {
        tokens.push({ kind: 'operator', value: operator[0] });
        offset += operator[0].length;
      } else throw new Error(`Caractère inattendu : ${remaining[0]}.`);
    }
    if (tokens.length > 512) throw new Error('Formule trop complexe.');
  }
  return tokens;
}

function compile(source: string): Expression {
  const cached = expressions.get(source);
  if (cached) return cached;
  const tokens = tokenize(source);
  let position = 0;
  const take = (value: string) => {
    if (tokens[position]?.value === value) {
      position += 1;
      return true;
    }
    return false;
  };
  function parse(minimum = 0, depth = 0): Expression {
    if (depth > 32) throw new Error('Formule trop complexe.');
    const token = tokens[position++];
    if (!token) throw new Error('Expression incomplète.');
    let left: Expression;
    if (token.kind === 'number')
      left = { kind: 'literal', value: Number(token.value) };
    else if (token.kind === 'string')
      left = { kind: 'literal', value: token.value };
    else if (token.value === '(') {
      left = parse(0, depth + 1);
      if (!take(')')) throw new Error('Parenthèse manquante.');
    } else if (['!', '-', '+'].includes(token.value))
      left = {
        kind: 'unary',
        operator: token.value,
        value: parse(7, depth + 1),
      };
    else if (
      token.kind === 'word' &&
      ['true', 'false', 'null'].includes(token.value)
    )
      left = {
        kind: 'literal',
        value: token.value === 'null' ? null : token.value === 'true',
      };
    else if (token.kind === 'word' && take('(')) {
      const arguments_: Expression[] = [];
      if (!take(')')) {
        do {
          arguments_.push(parse(0, depth + 1));
        } while (take(','));
        if (!take(')')) throw new Error('Parenthèse manquante.');
      }
      left = { kind: 'call', name: token.value, arguments: arguments_ };
    } else throw new Error(`Expression inconnue : ${token.value}.`);
    while (position < tokens.length) {
      const operator = tokens[position].value;
      const strength = precedence[operator];
      if (strength === undefined || strength < minimum) break;
      position += 1;
      left = {
        kind: 'binary',
        operator,
        left,
        right: parse(strength + 1, depth + 1),
      };
    }
    return left;
  }
  const result = parse();
  if (position !== tokens.length)
    throw new Error('Expression inattendue en fin de formule.');
  if (expressions.size >= 128)
    expressions.delete(expressions.keys().next().value!);
  expressions.set(source, result);
  return result;
}

const numeric = (value: CellValue): number => {
  const number = Number(value);
  if (!Number.isFinite(number)) throw new Error('Un nombre est attendu.');
  return number;
};
const text = (value: CellValue) =>
  Array.isArray(value) ? value.join(', ') : value === null ? '' : String(value);
const empty = (value: CellValue) =>
  value === null ||
  value === '' ||
  (Array.isArray(value) && value.length === 0);

export function evaluateFormula(
  source: string,
  property: (name: string) => CellValue,
  clock = new Date(),
): CellValue {
  let remaining = 1024;
  function evaluate(node: Expression): CellValue {
    if (!node) throw new Error('Argument manquant.');
    if (--remaining < 0) throw new Error('Formule trop complexe.');
    if (node.kind === 'literal') return node.value;
    if (node.kind === 'unary') {
      const value = evaluate(node.value);
      return node.operator === '!'
        ? !value
        : node.operator === '-'
          ? -numeric(value)
          : numeric(value);
    }
    if (node.kind === 'binary') {
      const left = evaluate(node.left);
      if (node.operator === '&&' && !left) return false;
      if (node.operator === '||' && left) return true;
      const right = evaluate(node.right);
      switch (node.operator) {
        case '+':
          return typeof left === 'string' || typeof right === 'string'
            ? text(left) + text(right)
            : numeric(left) + numeric(right);
        case '-':
          return numeric(left) - numeric(right);
        case '*':
          return numeric(left) * numeric(right);
        case '/':
          if (numeric(right) === 0) throw new Error('Division par zéro.');
          return numeric(left) / numeric(right);
        case '%':
          if (numeric(right) === 0) throw new Error('Division par zéro.');
          return numeric(left) % numeric(right);
        case '==':
          return JSON.stringify(left) === JSON.stringify(right);
        case '!=':
          return JSON.stringify(left) !== JSON.stringify(right);
        case '>':
          return typeof left === 'number' && typeof right === 'number'
            ? left > right
            : text(left) > text(right);
        case '<':
          return typeof left === 'number' && typeof right === 'number'
            ? left < right
            : text(left) < text(right);
        case '>=':
          return typeof left === 'number' && typeof right === 'number'
            ? left >= right
            : text(left) >= text(right);
        case '<=':
          return typeof left === 'number' && typeof right === 'number'
            ? left <= right
            : text(left) <= text(right);
        case '&&':
          return Boolean(right);
        case '||':
          return Boolean(right);
        default:
          throw new Error('Opérateur inconnu.');
      }
    }
    if (node.name === 'if')
      return evaluate(
        evaluate(node.arguments[0]) ? node.arguments[1] : node.arguments[2],
      );
    const args = node.arguments.map(evaluate);
    const numbers = () =>
      args.flatMap((value) =>
        Array.isArray(value) ? value.map(numeric) : [numeric(value)],
      );
    switch (node.name) {
      case 'prop':
        return property(text(args[0] ?? null));
      case 'concat':
        return args.map(text).join('');
      case 'empty':
        return empty(args[0] ?? null);
      case 'length':
        return Array.isArray(args[0])
          ? args[0].length
          : text(args[0] ?? null).length;
      case 'toNumber':
        return numeric(args[0] ?? null);
      case 'toString':
        return text(args[0] ?? null);
      case 'round':
        return (
          Math.round(numeric(args[0] ?? null) * 10 ** numeric(args[1] ?? 0)) /
          10 ** numeric(args[1] ?? 0)
        );
      case 'floor':
        return Math.floor(numeric(args[0] ?? null));
      case 'ceil':
        return Math.ceil(numeric(args[0] ?? null));
      case 'abs':
        return Math.abs(numeric(args[0] ?? null));
      case 'min':
        return Math.min(...numbers());
      case 'max':
        return Math.max(...numbers());
      case 'sum':
        return numbers().reduce((sum, value) => sum + value, 0);
      case 'average': {
        const values = numbers();
        return values.length
          ? values.reduce((sum, value) => sum + value, 0) / values.length
          : 0;
      }
      case 'contains':
        return Array.isArray(args[0])
          ? args[0].includes(text(args[1] ?? null))
          : text(args[0] ?? null).includes(text(args[1] ?? null));
      case 'lower':
        return text(args[0] ?? null).toLocaleLowerCase();
      case 'upper':
        return text(args[0] ?? null).toLocaleUpperCase();
      case 'replace':
        return text(args[0] ?? null).replaceAll(
          text(args[1] ?? null),
          text(args[2] ?? null),
        );
      case 'and':
        return args.every(Boolean);
      case 'or':
        return args.some(Boolean);
      case 'not':
        return !args[0];
      case 'now':
        return clock.toISOString();
      case 'today':
        return `${clock.getFullYear()}-${String(clock.getMonth() + 1).padStart(2, '0')}-${String(clock.getDate()).padStart(2, '0')}`;
      case 'dateBetween': {
        const difference =
          Date.parse(text(args[0] ?? null)) - Date.parse(text(args[1] ?? null));
        if (!Number.isFinite(difference)) throw new Error('Date invalide.');
        return Math.trunc(
          difference / (text(args[2] ?? null) === 'hours' ? 3600000 : 86400000),
        );
      }
      case 'formatDate': {
        const date = new Date(text(args[0] ?? null));
        if (Number.isNaN(date.getTime())) throw new Error('Date invalide.');
        return text(args[1] ?? 'DD/MM/YYYY')
          .replaceAll('YYYY', String(date.getFullYear()))
          .replaceAll('MM', String(date.getMonth() + 1).padStart(2, '0'))
          .replaceAll('DD', String(date.getDate()).padStart(2, '0'));
      }
      default:
        throw new Error(`Fonction inconnue : ${node.name}.`);
    }
  }
  const value = evaluate(compile(source));
  if (typeof value === 'number' && !Number.isFinite(value))
    throw new Error('Résultat numérique invalide.');
  return value;
}

export function createCellReader(files: NoteFiles, clock = new Date()) {
  const cache = new Map<string, { value: CellValue; error?: string }>();
  const visiting = new Set<string>();
  const allDatabases = databases(files);
  const pageNames = new Map(
    Object.values(files)
      .filter((value) => value.kind === 'workspace')
      .flatMap((workspace) =>
        workspace.pages.map((page) => [page.id, page.title] as const),
      ),
  );
  function read(
    database: NoteDatabase,
    row: DatabaseRow,
    column: Column,
  ): { value: CellValue; error?: string } {
    const key = `${database.id}:${row.id}:${column.id}`;
    const previous = cache.get(key);
    if (previous) return previous;
    if (visiting.has(key) || visiting.size >= 24)
      return { value: null, error: 'Référence circulaire.' };
    visiting.add(key);
    let result: { value: CellValue; error?: string };
    try {
      let value: CellValue;
      if (column.type === 'title')
        value = pageNames.get(row.pageId) ?? 'Sans titre';
      else if (column.type === 'formula')
        value = evaluateFormula(
          column.expression ?? '',
          (name) => {
            const property = database.columns.find(
              (column) => column.id === name || column.name === name,
            );
            if (!property) throw new Error(`Propriété introuvable : ${name}.`);
            const result = read(database, row, property);
            if (result.error) throw new Error(result.error);
            return displayCell(property, result.value, pageNames);
          },
          clock,
        );
      else if (column.type === 'rollup') {
        const relation = database.columns.find(
          (property) =>
            property.id === column.relationId && property.type === 'relation',
        );
        const related = allDatabases.find(
          (database) => database.id === relation?.databaseId,
        );
        const links = relation ? row.values[relation.id] : [];
        const rows =
          related && Array.isArray(links)
            ? activeRows(files, related).filter((row) =>
                links.includes(row.pageId),
              )
            : [];
        const property = related?.columns.find(
          (property) => property.id === column.propertyId,
        );
        if (column.aggregate === 'count') value = rows.length;
        else {
          const values =
            related && property
              ? rows.map((row) => read(related, row, property).value)
              : [];
          const numbers = values.filter(
            (value): value is number =>
              typeof value === 'number' && Number.isFinite(value),
          );
          switch (column.aggregate) {
            case 'sum':
              value = numbers.reduce((sum, value) => sum + value, 0);
              break;
            case 'average':
              value = numbers.length
                ? numbers.reduce((sum, value) => sum + value, 0) /
                  numbers.length
                : null;
              break;
            case 'min':
              value = numbers.length ? Math.min(...numbers) : null;
              break;
            case 'max':
              value = numbers.length ? Math.max(...numbers) : null;
              break;
            default:
              value = values.map((value) => text(value));
              break;
          }
        }
      } else value = row.values[column.id] ?? null;
      result = { value };
    } catch (error) {
      result = {
        value: null,
        error: error instanceof Error ? error.message : 'Formule invalide.',
      };
    }
    visiting.delete(key);
    cache.set(key, result);
    return result;
  }
  return {
    read,
    label: (column: Column, value: CellValue) =>
      displayCell(column, value, pageNames),
  };
}

export function displayCell(
  column: Column,
  value: CellValue,
  pageNames?: Map<string, string>,
): CellValue {
  if (column.type === 'select')
    return column.options?.find((option) => option.id === value)?.name ?? '';
  if (column.type === 'multiSelect' && Array.isArray(value))
    return value
      .map(
        (value) =>
          column.options?.find((option) => option.id === value)?.name ?? '',
      )
      .filter(Boolean);
  if (column.type === 'relation' && Array.isArray(value))
    return value.map((value) => pageNames?.get(value) ?? 'Page indisponible');
  return value;
}

export function queryRows(
  files: NoteFiles,
  database: NoteDatabase,
  view: DatabaseView,
  query = '',
  reader = createCellReader(files),
): DatabaseRow[] {
  let rows = activeRows(files, database);
  if (query)
    rows = rows.filter((row) =>
      database.columns.some((column) =>
        text(reader.label(column, reader.read(database, row, column).value))
          .toLocaleLowerCase()
          .includes(query.toLocaleLowerCase()),
      ),
    );
  rows = rows.filter((row) =>
    view.filters.every((filter) => {
      const column = database.columns.find(
        (column) => column.id === filter.propertyId,
      );
      if (!column) return true;
      const raw = reader.read(database, row, column).value;
      const value = reader.label(column, raw);
      const formatted = text(value).toLocaleLowerCase();
      const expected = filter.value.toLocaleLowerCase();
      switch (filter.operator) {
        case 'contains':
          return formatted.includes(expected);
        case 'notContains':
          return !formatted.includes(expected);
        case 'equals':
          return (
            formatted === expected ||
            raw === filter.value ||
            (Array.isArray(raw) && raw.includes(filter.value))
          );
        case 'notEquals':
          return (
            formatted !== expected &&
            raw !== filter.value &&
            !(Array.isArray(raw) && raw.includes(filter.value))
          );
        case 'greater':
          return column.type === 'number' ||
            column.type === 'formula' ||
            column.type === 'rollup'
            ? Number(raw) > Number(filter.value)
            : formatted > expected;
        case 'less':
          return column.type === 'number' ||
            column.type === 'formula' ||
            column.type === 'rollup'
            ? Number(raw) < Number(filter.value)
            : formatted < expected;
        case 'empty':
          return empty(raw);
        case 'notEmpty':
          return !empty(raw);
        default:
          return true;
      }
    }),
  );
  if (view.sorts.length)
    rows = [...rows].sort((left, right) => {
      for (const sort of view.sorts) {
        const column = database.columns.find(
          (column) => column.id === sort.propertyId,
        );
        if (!column) continue;
        const a = reader.label(
          column,
          reader.read(database, left, column).value,
        );
        const b = reader.label(
          column,
          reader.read(database, right, column).value,
        );
        const comparison =
          typeof a === 'number' && typeof b === 'number'
            ? a - b
            : text(a).localeCompare(text(b), 'fr', { numeric: true });
        if (comparison)
          return sort.direction === 'asc' ? comparison : -comparison;
      }
      return 0;
    });
  return rows;
}

export function rowGroups(
  files: NoteFiles,
  database: NoteDatabase,
  view: DatabaseView,
  rows: DatabaseRow[],
  reader = createCellReader(files),
) {
  const column = database.columns.find((column) => column.id === view.groupBy);
  if (!column)
    return [{ id: 'all', name: 'Tous les éléments', color: '', rows }];
  const groups = new Map<
    string,
    { id: string; name: string; color: string; rows: DatabaseRow[] }
  >();
  for (const option of column.options ?? [])
    groups.set(option.id, {
      id: option.id,
      name: option.name,
      color: option.color,
      rows: [],
    });
  for (const row of rows) {
    const value = reader.read(database, row, column).value;
    const keys =
      Array.isArray(value) && value.length
        ? value
        : [empty(value) ? '' : text(value)];
    for (const key of keys) {
      let group = groups.get(key);
      if (!group) {
        group = {
          id: key,
          name: key ? text(reader.label(column, key)) : 'Sans catégorie',
          color: '',
          rows: [],
        };
        groups.set(key, group);
      }
      group.rows.push(row);
    }
  }
  return [...groups.values()];
}

export function rowPageTitle(
  files: NoteFiles,
  database: NoteDatabase,
  row: DatabaseRow,
): string {
  return (
    getWorkspace(files, database.workspaceId)?.pages.find(
      (page) => page.id === row.pageId,
    )?.title || 'Sans titre'
  );
}
