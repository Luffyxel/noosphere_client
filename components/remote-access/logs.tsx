import { useState } from 'react';
import { Check, Copy, RefreshCw, ScrollText } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { APP_VERSION } from '@/lib/app-version';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog';
import type { RemoteLog } from '@/lib/remote-access';
import type { RemoteAccessModel } from '@/lib/use-remote-access';
import { cn } from '@/lib/utils';

function formatTime(value: number) {
  return new Intl.DateTimeFormat('fr-FR', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(new Date(value));
}

function logText(log: RemoteLog) {
  return `${formatTime(log.at)} [${log.scope}] ${log.message}`;
}

export function RemoteLogsDialog({
  open,
  onOpenChange,
  remote,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  remote: RemoteAccessModel;
}) {
  const [copied, setCopied] = useState(false);
  const logs = [
    ...(remote.directory?.logs ?? []),
    ...(remote.status?.logs ?? []),
  ].sort((left, right) => left.at - right.at);
  const unique = logs.filter(
    (log, index) =>
      index === 0 ||
      log.at !== logs[index - 1].at ||
      log.scope !== logs[index - 1].scope ||
      log.message !== logs[index - 1].message,
  );
  const session = remote.status?.session.state ?? 'idle';

  async function copy() {
    const header = [
      `Noosphere Remote ${APP_VERSION}`,
      `Session: ${session}`,
      `Streaming: ${remote.status?.streamingReady ? 'ready' : 'unavailable'}`,
      ...(remote.error ? [`Error: ${remote.error}`] : []),
      ...(remote.statusError ? [`Daemon error: ${remote.statusError}`] : []),
      ...(remote.status?.unavailable.map(
        (message) => `Unavailable: ${message}`,
      ) ?? []),
    ];
    await navigator.clipboard.writeText(
      [...header, ...unique.map(logText)].join('\n'),
    );
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[82vh] flex-col rounded-[22px] border-white/10 bg-[#202022] p-0 text-[#f5f5f7] sm:max-w-[720px]">
        <header className="flex items-start gap-3 border-b border-white/[.07] px-6 py-5 pr-14">
          <div className="grid size-10 shrink-0 place-items-center rounded-[11px] bg-white/[.05] text-[var(--noosphere-accent)]">
            <ScrollText className="size-5" />
          </div>
          <div className="min-w-0 flex-1">
            <DialogTitle className="text-[17px]">
              Journal de connexion
            </DialogTitle>
            <DialogDescription className="mt-1 text-[12px] text-[#8e8e93]">
              État : {session} · PID {remote.status?.pid ?? '—'}
            </DialogDescription>
          </div>
          <div className="flex gap-2">
            <Button
              variant="ghost"
              size="sm"
              className="h-8 rounded-[9px] text-[11px] text-[#a1a1a6]"
              onClick={() =>
                void remote.refresh().then(() => remote.refreshStatus())
              }
            >
              <RefreshCw className="size-3.5" />
              Actualiser
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-8 rounded-[9px] text-[11px] text-[#a1a1a6]"
              disabled={!unique.length}
              onClick={() => void copy()}
            >
              {copied ? (
                <Check className="size-3.5" />
              ) : (
                <Copy className="size-3.5" />
              )}
              {copied ? 'Copié' : 'Copier'}
            </Button>
          </div>
        </header>
        {(remote.error ||
          remote.statusError ||
          Boolean(remote.status?.unavailable.length)) && (
          <div className="whitespace-pre-line border-b border-white/[.07] bg-[#ff9f0a]/[.04] px-6 py-3 text-[11px] leading-5 text-[#d1b48a]">
            {[
              remote.error,
              remote.statusError,
              ...(remote.status?.unavailable ?? []),
            ]
              .filter(Boolean)
              .join('\n')}
          </div>
        )}
        <div className="min-h-[280px] flex-1 overflow-y-auto p-3">
          {unique.length ? (
            <div className="space-y-0.5 font-mono text-[11px] leading-5">
              {unique.map((log, index) => (
                <div
                  key={`${log.at}-${log.scope}-${index}`}
                  className="grid grid-cols-[70px_82px_1fr] gap-2 rounded-[8px] px-3 py-1.5 hover:bg-white/[.035]"
                >
                  <time className="text-[#636366]">{formatTime(log.at)}</time>
                  <span className="truncate text-[#8e8e93]">{log.scope}</span>
                  <span
                    className={cn(
                      'break-words',
                      log.level === 'error'
                        ? 'text-[#ff9f9a]'
                        : log.level === 'success'
                          ? 'text-[var(--noosphere-accent)]'
                          : 'text-[#d1d1d6]',
                    )}
                  >
                    {log.message}
                  </span>
                </div>
              ))}
            </div>
          ) : (
            <div className="grid min-h-[280px] place-items-center text-[12px] text-[#737378]">
              Aucun événement pour le moment.
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
