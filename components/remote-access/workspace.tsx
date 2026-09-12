import { useState } from 'react';
import {
  ArrowUpRight,
  Check,
  ChevronRight,
  Clock3,
  Keyboard,
  Laptop,
  Monitor,
  Mouse,
  RefreshCw,
  ScrollText,
  Search,
  Settings2,
  ShieldCheck,
  Users,
} from 'lucide-react';
import type { Conversation, GitHubViewer } from '@/lib/desktop-bridge';
import { Button } from '@/components/ui/button';
import {
  DESKTOP_PERMISSIONS,
  groupMachines,
  machineOnline,
  principalId,
  remoteUsers,
  samePrincipal,
  type RemoteMachine,
  type RemoteUser,
} from '@/lib/remote-access';
import type { RemoteAccessModel } from '@/lib/use-remote-access';
import { cn } from '@/lib/utils';
import { accentButton } from './controls';
import { RemoteAccessSettings } from './settings';
import { UserGrantEditor } from './permissions';
import { RemoteLogsDialog } from './logs';

type Filter = 'all' | 'own' | 'friends' | number;
export function RemoteWorkspace({
  viewer,
  friends,
  remote,
  settingsOpen,
  onSettingsChange,
}: {
  viewer: GitHubViewer;
  friends: Conversation[];
  remote: RemoteAccessModel;
  settingsOpen: boolean;
  onSettingsChange: (value: boolean) => void;
}) {
  const [filter, setFilter] = useState<Filter>('all');
  const [search, setSearch] = useState('');
  const [editing, setEditing] = useState<RemoteUser | null>(null);
  const [logsOpen, setLogsOpen] = useState(false);
  const data = remote.directory;
  const groups = data
    ? groupMachines(data.machines, data.owner)
    : { own: [], friends: [] };
  const users = remoteUsers(viewer, friends, [
    ...groups.own,
    ...groups.friends,
  ]);
  const friendMachines = users
    .filter((user) => !user.own)
    .flatMap((user) => user.machines);
  const matches = (m: RemoteMachine) =>
    `${m.name} ${m.login}`
      .toLocaleLowerCase('fr')
      .includes(search.toLocaleLowerCase('fr'));
  const own = groups.own.filter(matches);
  const selectedUser =
    typeof filter === 'number'
      ? users.find((user) => user.id === filter)
      : undefined;
  const shared = (
    typeof filter === 'number' ? (selectedUser?.machines ?? []) : friendMachines
  ).filter(
    (m) =>
      matches(m) &&
      (typeof filter !== 'number' || m.principal.githubUserId === filter),
  );
  function navigate(next: Filter) {
    setFilter(next);
    onSettingsChange(false);
  }
  function manageUser(githubUserId: number) {
    const user = users.find((candidate) => candidate.id === githubUserId);
    if (user) setEditing(user);
  }
  const navClass = (active: boolean) =>
    cn(
      'flex w-full items-center gap-2.5 rounded-[10px] px-3 py-2.5 text-left text-[13px] transition',
      active
        ? 'bg-white/[.08] text-[#f5f5f7]'
        : 'text-[#a1a1a6] hover:bg-white/[.04]',
    );

  return (
    <>
      <aside className="hidden w-[250px] shrink-0 flex-col border-r border-white/[.07] bg-[#202022] md:flex">
        <header className="flex h-[58px] items-center gap-2.5 border-b border-white/[.07] px-4">
          <Monitor className="size-4 text-[var(--noosphere-accent)]" />
          <span className="text-[14px] font-semibold tracking-tight">
            Bureau à distance
          </span>
        </header>
        <nav
          aria-label="Machines à distance"
          className="min-h-0 flex-1 overflow-y-auto p-2.5"
        >
          <div className="space-y-1">
            {(
              [
                {
                  id: 'all',
                  title: 'Tous les ordinateurs',
                  icon: Monitor,
                  count: groups.own.length + friendMachines.length + 1,
                },
                {
                  id: 'own',
                  title: 'Mes machines',
                  icon: Laptop,
                  count: groups.own.length + 1,
                },
                {
                  id: 'friends',
                  title: 'Machines des amis',
                  icon: Users,
                  count: friendMachines.length,
                },
              ] as const
            ).map(({ id, title, icon: Icon, count }) => (
              <button
                key={id}
                type="button"
                aria-current={
                  !settingsOpen && filter === id ? 'page' : undefined
                }
                className={navClass(!settingsOpen && filter === id)}
                onClick={() => navigate(id)}
              >
                <Icon className="size-[17px] text-[#8e8e93]" />
                <span className="flex-1">{title}</span>
                <span className="text-[11px] text-[#737378]">{count}</span>
              </button>
            ))}
          </div>
          <div className="mb-2 mt-8 px-3 text-[10px] font-semibold uppercase tracking-[.11em] text-[#737378]">
            Utilisateurs
          </div>
          {users.length ? (
            users.map((user) => (
              <button
                key={user.id}
                type="button"
                className={navClass(!settingsOpen && filter === user.id)}
                onClick={() => navigate(user.id)}
              >
                <div className="relative">
                  <img
                    src={user.avatarUrl}
                    alt=""
                    className="size-7 rounded-full bg-[#303033]"
                  />
                  <span
                    className={cn(
                      'absolute -bottom-0.5 -right-0.5 size-2 rounded-full border-2 border-[#202022]',
                      user.machines.some((m) => machineOnline(m, remote.time))
                        ? 'bg-[var(--noosphere-accent)]'
                        : 'bg-[#636366]',
                    )}
                  />
                </div>
                <span className="truncate">
                  {user.login}
                  {user.own ? ' · Moi' : ''}
                </span>
              </button>
            ))
          ) : (
            <p className="px-3 text-[12px] leading-5 text-[#737378]">
              Aucun utilisateur
            </p>
          )}
        </nav>
        <button
          type="button"
          className={cn('m-2.5', navClass(settingsOpen))}
          onClick={() => onSettingsChange(true)}
        >
          <Settings2 className="size-4" />
          Réglages de cet ordinateur
          <ChevronRight className="ml-auto size-3.5" />
        </button>
        <div className="flex h-[66px] items-center gap-2.5 border-t border-white/[.07] bg-[#19191b] px-4">
          <img src={viewer.avatarUrl} alt="" className="size-8 rounded-full" />
          <div className="min-w-0">
            <p className="truncate text-[12px] font-medium">
              {viewer.name || viewer.login}
            </p>
          </div>
        </div>
      </aside>
      <section className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-[58px] shrink-0 items-center justify-between gap-4 border-b border-white/[.07] px-6">
          <p className="text-[13px] font-medium text-[#d1d1d6]">
            {settingsOpen
              ? 'Réglages'
              : filter === 'own'
                ? 'Mes machines'
                : filter === 'friends'
                  ? 'Machines des amis'
                  : typeof filter === 'number'
                    ? `@${users.find((u) => u.id === filter)?.login ?? ''}`
                    : 'Ordinateurs'}
          </p>
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              className="h-8 rounded-[9px] px-2.5 text-[11px] text-[#8e8e93]"
              onClick={() => setLogsOpen(true)}
            >
              <ScrollText className="size-3.5" />
              Logs
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="size-8 rounded-[9px] text-[#8e8e93]"
              title="Actualiser les machines"
              aria-label="Actualiser les machines"
              onClick={() => void remote.refresh()}
            >
              <RefreshCw className="size-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="size-8 rounded-[9px] text-[#8e8e93]"
              title={
                settingsOpen
                  ? 'Voir les ordinateurs'
                  : 'Réglages du bureau à distance'
              }
              aria-label={
                settingsOpen
                  ? 'Voir les ordinateurs'
                  : 'Réglages du bureau à distance'
              }
              onClick={() => onSettingsChange(!settingsOpen)}
            >
              {settingsOpen ? <Monitor /> : <Settings2 />}
            </Button>
          </div>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {remote.error && (
            <div
              role="alert"
              className="mx-6 mt-4 rounded-[12px] border border-[#ff9f0a]/15 bg-[#ff9f0a]/[.04] px-4 py-3 text-[12px] leading-5 text-[#d1b48a]"
            >
              Synchronisation incomplète.
              <details className="mt-1">
                <summary className="cursor-pointer text-[#a1a1a6]">
                  Détails
                </summary>
                <p className="mt-2 whitespace-pre-line">{remote.error}</p>
              </details>
            </div>
          )}
          {settingsOpen ? (
            <RemoteAccessSettings
              remote={remote}
              friends={friends}
              viewer={viewer}
            />
          ) : (
            <div className="mx-auto max-w-[1100px] px-6 py-8 lg:px-10">
              <div className="mb-7 flex flex-wrap items-end justify-between gap-5">
                <div>
                  <h1 className="text-[28px] font-semibold tracking-[-.045em]">
                    Ordinateurs
                  </h1>
                </div>
                <label className="flex h-9 w-[210px] items-center gap-2 rounded-[10px] border border-white/[.07] bg-[#242426] px-3">
                  <Search className="size-3.5 text-[#636366]" />
                  <input
                    aria-label="Rechercher une machine"
                    placeholder="Rechercher une machine"
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                    className="min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-[#737378]"
                  />
                </label>
              </div>
              {!data && (
                <output className="block py-12 text-center text-[13px] text-[#8e8e93]">
                  Chargement…
                </output>
              )}
              {(filter === 'all' || filter === 'own') && (
                <section className="mb-9">
                  <h2 className="mb-4 text-[13px] font-medium text-[#c7c7cc]">
                    Mes machines{' '}
                    <span className="ml-2 text-[11px] text-[#636366]">
                      {groups.own.length + 1}
                    </span>
                  </h2>
                  <div className="grid gap-4 xl:grid-cols-2">
                    <div className="rounded-[18px] border border-white/[.07] bg-[#232325] p-5">
                      <div className="mb-6 flex items-start justify-between">
                        <div className="grid size-12 place-items-center rounded-[13px] bg-[#303633] text-[var(--noosphere-accent)]">
                          <Monitor className="size-6" strokeWidth={1.4} />
                        </div>
                      </div>
                      <h3 className="text-[15px] font-medium">
                        Cet ordinateur
                      </h3>
                      <div className="mt-5 flex items-center justify-between border-t border-white/[.06] pt-4">
                        <span className="flex items-center gap-1.5 text-[11px] text-[#8e8e93]">
                          <span
                            className={cn(
                              'size-1.5 rounded-full',
                              data?.hostEnabled
                                ? 'bg-[var(--noosphere-accent)]'
                                : 'bg-[#636366]',
                            )}
                          />
                          {data?.hostEnabled
                            ? 'Demandes autorisées'
                            : 'Accès désactivé'}
                        </span>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-8 text-[12px] text-[var(--noosphere-accent)]"
                          onClick={() => onSettingsChange(true)}
                        >
                          Configurer
                          <ChevronRight className="size-3.5" />
                        </Button>
                      </div>
                    </div>
                    {own.map((machine) => (
                      <MachineCard
                        key={principalId(machine.principal)}
                        machine={machine}
                        remote={remote}
                        onManage={() =>
                          manageUser(machine.principal.githubUserId)
                        }
                      />
                    ))}
                  </div>
                </section>
              )}
              {filter !== 'own' && (
                <section>
                  <h2 className="mb-4 text-[13px] font-medium text-[#c7c7cc]">
                    {typeof filter === 'number'
                      ? selectedUser?.own
                        ? 'Mes ordinateurs'
                        : 'Ordinateurs'
                      : 'Machines des amis'}{' '}
                    <span className="ml-2 text-[11px] text-[#636366]">
                      {shared.length}
                    </span>
                  </h2>
                  {shared.length ? (
                    <div className="grid gap-4 xl:grid-cols-2">
                      {shared.map((machine) => (
                        <MachineCard
                          key={principalId(machine.principal)}
                          machine={machine}
                          remote={remote}
                          onManage={() =>
                            manageUser(machine.principal.githubUserId)
                          }
                        />
                      ))}
                    </div>
                  ) : (
                    <div className="rounded-[18px] border border-dashed border-white/[.09] px-6 py-12 text-center">
                      <div className="mx-auto mb-4 grid size-14 place-items-center rounded-[16px] bg-white/[.035]">
                        <Users
                          className="size-6 text-[#737378]"
                          strokeWidth={1.3}
                        />
                      </div>
                      <h3 className="text-[15px] font-medium">
                        {search
                          ? 'Aucune machine correspondante'
                          : 'Aucune machine détectée'}
                      </h3>
                      {typeof filter === 'number' && !search && (
                        <div className="mt-3 flex justify-center gap-2">
                          {selectedUser && (
                            <Button
                              variant="ghost"
                              className="text-[var(--noosphere-accent)]"
                              disabled={remote.busy}
                              onClick={() => setEditing(selectedUser)}
                            >
                              Gérer l’accès
                            </Button>
                          )}
                          <Button
                            variant="ghost"
                            className="text-[var(--noosphere-accent)]"
                            title="Cet ami doit ouvrir une version compatible de Noosphere."
                            onClick={() => void remote.refresh()}
                          >
                            Actualiser
                          </Button>
                        </div>
                      )}
                    </div>
                  )}
                </section>
              )}
            </div>
          )}
        </div>
      </section>
      {editing && (
        <UserGrantEditor
          key={editing.id}
          user={editing}
          grants={data?.grants ?? []}
          userPolicies={data?.userGrants ?? []}
          remote={remote}
          onClose={() => setEditing(null)}
        />
      )}
      <RemoteLogsDialog
        open={logsOpen}
        onOpenChange={setLogsOpen}
        remote={remote}
      />
    </>
  );
}

function MachineCard({
  machine,
  remote,
  onManage,
}: {
  machine: RemoteMachine;
  remote: RemoteAccessModel;
  onManage: () => void;
}) {
  const online = machineOnline(machine, remote.time);
  const request = remote.directory?.outgoing.find(
    (r) =>
      samePrincipal(r.host, machine.principal) &&
      r.expiresAt * 1000 > remote.time,
  );
  const answer =
    request &&
    remote.directory?.answers.find(
      (a) => a.request.id.join('.') === request.id.join('.'),
    );
  const pending = request && (!answer || answer.decision === 'pending');
  const automatic = machine.unattended && machine.permissions.screen;
  return (
    <article className="rounded-[18px] border border-white/[.07] bg-[#232325] p-5 transition-colors hover:border-white/[.13]">
      <div className="mb-6 flex items-start justify-between">
        <div className="grid size-12 place-items-center rounded-[13px] bg-[#2e3030] text-[#b3c6bc]">
          <Laptop className="size-6" strokeWidth={1.4} />
        </div>
        <span
          className={cn(
            'flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[10px]',
            online
              ? 'bg-[var(--noosphere-accent)]/[.08] text-[var(--noosphere-accent)]'
              : 'bg-white/[.04] text-[#737378]',
          )}
        >
          <span
            className={cn(
              'size-1 rounded-full',
              online ? 'bg-[var(--noosphere-accent)]' : 'bg-[#636366]',
            )}
          />
          {online ? 'En ligne' : 'Hors ligne'}
        </span>
      </div>
      <h3 className="truncate text-[15px] font-medium">{machine.name}</h3>
      <p className="mt-1 text-[11px] text-[#737378]">
        @{machine.login} ·{' '}
        {machine.platform === 'unknown'
          ? 'Ordinateur'
          : machine.platform === 'windows'
            ? 'Windows'
            : machine.platform === 'linux'
              ? 'Linux'
              : 'macOS'}
      </p>
      <div className="mt-4 flex items-center gap-2 text-[11px] text-[#8e8e93]">
        {automatic ? (
          <ShieldCheck className="size-3.5 text-[var(--noosphere-accent)]" />
        ) : (
          <Clock3 className="size-3.5" />
        )}
        {!machine.hostEnabled
          ? 'Hébergement désactivé'
          : automatic
            ? 'Sans confirmation'
            : 'Sur demande'}
        <div className="ml-auto flex gap-2">
          {machine.permissions.keyboard && (
            <Keyboard aria-label="Clavier autorisé" className="size-3.5" />
          )}
          {machine.permissions.mouse && (
            <Mouse aria-label="Souris autorisée" className="size-3.5" />
          )}
        </div>
      </div>
      <div className="mt-4 border-t border-white/[.06] pt-4">
        {answer && answer.decision !== 'pending' ? (
          <output className="block text-[12px] leading-5">
            <p
              className={
                answer.decision === 'approved'
                  ? 'flex items-center gap-1.5 text-[var(--noosphere-accent)]'
                  : 'text-[#a1a1a6]'
              }
            >
              {answer.decision === 'approved' && <Check className="size-3.5" />}
              {answer.decision === 'approved'
                ? 'Accès autorisé'
                : answer.decision === 'declined'
                  ? 'Demande refusée'
                  : answer.decision === 'revoked'
                    ? 'Accès révoqué'
                    : 'En attente de confirmation'}
            </p>
            {answer.decision === 'approved' && (
              <Button
                className={cn('mt-3 h-9 w-full text-[12px]', accentButton)}
                disabled={remote.busy || remote.directory?.connecting}
                onClick={() =>
                  void remote.run((api) =>
                    api.connectMachine(machine.principal),
                  )
                }
              >
                {remote.directory?.connecting ? 'Connexion…' : 'Se connecter'}
                <ArrowUpRight className="size-3.5" />
              </Button>
            )}
          </output>
        ) : (
          <Button
            className={cn(
              'h-9 w-full text-[12px]',
              automatic
                ? accentButton
                : 'rounded-[10px] border border-white/[.09] bg-white/[.04] text-[#d1d1d6] hover:bg-white/[.08]',
            )}
            disabled={
              remote.busy ||
              remote.directory?.connecting ||
              !online ||
              !machine.hostEnabled ||
              Boolean(pending)
            }
            onClick={() =>
              void remote.run((api) =>
                automatic
                  ? api.connectMachine(machine.principal)
                  : api.requestAccess(machine.principal, DESKTOP_PERMISSIONS),
              )
            }
          >
            {remote.directory?.connecting
              ? 'Connexion…'
              : pending
                ? remote.error
                  ? 'Demande à synchroniser…'
                  : 'Demande envoyée…'
                : automatic
                  ? 'Se connecter'
                  : 'Demander l’accès'}
            {!pending && !remote.directory?.connecting && (
              <ArrowUpRight className="size-3.5" />
            )}
          </Button>
        )}
        {request && (
          <Button
            variant="ghost"
            size="sm"
            className="mt-2 h-7 w-full text-[11px] text-[#8e8e93]"
            disabled={remote.busy}
            onClick={() =>
              void remote.run((api) => api.cancelAccess(machine.principal))
            }
          >
            {answer && answer.decision !== 'pending'
              ? 'Fermer'
              : 'Annuler la demande'}
          </Button>
        )}
        <Button
          variant="ghost"
          className="mt-2 h-8 w-full text-[12px] text-[var(--noosphere-accent)]"
          onClick={onManage}
        >
          <ShieldCheck className="size-3.5" />
          Gérer son accès
        </Button>
      </div>
    </article>
  );
}
