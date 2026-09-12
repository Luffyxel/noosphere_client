'use client';

import { invoke } from '@tauri-apps/api/core';
import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { LoaderCircle, RefreshCw, X } from 'lucide-react';
import { useEffect, useState } from 'react';

import { updatePercentage } from '@/lib/app-update';

type Notice = {
  phase: 'downloading' | 'installing' | 'error';
  version: string;
  percentage: number | null;
};

function isTauri() {
  return '__TAURI_INTERNALS__' in window;
}

export function AppUpdater() {
  const [attempt, setAttempt] = useState(0);
  const [notice, setNotice] = useState<Notice | null>(null);

  useEffect(() => {
    if (!isTauri()) return;

    let active = true;
    let update: Update | null = null;
    let foundUpdate = false;
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const target = await invoke<string | null>('system_update_target');
          if (!active || !target) return;

          update = await check({ target, timeout: 15_000 });
          if (!active || !update) return;

          foundUpdate = true;
          const version = update.version;
          let downloaded = 0;
          let total: number | undefined;
          setNotice({ phase: 'downloading', version, percentage: null });
          await update.downloadAndInstall(
            (event) => {
              if (!active) return;
              if (event.event === 'Started') {
                total = event.data.contentLength;
                return;
              }
              if (event.event === 'Progress') {
                downloaded += event.data.chunkLength;
                setNotice({
                  phase: 'downloading',
                  version,
                  percentage: updatePercentage(downloaded, total),
                });
                return;
              }
              setNotice({ phase: 'installing', version, percentage: 100 });
            },
            { timeout: 5 * 60_000, restartAfterInstall: true },
          );
          if (active) {
            setNotice({ phase: 'installing', version, percentage: 100 });
            await relaunch();
          }
        } catch (error) {
          console.error('Noosphere update failed', error);
          if (active && foundUpdate) {
            setNotice((current) => ({
              phase: 'error',
              version: current?.version ?? '',
              percentage: null,
            }));
          }
          if (update) await update.close().catch(() => {});
        }
      })();
    }, 1_500);

    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [attempt]);

  if (!notice) return null;

  const busy = notice.phase !== 'error';
  return (
    <aside
      aria-live="polite"
      className="fixed right-5 top-5 z-[100] w-[280px] overflow-hidden rounded-[16px] border border-white/[.1] bg-[#29292c]/95 p-3.5 text-[#f5f5f7] shadow-2xl backdrop-blur-xl"
    >
      <div className="flex items-center gap-3">
        <div className="grid size-9 shrink-0 place-items-center rounded-[10px] bg-[var(--noosphere-accent)]/[.1] text-[var(--noosphere-accent)]">
          {busy ? (
            <LoaderCircle className="size-4 animate-spin" />
          ) : (
            <RefreshCw className="size-4" />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-medium">
            {notice.phase === 'error'
              ? 'Mise à jour interrompue'
              : `Mise à jour ${notice.version}`}
          </p>
          <p className="mt-0.5 text-[11px] text-[#a1a1a6]">
            {notice.phase === 'downloading'
              ? notice.percentage === null
                ? 'Téléchargement…'
                : `Téléchargement · ${notice.percentage}%`
              : notice.phase === 'installing'
                ? 'Installation…'
                : 'Réessayer au prochain lancement'}
          </p>
        </div>
        {notice.phase === 'error' && (
          <button
            type="button"
            aria-label="Réessayer"
            onClick={() => {
              setNotice(null);
              setAttempt((value) => value + 1);
            }}
            className="grid size-8 shrink-0 place-items-center rounded-[9px] text-[#a1a1a6] transition hover:bg-white/[.07] hover:text-white"
          >
            <RefreshCw className="size-3.5" />
          </button>
        )}
        {notice.phase === 'error' && (
          <button
            type="button"
            aria-label="Fermer"
            onClick={() => setNotice(null)}
            className="grid size-8 shrink-0 place-items-center rounded-[9px] text-[#a1a1a6] transition hover:bg-white/[.07] hover:text-white"
          >
            <X className="size-3.5" />
          </button>
        )}
      </div>
      {busy && (
        <div className="mt-3 h-1 overflow-hidden rounded-full bg-white/[.08]">
          <div
            className={
              notice.percentage === null
                ? 'h-full w-1/2 animate-pulse rounded-full bg-[var(--noosphere-accent)]'
                : 'h-full rounded-full bg-[var(--noosphere-accent)] transition-[width] duration-200'
            }
            style={
              notice.percentage === null
                ? undefined
                : { width: `${notice.percentage}%` }
            }
          />
        </div>
      )}
    </aside>
  );
}
