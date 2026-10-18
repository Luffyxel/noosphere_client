import { useEffect, useMemo, useRef, useState } from 'react';
import {
  Check,
  CheckCheck,
  Copy,
  ExternalLink,
  History,
  Loader2,
  LockKeyhole,
  MessageCircle,
  Plus,
  Send,
  Users,
  X,
} from 'lucide-react';
import type { GitHubViewer } from '@/lib/desktop-bridge';
import type { NoosnoteController } from '@/lib/noosnote/use-noosnote';
import { useNoosnote } from '@/lib/noosnote/use-noosnote';
import type {
  Team,
  TeamPreparation,
  TeamStatus,
  NoteThread,
  NoteComment,
  NoteRevision,
} from '@/lib/noosnote/team-types';
import { change, getWorkspace, id } from '@/lib/noosnote/model';
import { NoteDialog, errorMessage } from './context';
import { NoosnoteWorkspace } from './workspace';

function api() {
  const value = window.noosphereDesktop?.noosnoteTeams;
  if (!value)
    throw new Error('Le partage nécessite la dernière version de Noosphere.');
  return value;
}
export function useNoosnoteTeams(ownerId: number) {
  const [teams, setTeams] = useState<Team[]>([]);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    void window.noosphereDesktop?.noosnoteTeams
      ?.list()
      .then((teams) => {
        if (active) setTeams(teams);
      })
      .catch((error) => {
        if (active) setError(errorMessage(error));
      });
    return () => {
      active = false;
    };
  }, [ownerId]);
  return {
    teams,
    error,
    remember(team: Team) {
      setTeams((teams) => [...teams.filter((t) => t.id !== team.id), team]);
    },
  };
}
export function SharedNoosnoteRail({
  teams,
  selected,
  onSelect,
  onJoin,
}: {
  teams: Team[];
  selected: string | null;
  onSelect: (id: string) => void;
  onJoin: () => void;
}) {
  return (
    <div className="note-rail-list note-shared-rail">
      {teams.map((team) => (
        <button
          className="note-rail-icon"
          key={team.id}
          title={`${team.title} · partagé`}
          aria-label={`Noosnote partagé ${team.title}`}
          aria-current={selected === team.id ? 'page' : undefined}
          onClick={() => onSelect(team.id)}
        >
          <Users size={21} />
          <span className="note-team-dot" />
        </button>
      ))}
      <button
        className="note-rail-icon note-join-space"
        title="Rejoindre un Noosnote"
        aria-label="Rejoindre un Noosnote"
        onClick={onJoin}
      >
        <Users size={18} />
        <Plus size={10} />
      </button>
    </div>
  );
}
export function JoinNoosnoteDialog({
  onClose,
  onJoined,
}: {
  onClose: () => void;
  onJoined: (team: Team) => void;
}) {
  const [link, setLink] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  return (
    <NoteDialog title="Rejoindre un Noosnote" open onClose={onClose}>
      <div className="note-team-intro">
        <span className="note-team-hero">
          <Users size={27} />
        </span>
        <p>
          Un espace commun pour vos pages, cours et projets. Colle le lien
          envoyé par son propriétaire.
        </p>
      </div>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          setBusy(true);
          setError('');
          void api()
            .join(link)
            .then(onJoined)
            .catch((e) => setError(errorMessage(e)))
            .finally(() => setBusy(false));
        }}
      >
        <label className="note-team-label">
          Lien de l’espace
          <input
            required
            value={link}
            onChange={(e) => setLink(e.target.value)}
            placeholder="https://github.com/…/noosnote_team_…"
          />
        </label>
        <p className="note-muted">
          Accepte d’abord l’invitation GitHub avec le compte connecté à
          Noosphere. Le propriétaire doit autoriser l’application sur ce dépôt
          privé.
        </p>
        {error && (
          <p className="note-error" role="alert">
            {error}
          </p>
        )}
        <button className="note-primary" disabled={busy || !link.trim()}>
          {busy ? (
            <Loader2 className="animate-spin" size={16} />
          ) : (
            <Users size={16} />
          )}{' '}
          Rejoindre l’espace
        </button>
      </form>
    </NoteDialog>
  );
}
export function ShareNoosnote({
  notes,
  workspaceId,
  onShared,
}: {
  notes: NoosnoteController;
  workspaceId: string;
  onShared: (team: Team) => void;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [stage, setStage] = useState<TeamPreparation | null>(null);
  async function prepare() {
    setBusy(true);
    setError('');
    try {
      await notes.flush();
      const result = await api().prepare(workspaceId, true);
      setStage(result);
      if (result.team) {
        await api().notes(result.team).sync();
        onShared(result.team);
        setOpen(false);
      }
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <button className="note-team-share" onClick={() => setOpen(true)}>
        <Users size={15} /> Partager
      </button>
      <NoteDialog
        title="Travailler ensemble"
        open={open}
        onClose={() => {
          if (!busy) setOpen(false);
        }}
      >
        <div className="note-team-intro">
          <span className="note-team-hero">
            <Users size={28} />
          </span>
          <div>
            <h3>Un espace, toute votre équipe</h3>
            <p>
              Écrivez à plusieurs, échangez sur chaque page et retrouvez les
              modifications de chacun.
            </p>
          </div>
        </div>
        <div className="note-team-privacy">
          <LockKeyhole size={18} />
          <p>
            Seul ce Noosnote et ses sous-pages seront partagés dans un dépôt
            privé dédié. Une copie personnelle reste conservée.
          </p>
        </div>
        {stage && stage.stage !== 'ready' && (
          <div className="note-team-setup">
            <strong>
              {stage.stage === 'repository'
                ? 'Créer le dépôt privé'
                : 'Autoriser Noosphere'}
            </strong>
            <p>
              {stage.stage === 'repository'
                ? 'GitHub doit créer le dépôt privé proposé. Ajoute un README pour initialiser le dépôt, puis reviens ici.'
                : 'Sélectionne le dépôt noosnote_team correspondant dans les réglages de l’application GitHub.'}
            </p>
            <button
              className="note-secondary"
              onClick={() =>
                void api()
                  .open(
                    workspaceId,
                    stage.stage === 'repository'
                      ? 'setup-repository'
                      : 'setup-installation',
                  )
                  .catch((error) => setError(errorMessage(error)))
              }
            >
              <ExternalLink size={15} /> Ouvrir GitHub
            </button>
          </div>
        )}
        {error && (
          <p className="note-error" role="alert">
            {error}
          </p>
        )}
        <div className="note-dialog-actions">
          <button
            className="note-secondary"
            disabled={busy}
            onClick={() => setOpen(false)}
          >
            Annuler
          </button>
          <button
            className="note-primary"
            disabled={busy}
            onClick={() => void prepare()}
          >
            {busy ? (
              <Loader2 size={15} className="animate-spin" />
            ) : (
              <Users size={15} />
            )}{' '}
            {busy
              ? 'Préparation de l’espace…'
              : stage
                ? 'Vérifier et continuer'
                : 'Créer l’espace partagé'}
          </button>
        </div>
      </NoteDialog>
    </>
  );
}
export function TeamNoosnote({
  viewer,
  team,
  onCreate,
  onUpdated,
  onBusy,
}: {
  viewer: GitHubViewer;
  team: Team;
  onCreate: () => void;
  onUpdated: (team: Team) => void;
  onBusy?: (busy: boolean) => void;
}) {
  const [status, setStatus] = useState<TeamStatus>({
    team,
    members: [],
    activity: [],
  });
  const teamId = team.id;
  const notesApi = useMemo(
    () => api().notes({ id: teamId, canEdit: true }),
    [teamId],
  );
  const controller = useNoosnote(
    viewer.id,
    true,
    notesApi,
    !status.team.canEdit,
  );
  const notes = { ...controller, canEdit: status.team.canEdit };
  useEffect(() => {
    onBusy?.(controller.saving || controller.syncing);
    return () => onBusy?.(false);
  }, [onBusy, controller.saving, controller.syncing]);
  const updated = useRef(onUpdated);
  useEffect(() => {
    updated.current = onUpdated;
  }, [onUpdated]);
  const [statusError, setStatusError] = useState('');
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function refresh() {
      try {
        const value = await api().status(team.id);
        if (active) {
          setStatus(value);
          setStatusError('');
          updated.current(value.team);
        }
      } catch (error) {
        if (active) setStatusError(errorMessage(error));
      } finally {
        if (active) timer = setTimeout(() => void refresh(), 60000);
      }
    }
    void refresh();
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [team.id]);
  return (
    <NoosnoteWorkspace
      notes={notes}
      workspaceId={team.id}
      initialPageId={null}
      onCreate={onCreate}
      collaboration={(pageId) => (
        <TeamTools
          viewer={viewer}
          status={status}
          statusError={statusError}
          notes={notes}
          pageId={pageId}
        />
      )}
    />
  );
}
function displayName(
  status: TeamStatus,
  notes: NoosnoteController,
  id: number,
  fallback: string,
) {
  return (
    getWorkspace(notes.files, status.team.id)?.profiles?.[String(id)] ||
    fallback
  );
}
function Avatar({ name, id }: { name: string; id: number }) {
  return (
    <span
      className="note-member-avatar"
      style={{ background: `hsl(${(id * 137) % 360} 30% 34%)` }}
      aria-hidden
    >
      {name.slice(0, 2).toLocaleUpperCase()}
    </span>
  );
}
function CommentBody({ body }: { body: string }) {
  return (
    <p className="note-comment-body">
      {body
        .split(/(@[a-z\d](?:[a-z\d-]{0,38})\b)/gi)
        .map((part, i) =>
          part.startsWith('@') ? <mark key={i}>{part}</mark> : part,
        )}
    </p>
  );
}
function TeamTools({
  viewer,
  status,
  statusError,
  notes,
  pageId,
}: {
  viewer: GitHubViewer;
  status: TeamStatus;
  statusError: string;
  notes: NoosnoteController;
  pageId: string | null;
}) {
  const [tab, setTab] = useState<'comments' | 'members' | 'activity' | null>(
    null,
  );
  const [body, setBody] = useState('');
  const [sending, setSending] = useState(false);
  const posting = useRef(false);
  const [replyTo, setReplyTo] = useState<string | null>(null);
  const [filter, setFilter] = useState<'open' | 'resolved' | 'mentions'>(
    'open',
  );
  const [error, setError] = useState('');
  const [copied, setCopied] = useState(false);
  const [revisions, setRevisions] = useState<NoteRevision[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [pseudo, setPseudo] = useState(
    displayName(status, notes, viewer.id, viewer.name || viewer.login),
  );
  const [quote, setQuote] = useState('');
  const threads = Object.values(notes.files).filter(
    (file): file is NoteThread =>
      file.kind === 'thread' && file.pageId === pageId,
  );
  const openCount = threads.filter((t) => !t.resolved).length;
  const shown = threads.filter((t) =>
    filter === 'resolved'
      ? t.resolved
      : !t.resolved &&
        (filter !== 'mentions' ||
          t.comments.some((c) =>
            c.body
              .toLocaleLowerCase()
              .includes(`@${viewer.login.toLocaleLowerCase()}`),
          )),
  );
  const mentions = body.match(/(?:^|\s)@([a-z\d-]*)$/i)?.[1];
  const suggestions =
    mentions !== undefined
      ? status.members
          .filter((m) =>
            m.login.toLocaleLowerCase().includes(mentions.toLocaleLowerCase()),
          )
          .slice(0, 6)
      : [];
  useEffect(() => {
    queueMicrotask(() => {
      setBody('');
      setReplyTo(null);
      setQuote('');
    });
  }, [pageId]);
  useEffect(() => {
    if (tab !== 'activity') return;
    let active = true;
    queueMicrotask(() => {
      if (active) setHistoryLoading(true);
    });
    void api()
      .history(status.team.id, pageId ?? undefined)
      .then((value) => {
        if (active) setRevisions(value);
      })
      .catch((e) => {
        if (active) setError(errorMessage(e));
      })
      .finally(() => {
        if (active) setHistoryLoading(false);
      });
    return () => {
      active = false;
    };
  }, [tab, pageId, status.team.id]);
  async function publishThread(thread: NoteThread) {
    if (
      thread.comments.length > 500 ||
      new TextEncoder().encode(JSON.stringify(thread)).length >= 1024 * 1024
    )
      throw new Error(
        'Cette discussion est pleine. Crée une nouvelle discussion pour continuer.',
      );
    notes.edit([change(thread)]);
    await notes.flush();
    void notes.synchronize().catch((error) => setError(errorMessage(error)));
  }
  function send() {
    if (!pageId || !body.trim() || posting.current) return;
    posting.current = true;
    setSending(true);
    const comment: NoteComment = {
      id: id(),
      body: body.trim(),
      author: {
        id: viewer.id,
        login: viewer.login,
        name: viewer.name || viewer.login,
        avatarUrl: viewer.avatarUrl,
      },
      createdAt: new Date().toISOString(),
    };
    const previous = threads.find((t) => t.id === replyTo);
    const thread: NoteThread = previous
      ? { ...previous, comments: [...previous.comments, comment] }
      : {
          version: 1,
          kind: 'thread',
          id: id(),
          workspaceId: status.team.id,
          pageId,
          title: 'Discussion',
          resolved: false,
          comments: [comment],
          ...(quote ? { quote } : {}),
        };
    void publishThread(thread)
      .then(() => {
        setBody('');
        setReplyTo(null);
        setQuote('');
      })
      .catch((e) => setError(errorMessage(e)))
      .finally(() => {
        posting.current = false;
        setSending(false);
      });
  }
  const name = displayName(
    status,
    notes,
    viewer.id,
    viewer.name || viewer.login,
  );
  return (
    <>
      <div className="note-team-toolbar">
        <button
          className="note-team-avatars"
          aria-label={`${status.members.length} membres de l’espace`}
          onClick={() => setTab(tab === 'members' ? null : 'members')}
        >
          {status.members.slice(0, 3).map((member) => (
            <Avatar
              key={member.id}
              id={member.id}
              name={displayName(status, notes, member.id, member.name)}
            />
          ))}
          <Users size={14} />
        </button>
        <button
          className="note-team-share"
          onClick={() => setTab(tab === 'members' ? null : 'members')}
        >
          <Users size={15} /> Partagé
        </button>
        <button
          aria-label="Discussions de la page"
          aria-expanded={tab === 'comments'}
          onMouseDown={(event) => {
            event.preventDefault();
            const selection = window.getSelection()?.toString().trim();
            if (selection)
              setQuote(Array.from(selection).slice(0, 500).join(''));
          }}
          onClick={() => setTab(tab === 'comments' ? null : 'comments')}
        >
          <MessageCircle size={17} />
          {openCount > 0 && (
            <span className="note-discussion-count">{openCount}</span>
          )}
        </button>
        <button
          aria-label="Historique de la page"
          onClick={() => setTab(tab === 'activity' ? null : 'activity')}
        >
          <History size={17} />
        </button>
      </div>
      {tab && (
        <aside className="note-team-panel" aria-label="Collaboration">
          <header>
            <div className="note-team-tabs">
              {(['comments', 'members', 'activity'] as const).map((item) => (
                <button
                  key={item}
                  aria-pressed={tab === item}
                  onClick={() => setTab(item)}
                >
                  {item === 'comments'
                    ? 'Discussions'
                    : item === 'members'
                      ? 'Membres'
                      : 'Historique'}
                </button>
              ))}
            </div>
            <button
              aria-label="Fermer la collaboration"
              onClick={() => setTab(null)}
            >
              <X size={16} />
            </button>
          </header>
          {error && (
            <p className="note-error" role="alert">
              {error}
            </p>
          )}
          {tab === 'members' ? (
            <div className="note-team-panel-body">
              <div className="note-space-badge">
                <LockKeyhole size={16} /> Espace privé ·{' '}
                {status.team.canEdit ? 'Modification' : 'Lecture seule'}
              </div>
              <h3>Travailler ensemble</h3>
              <p className="note-muted">
                Le lien ouvre cet espace. Seules les personnes invitées sur
                GitHub peuvent y accéder.
              </p>
              <button
                className="note-secondary note-team-full"
                onClick={() => {
                  void navigator.clipboard
                    .writeText(
                      `https://github.com/${status.team.owner}/${status.team.name}`,
                    )
                    .then(() => {
                      setCopied(true);
                      setTimeout(() => setCopied(false), 2500);
                    })
                    .catch((e) => setError(errorMessage(e)));
                }}
              >
                {copied ? <Check size={15} /> : <Copy size={15} />}{' '}
                {copied ? 'Lien copié' : 'Copier le lien de l’espace'}
              </button>
              {status.team.canManage && (
                <button
                  className="note-primary note-team-full"
                  onClick={() =>
                    void api()
                      .open(status.team.id, 'access')
                      .catch((e) => setError(errorMessage(e)))
                  }
                >
                  <Plus size={15} /> Inviter ou gérer les accès sur GitHub
                </button>
              )}
              <div className="note-members-list">
                {status.members.map((member) => {
                  const display = displayName(
                    status,
                    notes,
                    member.id,
                    member.name,
                  );
                  return (
                    <div className="note-member" key={member.id}>
                      <Avatar id={member.id} name={display} />
                      <span>
                        <strong>
                          {display}
                          {member.id === viewer.id ? ' · toi' : ''}
                        </strong>
                        <small>@{member.login}</small>
                      </span>
                      <span className="note-member-role">
                        {member.owner
                          ? 'Propriétaire'
                          : member.canEdit
                            ? 'Éditeur'
                            : 'Lecteur'}
                      </span>
                    </div>
                  );
                })}
              </div>
              {statusError && <p className="note-error">{statusError}</p>}
              <form
                className="note-pseudo-form"
                onSubmit={(event) => {
                  event.preventDefault();
                  const workspace = getWorkspace(
                    notes.filesRef.current.files,
                    status.team.id,
                  );
                  if (workspace && pseudo.trim()) {
                    notes.edit([
                      change({
                        ...workspace,
                        profiles: {
                          ...workspace.profiles,
                          [String(viewer.id)]: pseudo.trim(),
                        },
                      }),
                    ]);
                    void notes
                      .synchronize()
                      .catch((e) => setError(errorMessage(e)));
                  }
                }}
              >
                <label className="note-team-label">
                  Ton pseudo dans cet espace
                  <input
                    value={pseudo}
                    maxLength={40}
                    disabled={!notes.canEdit}
                    onChange={(e) => setPseudo(e.target.value)}
                  />
                </label>
                <button
                  className="note-secondary"
                  disabled={!notes.canEdit || !pseudo.trim()}
                >
                  <Check size={14} /> Enregistrer le pseudo
                </button>
                <p className="note-muted">
                  Ton identifiant GitHub reste visible pour reconnaître ton
                  compte.
                </p>
              </form>
              <p className="note-team-sync-info">
                Les modifications sont fusionnées automatiquement toutes les 15
                secondes. Tu peux aussi synchroniser avec Ctrl / ⌘ S.
              </p>
            </div>
          ) : tab === 'activity' ? (
            <div className="note-team-panel-body">
              <h3>Historique de cette page</h3>
              <p className="note-muted">
                Versions enregistrées dans GitHub. Les brouillons locaux
                apparaissent après synchronisation.
              </p>
              {historyLoading ? (
                <Loader2 className="animate-spin" />
              ) : revisions.length ? (
                revisions.map((revision) => (
                  <div className="note-revision" key={revision.id}>
                    <span className="note-activity-dot" />
                    <div>
                      <strong>{revision.author?.login || revision.name}</strong>
                      <time dateTime={revision.date}>
                        {new Date(revision.date).toLocaleString('fr')}
                      </time>
                      <p>{revision.message.split('\n')[0]}</p>
                      <button
                        className="note-secondary"
                        onClick={() =>
                          void api()
                            .open(status.team.id, `revision:${revision.id}`)
                            .catch((e) => setError(errorMessage(e)))
                        }
                      >
                        <ExternalLink size={13} /> Voir les modifications
                      </button>
                    </div>
                  </div>
                ))
              ) : (
                <div className="note-team-empty">
                  <History size={29} />
                  <p>Aucune version publiée pour le moment.</p>
                </div>
              )}
            </div>
          ) : (
            <>
              <div className="note-discussion-filters">
                {(['open', 'mentions', 'resolved'] as const).map((value) => (
                  <button
                    key={value}
                    aria-pressed={filter === value}
                    onClick={() => setFilter(value)}
                  >
                    {value === 'open'
                      ? 'En cours'
                      : value === 'mentions'
                        ? 'Pour moi'
                        : 'Résolues'}
                  </button>
                ))}
              </div>
              <div className="note-team-panel-body note-threads">
                {shown.length ? (
                  shown.map((thread) => (
                    <section
                      className="note-thread"
                      key={thread.id}
                      aria-label="Discussion"
                    >
                      {thread.quote && <blockquote>{thread.quote}</blockquote>}
                      {thread.comments.map((comment) => (
                        <div className="note-comment" key={comment.id}>
                          <Avatar
                            id={comment.author.id}
                            name={displayName(
                              status,
                              notes,
                              comment.author.id,
                              comment.author.name,
                            )}
                          />
                          <div>
                            <div className="note-comment-byline">
                              <strong>
                                {displayName(
                                  status,
                                  notes,
                                  comment.author.id,
                                  comment.author.name,
                                )}
                              </strong>
                              <small
                                title={`@${comment.author.login} · ${new Date(comment.createdAt).toLocaleString('fr')}`}
                              >
                                {new Date(comment.createdAt).toLocaleTimeString(
                                  'fr',
                                  { hour: '2-digit', minute: '2-digit' },
                                )}
                              </small>
                            </div>
                            {comment.deletedAt ? (
                              <p className="note-muted">Commentaire supprimé</p>
                            ) : (
                              <CommentBody body={comment.body} />
                            )}
                            <small className="note-comment-login">
                              @{comment.author.login}
                              {comment.editedAt ? ' · modifié' : ''}
                            </small>
                            {notes.canEdit &&
                              !comment.deletedAt &&
                              (comment.author.id === viewer.id ||
                                status.team.canManage) && (
                                <button
                                  className="note-comment-delete"
                                  aria-label="Supprimer le commentaire"
                                  onClick={() =>
                                    void publishThread({
                                      ...thread,
                                      comments: thread.comments.map((c) =>
                                        c.id === comment.id
                                          ? {
                                              ...c,
                                              body: '',
                                              deletedAt:
                                                new Date().toISOString(),
                                            }
                                          : c,
                                      ),
                                    }).catch((e) => setError(errorMessage(e)))
                                  }
                                >
                                  Supprimer
                                </button>
                              )}
                          </div>
                        </div>
                      ))}
                      <footer>
                        {notes.canEdit && (
                          <>
                            <button
                              onClick={() => {
                                setReplyTo(thread.id);
                                setBody('');
                              }}
                            >
                              <MessageCircle size={13} /> Répondre
                            </button>
                            <button
                              onClick={() =>
                                void publishThread({
                                  ...thread,
                                  resolved: !thread.resolved,
                                }).catch((e) => setError(errorMessage(e)))
                              }
                            >
                              <CheckCheck size={14} />{' '}
                              {thread.resolved ? 'Rouvrir' : 'Résoudre'}
                            </button>
                          </>
                        )}
                      </footer>
                    </section>
                  ))
                ) : (
                  <div className="note-team-empty">
                    <MessageCircle size={30} />
                    <h4>
                      {filter === 'resolved'
                        ? 'Tout reste à écrire'
                        : filter === 'mentions'
                          ? 'Tu es à jour'
                          : 'On en parle ?'}
                    </h4>
                    <p>
                      {filter === 'mentions'
                        ? 'Les discussions qui te mentionnent apparaîtront ici.'
                        : 'Pose une question ou partage une idée avec l’équipe.'}
                    </p>
                  </div>
                )}
              </div>
              {notes.canEdit && pageId && (
                <form
                  className="note-comment-compose"
                  onSubmit={(event) => {
                    event.preventDefault();
                    send();
                  }}
                >
                  {replyTo && (
                    <div className="note-compose-context">
                      Réponse à une discussion
                      <button
                        aria-label="Annuler la réponse"
                        type="button"
                        onClick={() => setReplyTo(null)}
                      >
                        <X size={13} />
                      </button>
                    </div>
                  )}
                  {quote && !replyTo && (
                    <blockquote>
                      {quote}
                      <button
                        type="button"
                        aria-label="Retirer la citation"
                        onClick={() => setQuote('')}
                      >
                        <X size={13} />
                      </button>
                    </blockquote>
                  )}
                  <div className="note-compose-author">
                    <Avatar name={name} id={viewer.id} />
                    <strong>{name}</strong>
                  </div>
                  <textarea
                    aria-label={
                      replyTo
                        ? 'Répondre à la discussion'
                        : 'Nouveau commentaire'
                    }
                    value={body}
                    maxLength={4000}
                    disabled={sending}
                    rows={3}
                    placeholder="Une idée, une question… @ pour mentionner"
                    onChange={(e) => setBody(e.target.value)}
                    onKeyDown={(e) => {
                      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
                        e.preventDefault();
                        send();
                      }
                    }}
                  />
                  {suggestions.length > 0 && (
                    <div
                      className="note-mention-list"
                      aria-label="Suggestions de mentions"
                    >
                      {suggestions.map((member) => (
                        <button
                          type="button"
                          key={member.id}
                          onClick={() =>
                            setBody(
                              body.replace(
                                /@([a-z\d-]*)$/i,
                                `@${member.login} `,
                              ),
                            )
                          }
                        >
                          <Avatar id={member.id} name={member.name} />
                          <span>
                            {displayName(status, notes, member.id, member.name)}
                            <small>@{member.login}</small>
                          </span>
                        </button>
                      ))}
                    </div>
                  )}
                  <div className="note-compose-bottom">
                    <small>⌘ / Ctrl ↵ pour envoyer</small>
                    <button
                      className="note-primary"
                      disabled={!body.trim() || sending}
                    >
                      <Send size={14} /> Envoyer
                    </button>
                  </div>
                </form>
              )}
              {!notes.canEdit && (
                <p className="note-muted note-team-readonly">
                  Lecture seule · demande un accès en modification au
                  propriétaire pour participer.
                </p>
              )}
            </>
          )}
        </aside>
      )}
    </>
  );
}
