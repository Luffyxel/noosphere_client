import { useMemo, useState } from 'react';
import {
  ArrowUpRight,
  CalendarDays,
  ChevronLeft,
  ChevronRight,
  Columns3,
  Download,
  GripVertical,
  LayoutGrid,
  List,
  Plus,
  Search,
  Settings2,
  Table2,
  Trash2,
  X,
} from 'lucide-react';
import type {
  CellValue,
  Column,
  DatabaseRow,
  DatabaseView,
  FilterOperator,
  NoteDatabase,
  PropertyType,
} from '@/lib/noosnote/types';
import {
  activeRows,
  change,
  createRow,
  createView,
  databases,
  getDatabase,
  getWorkspace,
  id,
  normalizeColumnValues,
  NOTE_COLORS,
  renamePage,
  trashPage,
} from '@/lib/noosnote/model';
import {
  createCellReader,
  queryRows,
  rowGroups,
  rowPageTitle,
} from '@/lib/noosnote/data';
import { downloadFile, NoteDialog, useNoteContext } from './context';

const PROPERTY_TYPES: [PropertyType, string][] = [
  ['text', 'Texte'],
  ['number', 'Nombre'],
  ['select', 'Catégorie'],
  ['multiSelect', 'Étiquettes'],
  ['date', 'Date'],
  ['checkbox', 'Case à cocher'],
  ['url', 'Lien'],
  ['email', 'E-mail'],
  ['phone', 'Téléphone'],
  ['relation', 'Relation'],
  ['formula', 'Formule'],
  ['rollup', 'Agrégation'],
];
const LAYOUTS = [
  { id: 'table', name: 'Table', icon: Table2 },
  { id: 'board', name: 'Colonnes', icon: Columns3 },
  { id: 'list', name: 'Liste', icon: List },
  { id: 'gallery', name: 'Galerie', icon: LayoutGrid },
  { id: 'calendar', name: 'Calendrier', icon: CalendarDays },
] as const;
const FILTERS: [FilterOperator, string][] = [
  ['contains', 'contient'],
  ['notContains', 'ne contient pas'],
  ['equals', 'est égal à'],
  ['notEquals', 'est différent de'],
  ['greater', 'est supérieur à'],
  ['less', 'est inférieur à'],
  ['empty', 'est vide'],
  ['notEmpty', 'est rempli'],
];

