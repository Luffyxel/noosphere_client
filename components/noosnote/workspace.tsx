import { useEffect, useState } from 'react';
import {
  ArrowLeft,
  Check,
  ChevronDown,
  ChevronRight,
  Cloud,
  CloudOff,
  Copy,
  Download,
  ExternalLink,
  FilePlus2,
  FolderInput,
  Loader2,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Search,
  Settings2,
  ShieldCheck,
  Trash2,
} from 'lucide-react';
import type { NoosnoteController } from '@/lib/noosnote/use-noosnote';
import type { NoteConflict, Workspace } from '@/lib/noosnote/types';
import {
  applyFiles,
  breadcrumbs,
  change,
  createPage,
  createWorkspace,
  descendants,
  duplicatePage,
  getPage,
  getWorkspace,
  movePage,
  NOTE_COLORS,
  NOTE_ICONS,
  pagePath,
  pageText,
  renamePage,
  restorePage,
  searchPages,
  trashPage,
  workspaces,
} from '@/lib/noosnote/model';
import { downloadFile, errorMessage, NoteContext, NoteDialog } from './context';
import { NoteEditor } from './editor';

export function NoosnoteRail({
  notes,
  selected,
  onSelect,
}: {
  notes: NoosnoteController;
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="note-rail-list">
      {workspaces(notes.files).map((workspace) => (
        <button
          key={workspace.id}
          className="note-rail-icon"
          title={workspace.title}
          aria-label={`Noosnote ${workspace.title}`}
          aria-current={selected === workspace.id ? 'page' : undefined}
          style={{
            background:
              selected === workspace.id ? `${workspace.color}35` : undefined,
            borderColor:
              selected === workspace.id ? workspace.color : undefined,
          }}
          onClick={() => onSelect(workspace.id)}
        >
          <span>{workspace.icon}</span>
        </button>
      ))}
    </div>
  );
}

