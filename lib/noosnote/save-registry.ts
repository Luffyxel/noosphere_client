import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { noteCloseHandler } from './window-close';

const savers = new Set<() => Promise<void>>();
let listening: Promise<void> | undefined;
export async function flushNotebooks() {
  await Promise.all([...savers].map((flush) => flush()));
}
export function registerNotebookSave(save: () => Promise<void>) {
  savers.add(save);
  if (isTauri() && !listening) {
    listening = getCurrentWindow()
      .onCloseRequested(
        noteCloseHandler(
          flushNotebooks,
          () => invoke('system_close_window'),
          (error) =>
            window.dispatchEvent(
              new CustomEvent('noosnote-close-error', {
                detail: String(error),
              }),
            ),
        ),
      )
      .then(() => {})
      .catch((error) => {
        listening = undefined;
        window.dispatchEvent(
          new CustomEvent('noosnote-close-error', { detail: String(error) }),
        );
      });
  }
  return () => {
    void save()
      .then(() => savers.delete(save))
      .catch((error) => {
        window.dispatchEvent(
          new CustomEvent('noosnote-close-error', { detail: String(error) }),
        );
      });
  };
}