function Cell({
  database,
  row,
  column,
}: {
  database: NoteDatabase;
  row: DatabaseRow;
  column: Column;
}) {
  const { notes, workspaceId, openPage, report } = useNoteContext();
  const title = rowPageTitle(notes.files, database, row);
  const [titleDraft, setTitleDraft] = useState<string | null>(null);
  function write(value: CellValue) {
    const current = getDatabase(
      notes.filesRef.current.files,
      workspaceId,
      database.id,
    );
    if (current)
      notes.edit([
        change({
          ...current,
          rows: current.rows.map((item) =>
            item.id === row.id
              ? { ...item, values: { ...item.values, [column.id]: value } }
              : item,
          ),
        }),
      ]);
  }
  if (column.type === 'title')
    return (
      <div className="note-title-cell">
        <input
          aria-label="Nom de l’élément"
          maxLength={512}
          value={titleDraft ?? title}
          placeholder="Sans titre"
          onChange={(event) => setTitleDraft(event.target.value)}
          onBlur={() => {
            if (titleDraft === null) return;
            const value = titleDraft.trim() || 'Sans titre';
            void notes
              .loadPage(workspaceId, row.pageId)
              .then(() => {
                notes.edit(
                  renamePage(
                    notes.filesRef.current.files,
                    workspaceId,
                    row.pageId,
                    value,
                  ),
                );
                setTitleDraft(null);
              })
              .catch(report);
          }}
          onKeyDown={(event) => {
            if (event.key === 'Enter') event.currentTarget.blur();
          }}
        />
        <button
          title={`Ouvrir ${title}`}
          aria-label={`Ouvrir ${title}`}
          onClick={() => openPage(row.pageId)}
        >
          <ArrowUpRight size={15} />
        </button>
      </div>
    );
  const value = row.values[column.id];
  if (column.type === 'checkbox')
    return (
      <input
        type="checkbox"
        className="note-check"
        aria-label={column.name}
        checked={value === true}
        onChange={(event) => write(event.target.checked)}
      />
    );
  if (column.type === 'select')
    return (
      <select
        aria-label={column.name}
        value={typeof value === 'string' ? value : ''}
        onChange={(event) => write(event.target.value || null)}
      >
        <option value="">—</option>
        {column.options?.map((option) => (
          <option key={option.id} value={option.id}>
            {option.name}
          </option>
        ))}
      </select>
    );
  if (column.type === 'multiSelect' || column.type === 'relation') {
    const related = databases(notes.files).find(
      (item) => item.id === column.databaseId,
    );
    const options =
      column.type === 'multiSelect'
        ? (column.options ?? [])
        : related
          ? activeRows(notes.files, related).map((item) => ({
              id: item.pageId,
              name: rowPageTitle(notes.files, related, item),
            }))
          : [];
    const selected = Array.isArray(value) ? value : [];
    return (
      <details className="note-cell-picker">
        <summary>
          {selected.length
            ? selected
                .map(
                  (value) =>
                    options.find((option) => option.id === value)?.name ||
                    'Indisponible',
                )
                .join(', ')
            : '—'}
        </summary>
        <div className="note-cell-options">
          {options.length === 0 && <p>Aucun élément</p>}
          {options.map((option) => (
            <label key={option.id}>
              <input
                type="checkbox"
                checked={selected.includes(option.id)}
                onChange={(event) =>
                  write(
                    event.target.checked
                      ? [...selected, option.id].slice(0, 200)
                      : selected.filter((value) => value !== option.id),
                  )
                }
              />
              {option.name}
              {column.type === 'relation' && (
                <button
                  type="button"
                  aria-label={`Ouvrir ${option.name}`}
                  onClick={() => {
                    if (related?.workspaceId === workspaceId)
                      openPage(option.id);
                  }}
                  disabled={related?.workspaceId !== workspaceId}
                >
                  <ArrowUpRight size={13} />
                </button>
              )}
            </label>
          ))}
        </div>
      </details>
    );
  }
  if (column.type === 'formula' || column.type === 'rollup') {
    const reader = createCellReader(notes.files);
    const result = reader.read(database, row, column);
    const label = reader.label(column, result.value);
    return (
      <span
        className={result.error ? 'note-cell-error' : 'note-calculated'}
        title={result.error || 'Valeur calculée'}
      >
        {result.error
          ? 'Erreur'
          : Array.isArray(label)
            ? label.join(', ')
            : String(label ?? '')}
      </span>
    );
  }
  const type =
    column.type === 'date'
      ? 'date'
      : column.type === 'number'
        ? 'number'
        : column.type === 'email'
          ? 'email'
          : column.type === 'url'
            ? 'url'
            : column.type === 'phone'
              ? 'tel'
              : 'text';
  return (
    <div className="note-text-cell">
      <input
        aria-label={column.name}
        type={type}
        step={type === 'number' ? 'any' : undefined}
        maxLength={16384}
        value={
          typeof value === 'string' || typeof value === 'number' ? value : ''
        }
        onChange={(event) =>
          write(
            type === 'number'
              ? event.target.value === ''
                ? null
                : Number.isFinite(event.target.valueAsNumber)
                  ? event.target.valueAsNumber
                  : null
              : event.target.value,
          )
        }
      />
      {column.type === 'url' &&
        typeof value === 'string' &&
        /^https?:\/\//i.test(value) && (
          <a
            href={value}
            target="_blank"
            rel="noreferrer"
            title="Ouvrir le lien"
          >
            <ArrowUpRight size={13} />
          </a>
        )}
    </div>
  );
}

