import { createContext, useContext } from 'react';
import type { NoosnoteController } from '@/lib/noosnote/use-noosnote';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';

export type NoteContextValue = {
  notes: NoosnoteController;
  workspaceId: string;
  openPage: (pageId: string) => void;
  report: (error: unknown) => void;
  pageAction: (action: PageAction, pageId: string) => void;
};
export type PageAction =
  | 'open'
  | 'rename'
  | 'icon'
  | 'child'
  | 'duplicate'
  | 'move'
  | 'export'
  | 'trash';
export const NoteContext = createContext<NoteContextValue | null>(null);
export function useNoteContext() {
  const context = useContext(NoteContext);
  if (!context) throw new Error('Noosnote indisponible.');
  return context;
}
export function NoteDialog({
  title,
  open,
  onClose,
  children,
}: {
  title: string;
  open: boolean;
  onClose: () => void;
  children: React.ReactNode;
}) {
  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (!value) onClose();
      }}
    >
      <DialogContent className="note-dialog sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>
        {children}
      </DialogContent>
    </Dialog>
  );
}
export function downloadFile(
  name: string,
  contents: string,
  type = 'application/json',
) {
  const url = URL.createObjectURL(new Blob([contents], { type }));
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = name;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
export function errorMessage(error: unknown) {
  return typeof error === 'string'
    ? error
    : error instanceof Error
      ? error.message
      : 'Cette opération a échoué.';
}
