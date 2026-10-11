import { createRoot } from 'react-dom/client';
import { useState } from 'react';
import {
  CreateNoosnoteDialog,
  NoosnoteRail,
  NoosnoteWorkspace,
} from '@/components/noosnote/workspace';
import { applyFiles, createWorkspace, getPage } from '@/lib/noosnote/model';
import type {
  NoosnoteApi,
  NoteFiles,
  NoteSnapshot,
} from '@/lib/noosnote/types';
import type { NoosphereDesktopApi } from '@/lib/desktop-bridge';
import { useNoosnote } from '@/lib/noosnote/use-noosnote';
import '@/app/globals.css';

const school = createWorkspace('École', '📚', '#78b89b', 'school');
let files: NoteFiles = applyFiles({}, school.changes);
let dirtyPaths: string[] = [];
const repository = {
  id: 421,
  name: 'noosnote_42',
  url: 'https://github.com/octocat/noosnote_42',
  branch: 'main',
};
function snapshot(partial?: NoteFiles): NoteSnapshot {
  return {
    files: structuredClone(partial ?? files),
    incremental: !!partial,
    deletedPaths: [],
    dirtyPaths,
    dirtyCount: dirtyPaths.length,
    repository,
    conflicts: [],
    lastSyncedAt: null,
    setup: 'ready',
  };
}
const api: NoosnoteApi = {
  cached: async () => snapshot(),
  discover: async () => snapshot(),
  setup: async () => snapshot(),
  openSetup: async () => {},
  page: async (workspaceId, pageId) => {
    const page = getPage(files, workspaceId, pageId);
    if (!page) throw new Error('Page introuvable.');
    return snapshot({ [`notes/${workspaceId}/pages/${pageId}.json`]: page });
  },
  save: async (changes) => {
    files = applyFiles(files, changes);
    dirtyPaths = [
      ...new Set([...dirtyPaths, ...changes.map((change) => change.path)]),
    ];
    return snapshot();
  },
  sync: async () => {
    dirtyPaths = [];
    return { ...snapshot(), lastSyncedAt: new Date().toISOString() };
  },
  resolve: async () => snapshot(),
};
window.noosphereDesktop = { noosnote: api } as NoosphereDesktopApi;

function Fixture() {
  const notes = useNoosnote(42, true);
  const [workspaceId, setWorkspaceId] = useState(school.workspaceId);
  const [pageId, setPageId] = useState<string | null>(school.pageId);
  const [create, setCreate] = useState(false);
  return (
    <main
      style={{
        display: 'flex',
        height: '100vh',
        background: '#1c1c1e',
        color: '#f5f5f7',
        fontFamily: 'Segoe UI, sans-serif',
      }}
    >
      <aside
        style={{
          width: 72,
          flexShrink: 0,
          padding: '16px 14px',
          background: '#151517',
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        <NoosnoteRail
          notes={notes}
          selected={workspaceId}
          onSelect={(id) => {
            setWorkspaceId(id);
            setPageId(null);
          }}
        />
        <button
          aria-label="Créer un Noosnote"
          onClick={() => setCreate(true)}
          style={{ color: '#78b89b', fontSize: 26 }}
        >
          +
        </button>
      </aside>
      <NoosnoteWorkspace
        key={`${workspaceId}:${pageId}`}
        notes={notes}
        workspaceId={workspaceId}
        initialPageId={pageId}
        onCreate={() => setCreate(true)}
      />
      <CreateNoosnoteDialog
        key={String(create)}
        notes={notes}
        open={create}
        onClose={() => setCreate(false)}
        onCreated={(id, pageId) => {
          setWorkspaceId(id);
          setPageId(pageId);
        }}
      />
    </main>
  );
}
createRoot(document.getElementById('root')!).render(<Fixture />);