function PropertyEditor({
  database,
  source,
  onClose,
}: {
  database: NoteDatabase;
  source: Column;
  onClose: () => void;
}) {
  const { notes, workspaceId } = useNoteContext();
  const [column, setColumn] = useState<Column>(structuredClone(source));
  const all = databases(notes.files);
  const relation = database.columns.find(
    (item) => item.id === column.relationId && item.type === 'relation',
  );
  const related = all.find(
    (item) =>
      item.id ===
      (column.type === 'rollup' ? relation?.databaseId : column.databaseId),
  );
  function submit() {
    const latest = getDatabase(
      notes.filesRef.current.files,
      workspaceId,
      database.id,
    );
    if (!latest) return;
    const normalized = {
      ...column,
      name: column.name.trim() || 'Propriété',
      ...(column.type === 'formula'
        ? { expression: column.expression ?? '' }
        : {}),
    };
    const current = latest.columns.some((item) => item.id === column.id)
      ? latest
      : { ...latest, columns: [...latest.columns, normalized] };
    notes.edit([change(normalizeColumnValues(current, normalized))]);
    onClose();
  }
  function remove() {
    const latest = getDatabase(
      notes.filesRef.current.files,
      workspaceId,
      database.id,
    );
    if (!latest || column.type === 'title') return;
    notes.edit([
      change({
        ...latest,
        columns: latest.columns.filter((item) => item.id !== column.id),
        rows: latest.rows.map((row) => ({
          ...row,
          values: Object.fromEntries(
            Object.entries(row.values).filter(([key]) => key !== column.id),
          ),
        })),
        views: latest.views.map((view) => ({
          ...view,
          groupBy: view.groupBy === column.id ? undefined : view.groupBy,
          dateProperty:
            view.dateProperty === column.id ? undefined : view.dateProperty,
          filters: view.filters.filter(
            (filter) => filter.propertyId !== column.id,
          ),
          sorts: view.sorts.filter((sort) => sort.propertyId !== column.id),
          hidden: view.hidden.filter((value) => value !== column.id),
        })),
      }),
    ]);
    onClose();
  }
  return (
    <NoteDialog title="Propriété" open onClose={onClose}>
      <form
        className="note-form"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <label>
          Nom
          <input
            value={column.name}
            maxLength={128}
            onChange={(event) =>
              setColumn({ ...column, name: event.target.value })
            }
          />
        </label>
        <label>
          Type
          <select
            disabled={source.type === 'title'}
            value={column.type}
            onChange={(event) =>
              setColumn({
                ...column,
                type: event.target.value as PropertyType,
                options: column.options ?? [],
                expression: column.expression ?? '',
                aggregate: column.aggregate ?? 'count',
              })
            }
          >
            {source.type === 'title' ? (
              <option value="title">Titre de la page</option>
            ) : (
              PROPERTY_TYPES.map(([type, label]) => (
                <option key={type} value={type}>
                  {label}
                </option>
              ))
            )}
          </select>
        </label>
        {(column.type === 'select' || column.type === 'multiSelect') && (
          <div className="note-option-list">
            <p>Catégories personnalisées</p>
            {column.options?.map((option, index) => (
              <div key={option.id}>
                <input
                  type="color"
                  aria-label="Couleur de la catégorie"
                  value={option.color}
                  onChange={(event) =>
                    setColumn({
                      ...column,
                      options: column.options!.map((current) =>
                        current.id === option.id
                          ? { ...current, color: event.target.value }
                          : current,
                      ),
                    })
                  }
                />
                <input
                  aria-label="Nom de la catégorie"
                  maxLength={128}
                  value={option.name}
                  onChange={(event) =>
                    setColumn({
                      ...column,
                      options: column.options!.map((current) =>
                        current.id === option.id
                          ? { ...current, name: event.target.value }
                          : current,
                      ),
                    })
                  }
                />
                <button
                  type="button"
                  aria-label={`Supprimer ${option.name || index + 1}`}
                  onClick={() =>
                    setColumn({
                      ...column,
                      options: column.options!.filter(
                        (current) => current.id !== option.id,
                      ),
                    })
                  }
                >
                  <X size={15} />
                </button>
              </div>
            ))}
            <button
              type="button"
              disabled={(column.options?.length ?? 0) >= 200}
              onClick={() =>
                setColumn({
                  ...column,
                  options: [
                    ...(column.options ?? []),
                    {
                      id: id(),
                      name: 'Nouvelle catégorie',
                      color:
                        NOTE_COLORS[
                          (column.options?.length ?? 0) % NOTE_COLORS.length
                        ],
                    },
                  ],
                })
              }
            >
              <Plus size={15} /> Ajouter une catégorie
            </button>
          </div>
        )}
        {column.type === 'relation' && (
          <label>
            Table liée
            <select
              value={column.databaseId ?? ''}
              onChange={(event) =>
                setColumn({
                  ...column,
                  databaseId: event.target.value || undefined,
                })
              }
            >
              <option value="">Choisir une table</option>
              {all.map((item) => (
                <option key={item.id} value={item.id}>
                  {getWorkspace(notes.files, item.workspaceId)?.title} /{' '}
                  {item.title}
                </option>
              ))}
            </select>
          </label>
        )}
        {column.type === 'formula' && (
          <>
            <label>
              Formule
              <textarea
                rows={3}
                placeholder={'prop("Note") * 2'}
                value={column.expression ?? ''}
                maxLength={4096}
                onChange={(event) =>
                  setColumn({ ...column, expression: event.target.value })
                }
              />
            </label>
            <p className="note-muted">
              {
                'Utilise prop("Nom"), if(), sum(), concat(), round(), today()… Exemple : if(prop("Terminé"), "Fait", "À faire").'
              }
            </p>
          </>
        )}
        {column.type === 'rollup' && (
          <>
            <label>
              Relation
              <select
                value={column.relationId ?? ''}
                onChange={(event) =>
                  setColumn({
                    ...column,
                    relationId: event.target.value || undefined,
                    propertyId: undefined,
                  })
                }
              >
                <option value="">Choisir une relation</option>
                {database.columns
                  .filter((item) => item.type === 'relation')
                  .map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.name}
                    </option>
                  ))}
              </select>
            </label>
            <label>
              Propriété liée
              <select
                value={column.propertyId ?? ''}
                onChange={(event) =>
                  setColumn({
                    ...column,
                    propertyId: event.target.value || undefined,
                  })
                }
              >
                <option value="">Choisir une propriété</option>
                {related?.columns.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.name}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Calcul
              <select
                value={column.aggregate ?? 'count'}
                onChange={(event) =>
                  setColumn({
                    ...column,
                    aggregate: event.target.value as Column['aggregate'],
                  })
                }
              >
                {[
                  ['count', 'Compter'],
                  ['sum', 'Somme'],
                  ['average', 'Moyenne'],
                  ['min', 'Minimum'],
                  ['max', 'Maximum'],
                  ['show', 'Afficher les valeurs'],
                ].map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
          </>
        )}
        <div className="note-actions">
          {source.type !== 'title' &&
            database.columns.some((item) => item.id === column.id) && (
              <button type="button" className="note-danger" onClick={remove}>
                <Trash2 size={15} /> Supprimer
              </button>
            )}
          <button type="button" onClick={onClose}>
            Annuler
          </button>
          <button type="submit" className="note-primary">
            Enregistrer
          </button>
        </div>
      </form>
    </NoteDialog>
  );
}

