import { createRoot } from 'react-dom/client';
import type { GitHubViewer, NoosphereDesktopApi } from '@/lib/desktop-bridge';
import type {
  NoosnoteApi,
  NoteFiles,
  NoteSnapshot,
} from '@/lib/noosnote/types';
import type { Team } from '@/lib/noosnote/team-types';
import { applyFiles } from '@/lib/noosnote/model';
import { TeamNoosnote } from '@/components/noosnote/team';
import '@/app/globals.css';
const actor = Number(new URLSearchParams(location.search).get('actor') || 1);
const names = ['Alice', 'Bob', 'Clara', 'Lecteur'];
const viewer: GitHubViewer = {
  id: actor,
  login: names[actor - 1].toLowerCase(),
  name: names[actor - 1],
  avatarUrl: '',
  repository: {
    id: 50,
    name: 'noosphere',
    url: 'https://github.com/alice/noosphere',
    created: false,
  },
};
const team: Team = {
  id: '0f4fbbe5-3051-4944-a537-0342c58250ad',
  repositoryId: 100,
  ownerId: 1,
  owner: 'alice',
  name: 'noosnote_team_0f4fbbe5-3051-4944-a537-0342c58250ad',
  branch: 'main',
  title: 'Atelier · Projet commun',
  canEdit: actor !== 4,
  canManage: actor === 1,
};
let files: NoteFiles = {};
const dirty = new Set<string>();
function snapshot(): NoteSnapshot {
  return {
    files: structuredClone(files),
    incremental: false,
    deletedPaths: [],
    repository: {
      id: 100,
      name: team.name,
      url: `https://github.com/alice/${team.name}`,
      branch: 'main',
    },
    dirtyCount: dirty.size,
    dirtyPaths: [...dirty],
    conflicts: [],
    lastSyncedAt: new Date().toISOString(),
    setup: 'ready',
  };
}
async function rpc(method: string, value: unknown = {}) {
  const response = await fetch('/team-test-api', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ method, actor, value }),
  });
  if (!response.ok) throw new Error(await response.text());
  return response.json();
}
const api: NoosnoteApi = {
  shared: true,
  readOnly: actor === 4,
  cached: async () => {
    files = await rpc('files');
    return snapshot();
  },
  discover: async () => snapshot(),
  setup: async () => snapshot(),
  openSetup: async () => {},
  page: async () => snapshot(),
  save: async (changes) => {
    if (actor === 4) throw new Error('Lecture seule');
    files = applyFiles(files, changes);
    for (const change of changes) dirty.add(change.path);
    return snapshot();
  },
  sync: async () => {
    const sent = [...dirty];
    files = await rpc(
      'sync',
      sent.map((path) => ({ path, value: files[path] ?? null })),
    );
    for (const path of sent) dirty.delete(path);
    return snapshot();
  },
  resolve: async () => snapshot(),
  importFile: async () => {
    throw new Error('Not available');
  },
  readFile: async () => '',
};
window.noosphereDesktop = {
  noosnote: api,
  noosnoteTeams: {
    notes: () => api,
    list: async () => [team],
    status: async () => ({
      team,
      members: [1, 2, 3, 4].map((id) => ({
        id,
        login: names[id - 1].toLowerCase(),
        name: names[id - 1],
        avatarUrl: '',
        canEdit: id !== 4,
        owner: id === 1,
      })),
      activity: [],
    }),
    history: async () => [
      {
        id: 'f'.repeat(40),
        url: '',
        author: { id: 1, login: 'alice', avatar_url: '' },
        name: 'Alice',
        date: '2026-10-18T09:20:00Z',
        message: 'Update shared Noosnote workspace',
      },
    ],
    open: async () => {},
    join: async () => team,
    prepare: async () => ({ stage: 'ready', team, url: null }),
    leave: async () => {},
  },
} as unknown as NoosphereDesktopApi;
createRoot(document.getElementById('root')!).render(
  <main
    style={{
      display: 'flex',
      height: '100vh',
      background: '#18242f',
      color: '#d6e1e8',
      fontFamily: 'Segoe UI, sans-serif',
    }}
  >
    <TeamNoosnote
      viewer={viewer}
      team={team}
      onCreate={() => {}}
      onUpdated={() => {}}
    />
  </main>,
);
