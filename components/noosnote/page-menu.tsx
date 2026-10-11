import { useState, type ReactElement, type ReactNode } from 'react';
import { ContextMenu } from '@base-ui/react/context-menu';
import {
  Copy,
  Download,
  FilePlus2,
  FolderInput,
  MousePointer2,
  Pencil,
  Smile,
  Trash2,
} from 'lucide-react';
import { getWorkspace } from '@/lib/noosnote/model';
import { useNoteContext, type PageAction } from './context';

const actions = [
  { id: 'open', label: 'Ouvrir', icon: MousePointer2 },
  { id: 'rename', label: 'Renommer', icon: Pencil },
  { id: 'icon', label: 'Changer l’icône', icon: Smile },
  { id: 'child', label: 'Ajouter une sous-page', icon: FilePlus2 },
  { id: 'duplicate', label: 'Dupliquer avec les sous-pages', icon: Copy },
  { id: 'move', label: 'Déplacer', icon: FolderInput },
  { id: 'export', label: 'Exporter la page', icon: Download },
  { id: 'trash', label: 'Mettre à la corbeille', icon: Trash2 },
] satisfies { id: PageAction; label: string; icon: typeof Copy }[];

export function PageContextMenu({
  pageId,
  children,
}: {
  pageId: string;
  children: ReactElement;
}) {
  return (
    <ContextMenu.Trigger
      render={children}
      data-note-page={pageId}
      onKeyDown={(event) => {
        if (
          event.key !== 'ContextMenu' &&
          !(event.shiftKey && event.key === 'F10')
        )
          return;
        event.preventDefault();
        const bounds = event.currentTarget.getBoundingClientRect();
        event.currentTarget.dispatchEvent(
          new MouseEvent('contextmenu', {
            bubbles: true,
            cancelable: true,
            clientX: bounds.left + 20,
            clientY: bounds.top + bounds.height / 2,
          }),
        );
      }}
    />
  );
}

export function PageMenuProvider({ children }: { children: ReactNode }) {
  const [pageId, setPageId] = useState('');
  const { notes, workspaceId, pageAction } = useNoteContext();
  const entry = getWorkspace(notes.files, workspaceId)?.pages.find(
    (page) => page.id === pageId,
  );
  return (
    <ContextMenu.Root
      onOpenChange={(open, details) => {
        if (!open) return;
        const target = details.event.target;
        if (target instanceof Element) {
          const id = target
            .closest('[data-note-page]')
            ?.getAttribute('data-note-page');
          if (id) setPageId(id);
        }
      }}
    >
      {children}
      <ContextMenu.Portal>
        <ContextMenu.Positioner className="note-menu-positioner" sideOffset={4}>
          <ContextMenu.Popup
            className="note-context-menu"
            aria-label={`Actions de ${entry?.title || 'la page'}`}
          >
            {actions.map((action) => (
              <ContextMenu.Item
                key={action.id}
                className={action.id === 'trash' ? 'note-danger' : undefined}
                disabled={action.id === 'move' && !!entry?.databaseId}
                onClick={() => pageAction(action.id, pageId)}
              >
                <action.icon size={15} /> {action.label}
              </ContextMenu.Item>
            ))}
          </ContextMenu.Popup>
        </ContextMenu.Positioner>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
