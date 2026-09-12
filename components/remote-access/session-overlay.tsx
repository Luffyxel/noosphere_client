import { Monitor, X } from 'lucide-react';
import type { RemoteAccessModel } from '@/lib/use-remote-access';
import { samePrincipal } from '@/lib/remote-access';

export function RemoteSessionOverlay({
  remote,
}: {
  remote: RemoteAccessModel;
}) {
  const session = remote.status?.session;
  if (!session || session.state === 'idle') return null;
  const peer =
    session.state === 'streaming'
      ? remote.directory?.machines.find((machine) =>
          samePrincipal(machine.principal, session.peer),
        )
      : null;
  const title =
    session.state === 'streaming'
      ? peer?.name || 'Session active'
      : session.state === 'failed'
        ? 'Session interrompue'
        : session.state === 'waiting'
          ? 'Connexion entrante'
          : 'Connexion…';
  const detail =
    session.state === 'streaming'
      ? `${(session.rttUs / 1000).toFixed(1)} ms · ${session.frames} images`
      : session.state === 'failed'
        ? session.error
        : session.state === 'waiting'
          ? 'En attente de l’autre ordinateur'
          : 'Ouverture du canal sécurisé';
  return (
    <aside className="fixed bottom-5 right-5 z-[90] flex w-[310px] items-center gap-3 rounded-[16px] border border-white/[.1] bg-[#29292c]/95 p-3.5 shadow-2xl backdrop-blur-xl">
      <div className="grid size-10 shrink-0 place-items-center rounded-[11px] bg-[var(--noosphere-accent)]/[.1] text-[var(--noosphere-accent)]">
        <Monitor className="size-5" strokeWidth={1.5} />
      </div>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[13px] font-medium text-[#f5f5f7]">
          {title}
        </p>
        <p className="truncate text-[11px] text-[#8e8e93]">{detail}</p>
      </div>
      <button
        type="button"
        aria-label="Fermer la session distante"
        title="Fermer la session"
        className="grid size-8 shrink-0 place-items-center rounded-[9px] text-[#8e8e93] transition hover:bg-white/[.07] hover:text-white"
        onClick={() => void remote.stopSession()}
      >
        <X className="size-4" />
      </button>
    </aside>
  );
}
