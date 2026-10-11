import { useCallback, useEffect, useRef, useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { NoteChange, NoteDocument, NoteSnapshot } from './types';
import { applyFiles, getPage, pagePath } from './model';

const EMPTY: NoteSnapshot = {
  files: {},
  deletedPaths: [],
  incremental: false,
  repository: null,
  dirtyCount: 0,
  dirtyPaths: [],
  conflicts: [],
  lastSyncedAt: null,
  setup: 'repository',
};

function message(error: unknown) {
  return typeof error === 'string'
    ? error
    : error instanceof Error
      ? error.message
      : 'La synchronisation des notes a échoué.';
}

export function useNoosnote(ownerId: number, visible: boolean) {
  const [snapshot, setSnapshot] = useState<NoteSnapshot>(EMPTY);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [error, setError] = useState('');
  const current = useRef<NoteSnapshot>(EMPTY);
  const pending = useRef(
    new Map<string, { value: NoteDocument | null; revision: number }>(),
  );
  const revision = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const maximum = useRef<ReturnType<typeof setTimeout> | null>(null);
  const savingOperation = useRef<Promise<void> | null>(null);
  const syncingOperation = useRef<Promise<void> | null>(null);
  const alive = useRef(true);
  const scope = useRef(0);
  const lastPublish = useRef(0);
  const recentPages = useRef<string[]>([]);

  const accept = useCallback((incoming: NoteSnapshot) => {
    let files = incoming.incremental
      ? applyFiles(current.current.files, [
          ...Object.entries(incoming.files).map(([path, value]) => ({
            path,
            value,
          })),
          ...incoming.deletedPaths.map((path) => ({ path, value: null })),
        ])
      : incoming.files;
    files = applyFiles(
      files,
      [...pending.current].map(([path, entry]) => ({
        path,
        value: entry.value,
      })),
    );
    for (const [path, file] of Object.entries(files))
      if (
        file.kind === 'page' &&
        !recentPages.current.includes(path) &&
        !pending.current.has(path) &&
        !incoming.dirtyPaths.includes(path)
      )
        delete files[path];
    const next = { ...incoming, files, incremental: false, deletedPaths: [] };
    current.current = next;
    if (alive.current) setSnapshot(next);
  }, []);

  const loadPage = useCallback(
    async (workspaceId: string, pageId: string) => {
      const path = pagePath(workspaceId, pageId);
      recentPages.current = recentPages.current.filter(
        (value) => value !== path,
      );
      recentPages.current.push(path);
      if (recentPages.current.length > 24) recentPages.current.shift();
      if (!current.current.files[path]) {
        const response = await window.noosphereDesktop!.noosnote.page(
          workspaceId,
          pageId,
        );
        accept(response);
        return getPage(response.files, workspaceId, pageId);
      }
      return getPage(current.current.files, workspaceId, pageId);
    },
    [accept],
  );

  const flush = useCallback(async () => {
    while (savingOperation.current) await savingOperation.current;
    if (!pending.current.size) return;
    const api = window.noosphereDesktop?.noosnote;
    if (!api) throw new Error('Le stockage des notes est indisponible.');
    if (timer.current) clearTimeout(timer.current);
    if (maximum.current) clearTimeout(maximum.current);
    timer.current = null;
    maximum.current = null;
    const ownScope = scope.current;
    const operation = (async () => {
      if (alive.current) setSaving(true);
      while (pending.current.size && ownScope === scope.current) {
        const batch = [...pending.current].slice(0, 256);
        const response = await api.save(
          batch.map(([path, entry]) => ({ path, value: entry.value })),
        );
        if (ownScope !== scope.current) return;
        for (const [path, entry] of batch)
          if (pending.current.get(path)?.revision === entry.revision)
            pending.current.delete(path);
        accept(response);
      }
    })();
    savingOperation.current = operation;
    try {
      await operation;
      if (alive.current) setError('');
    } catch (error) {
      if (alive.current)
        setError(`Brouillon non enregistré : ${message(error)}`);
      throw error;
    } finally {
      if (savingOperation.current === operation) savingOperation.current = null;
      if (alive.current) setSaving(pending.current.size > 0);
    }
  }, [accept]);

  const edit = useCallback(
    (changes: NoteChange[]) => {
      if (!changes.length) return;
      revision.current += 1;
      for (const change of changes)
        pending.current.set(change.path, {
          value: change.value,
          revision: revision.current,
        });
      current.current = {
        ...current.current,
        files: applyFiles(current.current.files, changes),
      };
      setSnapshot(current.current);
      setSaving(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => void flush().catch(() => {}), 400);
      maximum.current ??= setTimeout(() => void flush().catch(() => {}), 1800);
    },
    [flush],
  );

  const synchronize = useCallback(async () => {
    if (syncingOperation.current) return syncingOperation.current;
    const ownScope = scope.current;
    const operation = (async () => {
      await flush();
      const api = window.noosphereDesktop?.noosnote;
      if (!api) return;
      if (alive.current) setSyncing(true);
      const response = await api.sync();
      if (ownScope !== scope.current) return;
      accept(response);
      lastPublish.current = Date.now();
      if (alive.current) setError('');
    })();
    syncingOperation.current = operation;
    try {
      await operation;
    } catch (error) {
      if (alive.current) setError(message(error));
      throw error;
    } finally {
      if (syncingOperation.current === operation)
        syncingOperation.current = null;
      if (alive.current) setSyncing(false);
    }
  }, [accept, flush]);

  const setup = useCallback(async () => {
    await flush();
    setSyncing(true);
    try {
      const response = await window.noosphereDesktop!.noosnote.setup();
      accept(response);
      if (response.setup === 'ready') await synchronize();
      return response.setup;
    } catch (error) {
      setError(message(error));
      throw error;
    } finally {
      setSyncing(false);
    }
  }, [accept, flush, synchronize]);

  const resolve = useCallback(
    async (id: string, choice: 'local' | 'remote') => {
      await flush();
      try {
        accept(await window.noosphereDesktop!.noosnote.resolve(id, choice));
        setError('');
      } catch (error) {
        setError(message(error));
        throw error;
      }
    },
    [accept, flush],
  );

  useEffect(() => {
    alive.current = true;
    scope.current += 1;
    const ownScope = scope.current;
    current.current = EMPTY;
    pending.current.clear();
    recentPages.current = [];
    queueMicrotask(() => {
      if (ownScope === scope.current) {
        setSnapshot(EMPTY);
        setLoading(true);
        setError('');
      }
    });
    void window.noosphereDesktop?.noosnote
      .cached()
      .then(async (response) => {
        if (ownScope !== scope.current) return;
        accept(response);
        if (!response.repository) {
          const discovered = await window.noosphereDesktop!.noosnote.discover();
          if (ownScope !== scope.current) return;
          accept(discovered);
          if (discovered.repository && discovered.setup === 'ready')
            await synchronize();
        }
        if (ownScope === scope.current) setLoading(false);
      })
      .catch((error) => {
        if (ownScope === scope.current) {
          setError(message(error));
          setLoading(false);
        }
      });
    return () => {
      alive.current = false;
      void flush().catch(() => {});
      if (timer.current) clearTimeout(timer.current);
      if (maximum.current) clearTimeout(maximum.current);
    };
  }, [ownerId, accept, flush, synchronize]);

  useEffect(() => {
    let active = true;
    let next: ReturnType<typeof setTimeout>;
    let failures = 0;
    async function poll() {
      if (
        current.current.repository &&
        (visible || current.current.dirtyCount > 0)
      ) {
        const wait =
          current.current.dirtyCount > 0
            ? Math.max(0, 60000 - (Date.now() - lastPublish.current))
            : 0;
        if (wait === 0)
          try {
            await synchronize();
            failures = 0;
          } catch {
            failures += 1;
          }
      }
      if (active)
        next = setTimeout(
          () => void poll(),
          failures
            ? Math.min(300000, 15000 * 2 ** failures)
            : current.current.dirtyCount > 0
              ? 15000
              : visible
                ? 30000
                : 300000,
        );
    }
    next = setTimeout(() => void poll(), visible ? 1500 : 30000);
    return () => {
      active = false;
      clearTimeout(next);
    };
  }, [visible, synchronize]);

  useEffect(() => {
    const save = (event: KeyboardEvent) => {
      if (
        visible &&
        (event.ctrlKey || event.metaKey) &&
        event.key.toLowerCase() === 's'
      ) {
        event.preventDefault();
        void synchronize().catch(() => {});
      }
    };
    const online = () => {
      if (current.current.repository) void synchronize().catch(() => {});
    };
    document.addEventListener('keydown', save);
    window.addEventListener('online', online);
    return () => {
      document.removeEventListener('keydown', save);
      window.removeEventListener('online', online);
    };
  }, [visible, synchronize]);

  useEffect(() => {
    if (!isTauri()) return;
    let dispose: (() => void) | undefined;
    let active = true;
    void getCurrentWindow()
      .onCloseRequested(async (event) => {
        if (!pending.current.size) return;
        event.preventDefault();
        try {
          await flush();
          await getCurrentWindow().close();
        } catch {}
      })
      .then((remove) => {
        if (active) dispose = remove;
        else remove();
      });
    return () => {
      active = false;
      dispose?.();
    };
  }, [flush]);

  return {
    ...snapshot,
    setupStage: snapshot.setup,
    loading,
    saving,
    syncing,
    error,
    edit,
    flush,
    synchronize,
    setup,
    resolve,
    loadPage,
    filesRef: current,
  };
}

export type NoosnoteController = ReturnType<typeof useNoosnote>;
