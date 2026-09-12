import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  RemoteAccessApi,
  RemoteDirectory,
  RemoteStatus,
} from './remote-access';

export function useRemoteAccess(accountId: number, visible: boolean) {
  const [directory, setDirectory] = useState<RemoteDirectory | null>(null);
  const [error, setError] = useState('');
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [statusError, setStatusError] = useState('');
  const [busy, setBusy] = useState(false);
  const [time, setTime] = useState(Date.now);
  const working = useRef(false);
  const generation = useRef(0);

  const run = useCallback(
    async (action: (api: RemoteAccessApi) => Promise<RemoteDirectory>) => {
      const api = window.noosphereDesktop?.remoteAccess;
      if (!api || working.current) return false;
      const epoch = generation.current;
      working.current = true;
      setBusy(true);
      setError('');
      try {
        const next = await action(api);
        if (
          epoch !== generation.current ||
          next.owner.githubUserId !== accountId
        )
          return false;
        setDirectory(next);
        setTime(Date.now());
        setError(next.syncErrors.join('\n'));
        return true;
      } catch (failure) {
        if (epoch === generation.current) setError(String(failure));
        return false;
      } finally {
        working.current = false;
        if (epoch === generation.current) setBusy(false);
      }
    },
    [accountId],
  );

  const readStatus = useCallback(async () => {
    const api = window.noosphereDesktop?.remoteAccess;
    if (!api) return null;
    try {
      const next = await api.status();
      if (next.owner?.githubUserId !== accountId) return null;
      setStatus(next);
      setStatusError('');
      return next;
    } catch (failure) {
      setStatusError(String(failure));
      return null;
    }
  }, [accountId]);

  useEffect(() => {
    const epoch = ++generation.current;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      if (generation.current !== epoch) return;
      // Discovery also registers guest-only devices, without enabling hosting.
      await run((api) => api.syncDirectory());
      if (generation.current === epoch) {
        const delay = directory?.connecting ? 750 : visible ? 2_000 : 4_000;
        timer = setTimeout(() => void poll(), delay);
      }
    }
    timer = setTimeout(
      () => void run((api) => api.directory()).then(() => poll()),
      0,
    );
    return () => {
      generation.current = epoch + 1;
      clearTimeout(timer);
    };
  }, [directory?.connecting, run, visible]);

  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function pollStatus() {
      if (!active) return;
      let delay = 5_000;
      const next = await readStatus();
      if (next?.session.state !== 'idle') delay = 750;
      if (active) timer = setTimeout(() => void pollStatus(), delay);
    }
    void pollStatus();
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [readStatus]);

  const hasPending =
    directory?.incoming.some((r) => r.decision === 'pending') ?? false;
  useEffect(() => {
    if (!visible && !hasPending) return;
    const timer = setInterval(() => setTime(Date.now()), 5000);
    return () => clearInterval(timer);
  }, [visible, hasPending]);

  useEffect(() => {
    if (!visible) return;
    // Opening this section refreshes the global discovery cache.
    const timer = setTimeout(
      () => void run((api) => api.syncDirectory(true)),
      0,
    );
    return () => clearTimeout(timer);
  }, [visible, run]);

  return {
    directory: directory?.owner.githubUserId === accountId ? directory : null,
    status,
    statusError,
    time,
    busy,
    error,
    run,
    refresh: () => run((api) => api.syncDirectory(true)),
    refreshStatus: readStatus,
    stopSession: async () => {
      const api = window.noosphereDesktop?.remoteAccess;
      if (!api) return;
      await api.stopSession();
      await readStatus();
    },
  };
}

export type RemoteAccessModel = ReturnType<typeof useRemoteAccess>;