function ViewEditor({
  database,
  source,
  onClose,
}: {
  database: NoteDatabase;
  source: DatabaseView;
  onClose: () => void;
}) {
  const { notes, workspaceId } = useNoteContext();
  const [view, setView] = useState<DatabaseView>(structuredClone(source));
  function save() {
    const latest = getDatabase(
      notes.filesRef.current.files,
      workspaceId,
      database.id,
    );
    if (!latest) return;
    const exists = latest.views.some((item) => item.id === view.id);
    notes.edit([
      change({
        ...latest,
        views: exists
          ? latest.views.map((item) => (item.id === view.id ? view : item))
          : [...latest.views, view],
      }),
    ]);
    onClose();
  }
  return (
    <NoteDialog title="Personnaliser la vue" open onClose={onClose}>
      <form
        className="note-form"
        onSubmit={(event) => {
          event.preventDefault();
          save();
        }}
      >
        <label>
          Nom
          <input
            value={view.name}
            maxLength={128}
            onChange={(event) => setView({ ...view, name: event.target.value })}
          />
        </label>
        <label>
          Présentation
          <select
            value={view.layout}
            onChange={(event) =>
              setView({
                ...view,
                layout: event.target.value as DatabaseView['layout'],
              })
            }
          >
            {LAYOUTS.map((layout) => (
              <option key={layout.id} value={layout.id}>
                {layout.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Grouper par
          <select
            value={view.groupBy ?? ''}
            onChange={(event) =>
              setView({ ...view, groupBy: event.target.value || undefined })
            }
          >
            <option value="">Aucun groupe</option>
            {database.columns
              .filter((column) =>
                ['select', 'multiSelect', 'checkbox', 'text'].includes(
                  column.type,
                ),
              )
              .map((column) => (
                <option key={column.id} value={column.id}>
                  {column.name}
                </option>
              ))}
          </select>
        </label>
        {view.layout === 'calendar' && (
          <label>
            Date du calendrier
            <select
              value={view.dateProperty ?? ''}
              onChange={(event) =>
                setView({
                  ...view,
                  dateProperty: event.target.value || undefined,
                })
              }
            >
              <option value="">Choisir une date</option>
              {database.columns
                .filter((column) => column.type === 'date')
                .map((column) => (
                  <option key={column.id} value={column.id}>
                    {column.name}
                  </option>
                ))}
            </select>
          </label>
        )}
        <fieldset>
          <legend>Propriétés visibles</legend>
          <div className="note-property-checks">
            {database.columns.map((column) => (
              <label key={column.id}>
                <input
                  type="checkbox"
                  disabled={column.type === 'title'}
                  checked={!view.hidden.includes(column.id)}
                  onChange={(event) =>
                    setView({
                      ...view,
                      hidden: event.target.checked
                        ? view.hidden.filter((value) => value !== column.id)
                        : [...view.hidden, column.id],
                    })
                  }
                />
                {column.name}
              </label>
            ))}
          </div>
        </fieldset>
        <fieldset>
          <legend>Filtres</legend>
          {view.filters.map((filter) => (
            <div className="note-filter-row" key={filter.id}>
              <select
                aria-label="Propriété à filtrer"
                value={filter.propertyId}
                onChange={(event) =>
                  setView({
                    ...view,
                    filters: view.filters.map((item) =>
                      item.id === filter.id
                        ? { ...item, propertyId: event.target.value }
                        : item,
                    ),
                  })
                }
              >
                {database.columns.map((column) => (
                  <option key={column.id} value={column.id}>
                    {column.name}
                  </option>
                ))}
              </select>
              <select
                aria-label="Condition"
                value={filter.operator}
                onChange={(event) =>
                  setView({
                    ...view,
                    filters: view.filters.map((item) =>
                      item.id === filter.id
                        ? {
                            ...item,
                            operator: event.target.value as FilterOperator,
                          }
                        : item,
                    ),
                  })
                }
              >
                {FILTERS.map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
              {!['empty', 'notEmpty'].includes(filter.operator) && (
                <input
                  aria-label="Valeur du filtre"
                  value={filter.value}
                  onChange={(event) =>
                    setView({
                      ...view,
                      filters: view.filters.map((item) =>
                        item.id === filter.id
                          ? { ...item, value: event.target.value }
                          : item,
                      ),
                    })
                  }
                />
              )}
              <button
                type="button"
                aria-label="Retirer le filtre"
                onClick={() =>
                  setView({
                    ...view,
                    filters: view.filters.filter(
                      (item) => item.id !== filter.id,
                    ),
                  })
                }
              >
                <X size={14} />
              </button>
            </div>
          ))}
          <button
            type="button"
            onClick={() =>
              setView({
                ...view,
                filters: [
                  ...view.filters,
                  {
                    id: id(),
                    propertyId: database.columns[0].id,
                    operator: 'contains',
                    value: '',
                  },
                ],
              })
            }
          >
            <Plus size={14} /> Ajouter un filtre
          </button>
        </fieldset>
        <fieldset>
          <legend>Tri</legend>
          {view.sorts.map((sort, index) => (
            <div className="note-filter-row" key={index}>
              <select
                aria-label="Propriété à trier"
                value={sort.propertyId}
                onChange={(event) =>
                  setView({
                    ...view,
                    sorts: view.sorts.map((item, i) =>
                      i === index
                        ? { ...item, propertyId: event.target.value }
                        : item,
                    ),
                  })
                }
              >
                {database.columns.map((column) => (
                  <option key={column.id} value={column.id}>
                    {column.name}
                  </option>
                ))}
              </select>
              <select
                aria-label="Sens du tri"
                value={sort.direction}
                onChange={(event) =>
                  setView({
                    ...view,
                    sorts: view.sorts.map((item, i) =>
                      i === index
                        ? {
                            ...item,
                            direction: event.target.value as 'asc' | 'desc',
                          }
                        : item,
                    ),
                  })
                }
              >
                <option value="asc">Croissant</option>
                <option value="desc">Décroissant</option>
              </select>
              <button
                type="button"
                aria-label="Retirer le tri"
                onClick={() =>
                  setView({
                    ...view,
                    sorts: view.sorts.filter((_, i) => i !== index),
                  })
                }
              >
                <X size={14} />
              </button>
            </div>
          ))}
          <button
            type="button"
            onClick={() =>
              setView({
                ...view,
                sorts: [
                  ...view.sorts,
                  { propertyId: database.columns[0].id, direction: 'asc' },
                ],
              })
            }
          >
            <Plus size={14} /> Ajouter un tri
          </button>
        </fieldset>
        <div className="note-actions">
          {database.views.length > 1 &&
            database.views.some((item) => item.id === view.id) && (
              <button
                type="button"
                className="note-danger"
                onClick={() => {
                  const latest = getDatabase(
                    notes.filesRef.current.files,
                    workspaceId,
                    database.id,
                  );
                  if (latest)
                    notes.edit([
                      change({
                        ...latest,
                        views: latest.views.filter(
                          (item) => item.id !== view.id,
                        ),
                      }),
                    ]);
                  onClose();
                }}
              >
                Supprimer la vue
              </button>
            )}
          <button type="button" onClick={onClose}>
            Annuler
          </button>
          <button type="submit" className="note-primary">
            Enregistrer
          </button>
        </div>
      </form>
    </NoteDialog>
  );
}

export function Database({ databaseId }: { databaseId: string }) {
  const { notes, workspaceId, openPage, report } = useNoteContext();
  const database = getDatabase(notes.files, workspaceId, databaseId);
  const [selectedView, setSelectedView] = useState('');
  const [query, setQuery] = useState('');
  const [limit, setLimit] = useState(100);
  const [property, setProperty] = useState<Column | null>(null);
  const [editingView, setEditingView] = useState<DatabaseView | null>(null);
  const [month, setMonth] = useState(
    () => new Date(new Date().getFullYear(), new Date().getMonth(), 1),
  );
  const reader = useMemo(() => createCellReader(notes.files), [notes.files]);
  if (!database)
    return <div className="note-empty">Cette table est indisponible.</div>;
  const view =
    database.views.find((view) => view.id === selectedView) ??
    database.views[0];
  const rows = queryRows(notes.files, database, view, query, reader);
  const shown = rows.slice(0, limit);
  const columns = database.columns.filter(
    (column) => column.type === 'title' || !view.hidden.includes(column.id),
  );
  const groups = rowGroups(notes.files, database, view, shown, reader);
  function update(patch: Partial<NoteDatabase>) {
    const current = getDatabase(
      notes.filesRef.current.files,
      workspaceId,
      databaseId,
    );
    if (current) notes.edit([change({ ...current, ...patch })]);
  }
  function addRow(groupId?: string, date?: string) {
    try {
      const current = getDatabase(
        notes.filesRef.current.files,
        workspaceId,
        databaseId,
      )!;
      const values: Record<string, CellValue> = {};
      const group = current.columns.find(
        (column) => column.id === view.groupBy,
      );
      if (group && groupId && groupId !== 'all')
        values[group.id] =
          group.type === 'multiSelect'
            ? [groupId]
            : group.type === 'checkbox'
              ? groupId === 'true'
              : groupId;
      if (view.dateProperty && date) values[view.dateProperty] = date;
      const result = createRow(
        notes.filesRef.current.files,
        current,
        'Sans titre',
        values,
      );
      notes.edit(result.changes);
      openPage(result.page.id);
    } catch (error) {
      report(error);
    }
  }
  function archive(row: DatabaseRow) {
    const workspace = getWorkspace(notes.filesRef.current.files, workspaceId);
    if (workspace) notes.edit(trashPage(workspace, row.pageId));
  }
  function drop(rowId: string, groupId: string) {
    const current = getDatabase(
      notes.filesRef.current.files,
      workspaceId,
      databaseId,
    );
    const column = current?.columns.find(
      (column) => column.id === view.groupBy,
    );
    if (
      !current ||
      !column ||
      !['select', 'checkbox', 'text'].includes(column.type)
    )
      return;
    const value =
      column.type === 'checkbox' ? groupId === 'true' : groupId || null;
    notes.edit([
      change({
        ...current,
        rows: current.rows.map((row) =>
          row.id === rowId
            ? { ...row, values: { ...row.values, [column.id]: value } }
            : row,
        ),
      }),
    ]);
  }
  function card(row: DatabaseRow, draggable = false) {
    return (
      <div
        className="note-card"
        key={row.id}
        draggable={draggable}
        onDragStart={(event) => {
          event.dataTransfer.setData(
            'application/x-noosnote-row',
            `${databaseId}:${row.id}`,
          );
          event.dataTransfer.effectAllowed = 'move';
        }}
      >
        <div className="note-card-heading">
          {draggable && <GripVertical size={14} />}
          <button onClick={() => openPage(row.pageId)}>
            {rowPageTitle(notes.files, database!, row)}
          </button>
          <button
            aria-label="Mettre l’élément à la corbeille"
            className="note-card-trash"
            onClick={() => archive(row)}
          >
            <Trash2 size={13} />
          </button>
        </div>
        {columns
          .filter((column) => column.type !== 'title')
          .map((column) => {
            const result = reader.read(database!, row, column);
            const label = reader.label(column, result.value);
            return (
              <div className="note-card-property" key={column.id}>
                <span>{column.name}</span>
                <span title={result.error}>
                  {result.error ||
                    (Array.isArray(label)
                      ? label.join(', ')
                      : String(label ?? '—'))}
                </span>
              </div>
            );
          })}
        <details className="note-card-edit">
          <summary>Modifier les propriétés</summary>
          {columns
            .filter((column) => column.type !== 'title')
            .map((column) => (
              <label key={column.id}>
                {column.name}
                <Cell database={database!} row={row} column={column} />
              </label>
            ))}
        </details>
      </div>
    );
  }
  function table(items: DatabaseRow[]) {
    return (
      <div className="note-table-scroll">
        <table className="note-table">
          <thead>
            <tr>
              {columns.map((column) => (
                <th
                  key={column.id}
                  style={{
                    minWidth:
                      column.type === 'title' ? 210 : (column.width ?? 155),
                  }}
                >
                  <button onClick={() => setProperty(column)}>
                    {column.name}
                    <Settings2 size={12} />
                  </button>
                </th>
              ))}
              <th>
                <button
                  aria-label="Ajouter une propriété"
                  disabled={database!.columns.length >= 64}
                  onClick={() =>
                    setProperty({ id: id(), name: 'Propriété', type: 'text' })
                  }
                >
                  <Plus size={15} />
                </button>
              </th>
            </tr>
          </thead>
          <tbody>
            {items.map((row) => (
              <tr key={row.id}>
                {columns.map((column) => (
                  <td key={column.id}>
                    <Cell database={database!} row={row} column={column} />
                  </td>
                ))}
                <td>
                  <button
                    className="note-row-trash"
                    aria-label={`Mettre ${rowPageTitle(notes.files, database!, row)} à la corbeille`}
                    onClick={() => archive(row)}
                  >
                    <Trash2 size={14} />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }
  function calendar() {
    if (!view.dateProperty)
      return (
        <div className="note-empty">
          Choisis une propriété Date dans les réglages de cette vue.
          <button onClick={() => setEditingView(view)}>
            Configurer le calendrier
          </button>
        </div>
      );
    const start = new Date(month);
    start.setDate(1 - ((month.getDay() + 6) % 7));
    const days = Array.from({ length: 42 }, (_, offset) => {
      const day = new Date(start);
      day.setDate(start.getDate() + offset);
      return day;
    });
    const iso = (date: Date) =>
      `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
    const undated = shown.filter((row) => !row.values[view.dateProperty!]);
    return (
      <>
        <div className="note-calendar-navigation">
          <button
            aria-label="Mois précédent"
            onClick={() =>
              setMonth(new Date(month.getFullYear(), month.getMonth() - 1, 1))
            }
          >
            <ChevronLeft size={16} />
          </button>
          <strong>
            {month.toLocaleDateString('fr', { month: 'long', year: 'numeric' })}
          </strong>
          <button
            aria-label="Mois suivant"
            onClick={() =>
              setMonth(new Date(month.getFullYear(), month.getMonth() + 1, 1))
            }
          >
            <ChevronRight size={16} />
          </button>
        </div>
        <div className="note-calendar">
          {['Lun', 'Mar', 'Mer', 'Jeu', 'Ven', 'Sam', 'Dim'].map((day) => (
            <div className="note-calendar-dayname" key={day}>
              {day}
            </div>
          ))}
          {days.map((day) => (
            <div
              key={iso(day)}
              className={
                day.getMonth() === month.getMonth()
                  ? 'note-calendar-day'
                  : 'note-calendar-day note-other-month'
              }
            >
              <div>
                <span>{day.getDate()}</span>
                <button
                  aria-label={`Ajouter le ${day.toLocaleDateString('fr')}`}
                  onClick={() => addRow(undefined, iso(day))}
                >
                  <Plus size={12} />
                </button>
              </div>
              {shown
                .filter((row) => row.values[view.dateProperty!] === iso(day))
                .map((row) => (
                  <button
                    key={row.id}
                    className="note-calendar-event"
                    onClick={() => openPage(row.pageId)}
                  >
                    {rowPageTitle(notes.files, database!, row)}
                  </button>
                ))}
            </div>
          ))}
        </div>
        {undated.length > 0 && (
          <details className="note-undated">
            <summary>Sans date · {undated.length}</summary>
            {undated.map((row) => card(row))}
          </details>
        )}
      </>
    );
  }
  return (
    <section className="note-database" aria-label={`Table ${database.title}`}>
      <div className="note-database-heading">
        <input
          aria-label="Nom de la table"
          maxLength={512}
          value={database.title}
          onChange={(event) => update({ title: event.target.value })}
        />
        <div>
          <button
            title="Exporter la table en CSV"
            aria-label="Exporter la table en CSV"
            onClick={() => {
              const quote = (value: CellValue) =>
                `"${String(value ?? '').replaceAll('"', '""')}"`;
              downloadFile(
                `${database.title || 'table'}.csv`,
                [
                  database.columns
                    .map((column) => quote(column.name))
                    .join(','),
                  ...rows.map((row) =>
                    database.columns
                      .map((column) => {
                        const value = reader.label(
                          column,
                          reader.read(database, row, column).value,
                        );
                        return quote(
                          Array.isArray(value) ? value.join(', ') : value,
                        );
                      })
                      .join(','),
                  ),
                ].join('\r\n'),
                'text/csv;charset=utf-8',
              );
            }}
          >
            <Download size={15} />
          </button>
          <button className="note-primary" onClick={() => addRow()}>
            <Plus size={15} /> Nouvel élément
          </button>
        </div>
      </div>
      <div className="note-view-bar">
        <div className="note-view-tabs">
          {database.views.map((item) => {
            const Icon = LAYOUTS.find(
              (layout) => layout.id === item.layout,
            )!.icon;
            return (
              <button
                key={item.id}
                aria-pressed={item.id === view.id}
                onClick={() => {
                  setSelectedView(item.id);
                  setLimit(100);
                }}
              >
                <Icon size={14} />
                {item.name}
              </button>
            );
          })}
          <button
            aria-label="Ajouter une vue"
            disabled={database.views.length >= 24}
            onClick={() => {
              const next = createView('Nouvelle vue');
              setEditingView(next);
            }}
          >
            <Plus size={14} />
          </button>
        </div>
        <label className="note-table-search">
          <Search size={14} />
          <input
            aria-label="Rechercher dans la table"
            placeholder="Rechercher…"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setLimit(100);
            }}
          />
        </label>
        <button
          aria-label="Réglages de la vue"
          title="Filtres, tri et propriétés"
          onClick={() => setEditingView(view)}
        >
          <Settings2 size={16} />
          {view.filters.length > 0 && <small>{view.filters.length}</small>}
        </button>
      </div>
      {view.layout === 'calendar' ? (
        calendar()
      ) : view.layout === 'board' ? (
        <div className="note-board">
          {groups.map((group) => (
            <div
              key={group.id}
              className="note-board-column"
              onDragOver={(event) => {
                if (
                  event.dataTransfer.types.includes(
                    'application/x-noosnote-row',
                  )
                )
                  event.preventDefault();
              }}
              onDrop={(event) => {
                const [database, row] = event.dataTransfer
                  .getData('application/x-noosnote-row')
                  .split(':');
                if (database === databaseId) {
                  event.preventDefault();
                  drop(row, group.id);
                }
              }}
            >
              <h4>
                <i style={{ background: group.color || '#8e8e93' }} />
                {group.name}
                <span>{group.rows.length}</span>
              </h4>
              {group.rows.map((row) => card(row, true))}
              <button className="note-add-row" onClick={() => addRow(group.id)}>
                <Plus size={14} /> Ajouter
              </button>
            </div>
          ))}
        </div>
      ) : (
        groups.map((group) => (
          <div className="note-row-group" key={group.id}>
            {view.groupBy && (
              <div className="note-group-heading">
                <i style={{ background: group.color || '#8e8e93' }} />
                <strong>{group.name}</strong>
                <span>{group.rows.length}</span>
                <button
                  aria-label={`Ajouter dans ${group.name}`}
                  onClick={() => addRow(group.id)}
                >
                  <Plus size={14} />
                </button>
              </div>
            )}
            {view.layout === 'table' ? (
              table(group.rows)
            ) : view.layout === 'gallery' ? (
              <div className="note-gallery">
                {group.rows.map((row) => card(row))}
              </div>
            ) : (
              <div className="note-list">
                {group.rows.map((row) => card(row))}
              </div>
            )}
            <button className="note-add-row" onClick={() => addRow(group.id)}>
              <Plus size={14} /> Nouvel élément
            </button>
          </div>
        ))
      )}
      {rows.length === 0 && (
        <p className="note-muted note-table-empty">
          {query || view.filters.length
            ? 'Aucun élément ne correspond à cette vue.'
            : 'Ajoute ton premier élément. Sa page sera vierge.'}
        </p>
      )}
      <div className="note-database-footer">
        <span>
          {rows.length} élément{rows.length !== 1 ? 's' : ''}
        </span>
        {rows.length > limit && (
          <button onClick={() => setLimit(limit + 100)}>
            Afficher 100 éléments supplémentaires
          </button>
        )}
        <button
          onClick={() =>
            setProperty({
              id: id(),
              name: 'Catégorie',
              type: 'select',
              options: [],
            })
          }
          disabled={database.columns.length >= 64}
        >
          <Plus size={13} /> Propriété
        </button>
      </div>
      {property && (
        <PropertyEditor
          key={property.id}
          database={database}
          source={property}
          onClose={() => setProperty(null)}
        />
      )}
      {editingView && (
        <ViewEditor
          key={editingView.id}
          database={database}
          source={editingView}
          onClose={() => setEditingView(null)}
        />
      )}
    </section>
  );
}
