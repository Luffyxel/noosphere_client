export const SOCIAL_REFRESH_RETRY_MS = 3_000;
export const SOCIAL_REFRESH_INTERVAL_MS = 30_000;
export const SOCIAL_REFRESH_MAX_RETRY_MS = 300_000;

export type SocialRefreshScheduler = {
  schedule: (callback: () => void, delay: number) => unknown;
  cancel: (handle: unknown) => void;
};

const defaultScheduler: SocialRefreshScheduler = {
  schedule: (callback, delay) => globalThis.setTimeout(callback, delay),
  cancel: (handle) =>
    globalThis.clearTimeout(handle as ReturnType<typeof globalThis.setTimeout>),
};

export function startSocialRefreshLoop(
  refresh: (announceNew: boolean) => Promise<boolean>,
  scheduler: SocialRefreshScheduler = defaultScheduler,
): () => void {
  let active = true;
  let synchronized = false;
  let retryDelay = SOCIAL_REFRESH_RETRY_MS;
  let handle: unknown;

  async function run() {
    let succeeded = false;
    try {
      succeeded = await refresh(synchronized);
    } catch {}
    if (!active) return;
    if (succeeded) {
      synchronized = true;
      retryDelay = SOCIAL_REFRESH_RETRY_MS;
    }
    handle = scheduler.schedule(
      () => void run(),
      succeeded ? SOCIAL_REFRESH_INTERVAL_MS : retryDelay,
    );
    if (!succeeded)
      retryDelay = Math.min(retryDelay * 2, SOCIAL_REFRESH_MAX_RETRY_MS);
  }

  void run();
  return () => {
    active = false;
    if (handle !== undefined) scheduler.cancel(handle);
  };
}