export function CreateNoosnoteDialog({
  notes,
  open,
  onClose,
  onCreated,
}: {
  notes: NoosnoteController;
  open: boolean;
  onClose: () => void;
  onCreated: (workspaceId: string, pageId: string) => void;
}) {
  const [title, setTitle] = useState('Mon Noosnote');
  const [icon, setIcon] = useState('📝');
  const [color, setColor] = useState(NOTE_COLORS[0]);
  const [template, setTemplate] = useState<'blank' | 'school'>('blank');
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [stage, setStage] = useState(notes.repository ? 'ready' : 'initial');
  async function create() {
    setBusy(true);
    setError('');
    try {
      if (!notes.repository || notes.setupStage !== 'ready') {
        const state = await notes.setup();
        setStage(state);
        if (state !== 'ready') return;
      }
      const created = createWorkspace(title, icon, color, template);
      notes.edit(created.changes);
      await notes.flush();
      onCreated(created.workspaceId, created.pageId);
      onClose();
      void notes.synchronize().catch(() => {});
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function openGitHub(mode: 'repository' | 'installation') {
    try {
      await window.noosphereDesktop!.noosnote.openSetup(mode);
    } catch (error) {
      setError(errorMessage(error));
    }
  }
  return (
    <NoteDialog
      title="Nouveau Noosnote"
      open={open}
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <form
        className="note-form"
        onSubmit={(event) => {
          event.preventDefault();
          void create();
        }}
      >
        <label>
          Nom
          <input
            maxLength={512}
            placeholder="École, projets, idées…"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
          />
        </label>
        <div className="note-appearance">
          <div>
            <span>Icône</span>
            <div className="note-icon-grid">
              {NOTE_ICONS.map((value) => (
                <button
                  key={value}
                  type="button"
                  aria-label={`Icône ${value}`}
                  aria-pressed={icon === value}
                  onClick={() => setIcon(value)}
                >
                  {value}
                </button>
              ))}
            </div>
          </div>
          <div>
            <span>Couleur</span>
            <div className="note-color-grid">
              {NOTE_COLORS.map((value) => (
                <button
                  key={value}
                  type="button"
                  aria-label={`Couleur ${value}`}
                  aria-pressed={color === value}
                  style={{ background: value }}
                  onClick={() => setColor(value)}
                >
                  {color === value && <Check size={13} />}
                </button>
              ))}
            </div>
          </div>
        </div>
        <fieldset>
          <legend>Pour commencer</legend>
          <div className="note-templates">
            <button
              type="button"
              aria-pressed={template === 'blank'}
              onClick={() => setTemplate('blank')}
            >
              <span>📝</span>
              <strong>Page vierge</strong>
              <small>Tout créer à ta façon</small>
            </button>
            <button
              type="button"
              aria-pressed={template === 'school'}
              onClick={() => {
                setTemplate('school');
                setTitle((current) =>
                  current === 'Mon Noosnote' ? 'École' : current,
                );
                setIcon('📚');
              }}
            >
              <span>📚</span>
              <strong>École</strong>
              <small>Cours → Anglais et Français</small>
            </button>
          </div>
        </fieldset>
        <div className="note-private-info">
          <ShieldCheck size={21} />
          <div>
            <strong>Un seul dépôt privé pour toutes tes notes</strong>
            <p>
              {notes.repository
                ? `Le dépôt ${notes.repository.name} sera réutilisé.`
                : 'Le premier Noosnote crée un dépôt privé dans ton compte GitHub. Les suivants y auront chacun leur dossier.'}{' '}
              Tes autres machines retrouveront les mêmes notes avec ce compte.
            </p>
          </div>
        </div>
        {!notes.repository && (
          <label className="note-consent">
            <input
              type="checkbox"
              checked={consent}
              onChange={(event) => setConsent(event.target.checked)}
            />
            <span>
              J’accepte de créer ce dépôt privé et d’y autoriser Noosphere.
            </span>
          </label>
        )}
        {(stage === 'repository' || stage === 'installation') && (
          <div className="note-setup-steps">
            <strong>Terminer sur GitHub</strong>
            <p>
              L’autorisation actuelle de Noosphere ne permet pas de créer ou de
              lire ce nouveau dépôt automatiquement.
            </p>
            {stage === 'repository' && (
              <button
                type="button"
                onClick={() => void openGitHub('repository')}
              >
                <ExternalLink size={15} /> 1. Créer le dépôt privé
              </button>
            )}
            <button
              type="button"
              onClick={() => void openGitHub('installation')}
            >
              <ExternalLink size={15} /> {stage === 'repository' ? '2.' : '1.'}{' '}
              Ajouter le dépôt à l’installation Noosphere
            </button>
            <p className="note-muted">
              Garde « Only select repositories » et sélectionne aussi le dépôt
              de notes. Reviens ici, puis clique sur « Vérifier et créer ».
            </p>
          </div>
        )}
        {error && (
          <p role="alert" className="note-error">
            {error}
          </p>
        )}
        <div className="note-actions">
          <button type="button" onClick={onClose} disabled={busy}>
            Annuler
          </button>
          <button
            type="submit"
            className="note-primary"
            disabled={busy || !title.trim() || (!notes.repository && !consent)}
          >
            {busy ? (
              <>
                <Loader2 size={15} className="animate-spin" /> Préparation…
              </>
            ) : stage === 'repository' || stage === 'installation' ? (
              'Vérifier et créer'
            ) : (
              <>
                <Plus size={15} /> Créer le Noosnote
              </>
            )}
          </button>
        </div>
      </form>
    </NoteDialog>
  );
}

function ConflictDialog({
  conflicts,
  notes,
  onClose,
}: {
  conflicts: NoteConflict[];
  notes: NoosnoteController;
  onClose: () => void;
}) {
  const [selected, setSelected] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const conflict = conflicts[Math.min(selected, conflicts.length - 1)];
  if (!conflict) return null;
  const title =
    conflict.local?.title || conflict.remote?.title || 'Page supprimée';
  const summary = (value: NoteConflict['local']) =>
    !value
      ? 'Supprimée'
      : value.kind === 'page'
        ? pageText(value.content) || 'Page vierge'
        : JSON.stringify(value, null, 2);
  return (
    <NoteDialog title="Choisir la version à garder" open onClose={onClose}>
      <p className="note-muted">
        Cette modification et celle d’une autre machine touchent le même
        contenu. Les deux versions restent conservées jusqu’à ton choix.
      </p>
      <div className="note-conflict-heading">
        <strong>{title}</strong>
        <select
          aria-label="Conflit à résoudre"
          value={Math.min(selected, conflicts.length - 1)}
          onChange={(event) => setSelected(Number(event.target.value))}
        >
          {conflicts.map((conflict, index) => (
            <option key={conflict.id} value={index}>
              {index + 1}.{' '}
              {conflict.local?.title || conflict.remote?.title || 'Suppression'}
            </option>
          ))}
        </select>
      </div>
      <div className="note-conflict-versions">
        {(['local', 'remote'] as const).map((choice) => (
          <div key={choice}>
            <strong>{choice === 'local' ? 'Cette machine' : 'GitHub'}</strong>
            <pre>{summary(conflict[choice])}</pre>
            <button
              className="note-primary"
              disabled={busy}
              onClick={() => {
                setBusy(true);
                void notes
                  .resolve(conflict.id, choice)
                  .then(() => {
                    if (conflicts.length === 1) onClose();
                  })
                  .catch((error) => setError(errorMessage(error)))
                  .finally(() => setBusy(false));
              }}
            >
              Garder cette version
            </button>
          </div>
        ))}
      </div>
      <button
        className="note-export-conflict"
        onClick={() =>
          downloadFile(
            `${title}-versions.json`,
            JSON.stringify(conflict, null, 2),
          )
        }
      >
        <Download size={15} /> Télécharger les deux versions
      </button>
      {error && (
        <p role="alert" className="note-error">
          {error}
        </p>
      )}
    </NoteDialog>
  );
}

function PageTree({
  workspace,
  selected,
  openPage,
  parent = null,
  depth = 0,
}: {
  workspace: Workspace;
  selected: string | null;
  openPage: (id: string) => void;
  parent?: string | null;
  depth?: number;
}) {
  const [closed, setClosed] = useState(new Set<string>());
  if (depth > 48) return null;
  const pages = workspace.pages.filter(
    (page) => page.parentId === parent && !page.trashedAt,
  );
  return (
    <div>
      {pages.map((page) => {
        const hasChildren = workspace.pages.some(
          (child) => child.parentId === page.id && !child.trashedAt,
        );
        return (
          <div key={page.id}>
            <div
              className="note-tree-row"
              aria-current={selected === page.id ? 'page' : undefined}
              style={{ paddingLeft: 8 + Math.min(depth, 10) * 12 }}
            >
              <button
                className="note-tree-toggle"
                aria-label={`${closed.has(page.id) ? 'Déplier' : 'Replier'} ${page.title}`}
                disabled={!hasChildren}
                onClick={() =>
                  setClosed((current) => {
                    const next = new Set(current);
                    if (next.has(page.id)) next.delete(page.id);
                    else next.add(page.id);
                    return next;
                  })
                }
              >
                {hasChildren ? (
                  closed.has(page.id) ? (
                    <ChevronRight size={13} />
                  ) : (
                    <ChevronDown size={13} />
                  )
                ) : (
                  <span />
                )}
              </button>
              <button
                className="note-tree-page"
                onClick={() => openPage(page.id)}
              >
                <span>{page.icon || (page.databaseId ? '◻' : '📄')}</span>
                <span>{page.title || 'Sans titre'}</span>
              </button>
            </div>
            {hasChildren && !closed.has(page.id) && (
              <PageTree
                workspace={workspace}
                selected={selected}
                openPage={openPage}
                parent={page.id}
                depth={depth + 1}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

export function NoosnoteWorkspace({
  notes,
  workspaceId,
  initialPageId,
  onCreate,
}: {
  notes: NoosnoteController;
  workspaceId: string | null;
  initialPageId: string | null;
  onCreate: () => void;
}) {
  const [selectedPageId, setPageId] = useState<string | null>(initialPageId);
  const [history, setHistory] = useState<string[]>([]);
  const [query, setQuery] = useState('');
  const [trash, setTrash] = useState(false);
  const [error, setError] = useState('');
  const [pageLoading, setPageLoading] = useState(false);
  const [actions, setActions] = useState(false);
  const [moving, setMoving] = useState(false);
  const [conflictsOpen, setConflictsOpen] = useState(false);
  const [settings, setSettings] = useState(false);
  const workspace = workspaceId
    ? getWorkspace(notes.files, workspaceId)
    : undefined;
  const pageId = workspace?.pages.some(
    (entry) => entry.id === selectedPageId && !entry.trashedAt,
  )
    ? selectedPageId
    : (workspace?.pages.find((entry) => !entry.parentId && !entry.trashedAt)
        ?.id ??
      workspace?.pages.find((entry) => !entry.trashedAt)?.id ??
      null);
  const page =
    workspaceId && pageId
      ? getPage(notes.files, workspaceId, pageId)
      : undefined;
  const entry = workspace?.pages.find((entry) => entry.id === pageId);
  const report = (error: unknown) => setError(errorMessage(error));
  function openPage(id: string) {
    if (id !== pageId) {
      if (pageId) setHistory((current) => [...current.slice(-49), pageId]);
      setPageId(id);
    }
    setTrash(false);
    setActions(false);
    setError('');
  }
  const loadPage = notes.loadPage;
  useEffect(() => {
    if (!workspaceId || !pageId) return;
    let active = true;
    queueMicrotask(() => {
      if (!active) return;
      setPageLoading(true);
      setError('');
      void loadPage(workspaceId, pageId)
        .catch((error) => {
          if (active) setError(errorMessage(error));
        })
        .finally(() => {
          if (active) setPageLoading(false);
        });
    });
    return () => {
      active = false;
    };
  }, [workspaceId, pageId, loadPage]);
  const context = { notes, workspaceId: workspaceId ?? '', openPage, report };
  async function duplicate() {
    if (!workspace || !pageId) return;
    setActions(false);
    setPageLoading(true);
    try {
      await notes.flush();
      let files = { ...notes.filesRef.current.files };
      const ids = descendants(workspace, pageId);
      if (ids.size > 200)
        throw new Error('Duplique au maximum 200 pages à la fois.');
      for (const id of ids) {
        const document = await notes.loadPage(workspace.id, id);
        if (document)
          files = applyFiles(files, [
            { path: pagePath(workspace.id, id), value: document },
          ]);
      }
      const result = duplicatePage(files, workspace.id, pageId);
      if (result.changes.length > 250)
        throw new Error(
          'Cette copie contient trop de tables. Duplique les sous-pages séparément.',
        );
      notes.edit(result.changes);
      await notes.flush();
      openPage(result.pageId);
    } catch (error) {
      report(error);
    } finally {
      setPageLoading(false);
    }
  }
  function addPage(parent: string | null = null) {
    if (!workspace) return;
    try {
      const result = createPage(
        notes.filesRef.current.files,
        workspace.id,
        parent,
      );
      notes.edit(result.changes);
      openPage(result.page.id);
    } catch (error) {
      report(error);
    }
  }
  const status = notes.syncing
    ? 'Synchronisation…'
    : notes.saving
      ? 'Enregistrement…'
      : notes.conflicts.length
        ? 'Versions à choisir'
        : notes.dirtyCount
          ? 'Enregistré ici · synchronisation en attente'
          : notes.lastSyncedAt
            ? `Synchronisé à ${new Date(notes.lastSyncedAt).toLocaleTimeString('fr', { hour: '2-digit', minute: '2-digit' })}`
            : 'Enregistré sur cette machine';
  return (
    <NoteContext.Provider value={context}>
      <section className="noosnote-workspace">
        <aside className="note-sidebar">
          <header>
            <span>{workspace?.icon || '📝'}</span>
            <strong>{workspace?.title || 'Noosnote'}</strong>
            <button
              aria-label="Réglages du Noosnote"
              disabled={!workspace}
              onClick={() => setSettings(true)}
            >
              <Settings2 size={16} />
            </button>
          </header>
          <label className="note-search">
            <Search size={15} />
            <input
              aria-label="Rechercher une page"
              placeholder="Rechercher une page…"
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setTrash(false);
              }}
            />
          </label>
          <div className="note-sidebar-label">
            <span>Pages</span>
            <button
              aria-label="Nouvelle page"
              onClick={() => addPage()}
              disabled={!workspace}
            >
              <Plus size={15} />
            </button>
          </div>
          <div className="note-page-tree">
            {workspace &&
              (query ? (
                searchPages(notes.files, workspace, query).map((entry) => (
                  <button
                    className="note-search-result"
                    key={entry.id}
                    onClick={() => openPage(entry.id)}
                  >
                    {entry.icon || '📄'} {entry.title || 'Sans titre'}
                  </button>
                ))
              ) : (
                <PageTree
                  workspace={workspace}
                  selected={pageId}
                  openPage={openPage}
                />
              ))}
            {workspace && workspace.pages.every((page) => !!page.trashedAt) && (
              <button className="note-add-row" onClick={() => addPage()}>
                <FilePlus2 size={15} /> Créer une page
              </button>
            )}
          </div>
          <div className="note-sidebar-bottom">
            <button
              aria-pressed={trash}
              onClick={() => {
                setTrash(!trash);
                setQuery('');
              }}
            >
              <Trash2 size={15} /> Corbeille{' '}
              {workspace &&
                workspace.pages.filter((page) => page.trashedAt).length > 0 && (
                  <span>
                    {workspace.pages.filter((page) => page.trashedAt).length}
                  </span>
                )}
            </button>
            <button onClick={onCreate}>
              <Plus size={15} /> Nouveau Noosnote
            </button>
            <div className="note-sync-status" title={status}>
              {notes.syncing || notes.saving ? (
                <Loader2 size={13} className="animate-spin" />
              ) : notes.error ? (
                <CloudOff size={13} />
              ) : (
                <Cloud size={13} />
              )}
              <span>{status}</span>
              <button
                aria-label="Synchroniser les notes"
                title="Synchroniser · Ctrl+S"
                disabled={notes.syncing}
                onClick={() => void notes.synchronize().catch(() => {})}
              >
                <RefreshCw size={14} />
              </button>
            </div>
            {notes.repository && (
              <a
                href={notes.repository.url}
                target="_blank"
                rel="noreferrer"
                className="note-repository-link"
              >
                <ShieldCheck size={12} /> Dépôt privé <ExternalLink size={11} />
              </a>
            )}
          </div>
        </aside>
        <div className="note-main">
          <header className="note-topbar">
            <button
              aria-label="Page précédente"
              disabled={!history.length}
              onClick={() => {
                const previous = history.at(-1);
                if (previous) {
                  setPageId(previous);
                  setHistory(history.slice(0, -1));
                  setTrash(false);
                }
              }}
            >
              <ArrowLeft size={16} />
            </button>
            <nav aria-label="Fil d’Ariane">
              {workspace &&
                pageId &&
                breadcrumbs(workspace, pageId).map((entry, index) => (
                  <span key={entry.id}>
                    {index > 0 && <ChevronRight size={12} />}
                    <button onClick={() => openPage(entry.id)}>
                      {entry.title || 'Sans titre'}
                    </button>
                  </span>
                ))}
            </nav>
            {page && !trash && (
              <div className="note-page-actions">
                <button
                  aria-label="Options de la page"
                  aria-expanded={actions}
                  onClick={() => setActions(!actions)}
                >
                  <MoreHorizontal size={19} />
                </button>
                {actions && (
                  <div className="note-action-menu">
                    <button onClick={() => addPage(pageId)}>
                      <FilePlus2 size={15} /> Ajouter une sous-page
                    </button>
                    <button onClick={() => void duplicate()}>
                      <Copy size={15} /> Dupliquer avec les sous-pages
                    </button>
                    <button
                      disabled={!!entry?.databaseId}
                      onClick={() => {
                        setMoving(true);
                        setActions(false);
                      }}
                    >
                      <FolderInput size={15} /> Déplacer
                    </button>
                    <button
                      onClick={() => {
                        downloadFile(
                          `${page.title || 'page'}.json`,
                          JSON.stringify(page, null, 2),
                        );
                        setActions(false);
                      }}
                    >
                      <Download size={15} /> Exporter la page
                    </button>
                    <button
                      className="note-danger"
                      onClick={() => {
                        notes.edit(trashPage(workspace!, page.id));
                        setActions(false);
                      }}
                    >
                      <Trash2 size={15} /> Mettre à la corbeille
                    </button>
                  </div>
                )}
              </div>
            )}
          </header>
          {(error || notes.error) && (
            <div className="note-error-banner" role="alert">
              {error || notes.error}
              <button
                onClick={() => {
                  setError('');
                  void notes.synchronize().catch(() => {});
                }}
              >
                Réessayer
              </button>
            </div>
          )}
          {notes.conflicts.length > 0 && (
            <div className="note-conflict-banner">
              <span>
                {notes.conflicts.length} modification
                {notes.conflicts.length > 1 ? 's' : ''} demande
                {notes.conflicts.length > 1 ? 'nt' : ''} un choix de version.
              </span>
              <button onClick={() => setConflictsOpen(true)}>
                Comparer les versions
              </button>
            </div>
          )}
          <div className="note-page-scroll">
            {notes.loading ? (
              <div className="note-welcome">
                <Loader2 className="animate-spin" />
                <p>Ouverture des notes…</p>
              </div>
            ) : trash && workspace ? (
              <div className="note-trash">
                <h1>Corbeille</h1>
                <p className="note-muted">
                  Les pages restent disponibles pour être restaurées.
                </p>
                {workspace.pages
                  .filter((page) => page.trashedAt)
                  .map((page) => (
                    <div key={page.id}>
                      <span>
                        {page.icon || '📄'} {page.title}
                      </span>
                      <button
                        onClick={() =>
                          notes.edit(restorePage(workspace, page.id))
                        }
                      >
                        Restaurer
                      </button>
                    </div>
                  ))}
              </div>
            ) : page && !entry?.trashedAt ? (
              <article className="note-page">
                <div className="note-page-icon">
                  <select
                    aria-label="Icône de la page"
                    value={page.icon}
                    onChange={(event) =>
                      notes.edit(
                        renamePage(
                          notes.filesRef.current.files,
                          workspace!.id,
                          page.id,
                          page.title,
                          event.target.value,
                        ),
                      )
                    }
                  >
                    <option value="">📄</option>
                    {NOTE_ICONS.map((icon) => (
                      <option key={icon} value={icon}>
                        {icon}
                      </option>
                    ))}
                  </select>
                </div>
                <input
                  className="note-page-title"
                  aria-label="Titre de la page"
                  placeholder="Sans titre"
                  maxLength={512}
                  value={page.title}
                  onChange={(event) =>
                    notes.edit(
                      renamePage(
                        notes.filesRef.current.files,
                        workspace!.id,
                        page.id,
                        event.target.value,
                      ),
                    )
                  }
                />
                <NoteEditor key={page.id} page={page} />
              </article>
            ) : pageLoading ? (
              <div className="note-welcome">
                <Loader2 className="animate-spin" />
                <p>Chargement de la page…</p>
              </div>
            ) : (
              <div className="note-welcome">
                <div className="note-welcome-icon">📝</div>
                <h1>
                  {workspace ? workspace.title : 'Tes idées, au même endroit.'}
                </h1>
                <p>
                  Des pages, des cours et des tables qui se retrouvent sur tes
                  machines.
                </p>
                <button
                  className="note-primary"
                  onClick={() => (workspace ? addPage() : onCreate())}
                >
                  <Plus size={16} />
                  {workspace ? 'Créer une page' : 'Créer mon premier Noosnote'}
                </button>
              </div>
            )}
          </div>
        </div>
        {conflictsOpen && notes.conflicts.length > 0 && (
          <ConflictDialog
            conflicts={notes.conflicts}
            notes={notes}
            onClose={() => setConflictsOpen(false)}
          />
        )}
        {moving && workspace && pageId && (
          <NoteDialog
            title="Déplacer la page"
            open
            onClose={() => setMoving(false)}
          >
            <div className="note-move-list">
              {[
                { id: '', title: 'Racine du Noosnote', icon: '📁' },
                ...workspace.pages.filter(
                  (entry) =>
                    !entry.trashedAt &&
                    !descendants(workspace, pageId).has(entry.id),
                ),
              ].map((entry) => (
                <button
                  key={entry.id}
                  onClick={() => {
                    try {
                      notes.edit(movePage(workspace, pageId, entry.id || null));
                      setMoving(false);
                    } catch (error) {
                      report(error);
                    }
                  }}
                >
                  {entry.icon || '📄'} {entry.title}
                </button>
              ))}
            </div>
          </NoteDialog>
        )}
        {settings && workspace && (
          <WorkspaceSettings
            workspace={workspace}
            notes={notes}
            onClose={() => setSettings(false)}
          />
        )}
      </section>
    </NoteContext.Provider>
  );
}

function WorkspaceSettings({
  workspace,
  notes,
  onClose,
}: {
  workspace: Workspace;
  notes: NoosnoteController;
  onClose: () => void;
}) {
  const [title, setTitle] = useState(workspace.title);
  const [icon, setIcon] = useState(workspace.icon);
  const [color, setColor] = useState(workspace.color);
  return (
    <NoteDialog title="Personnaliser le Noosnote" open onClose={onClose}>
      <form
        className="note-form"
        onSubmit={(event) => {
          event.preventDefault();
          const current = getWorkspace(
            notes.filesRef.current.files,
            workspace.id,
          );
          if (current)
            notes.edit([
              change({
                ...current,
                title: title.trim() || 'Mon Noosnote',
                icon,
                color,
              }),
            ]);
          onClose();
        }}
      >
        <label>
          Nom
          <input
            maxLength={512}
            value={title}
            onChange={(event) => setTitle(event.target.value)}
          />
        </label>
        <label>
          Icône
          <select
            value={icon}
            onChange={(event) => setIcon(event.target.value)}
          >
            {NOTE_ICONS.map((icon) => (
              <option key={icon} value={icon}>
                {icon}
              </option>
            ))}
          </select>
        </label>
        <label>
          Couleur
          <input
            type="color"
            value={color}
            onChange={(event) => setColor(event.target.value)}
          />
        </label>
        <div className="note-actions">
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
