export type Team = {
  id: string;
  repositoryId: number;
  ownerId: number;
  owner: string;
  name: string;
  branch: string;
  title: string;
  canEdit: boolean;
  canManage: boolean;
};
export type NoteMember = {
  id: number;
  login: string;
  name: string;
  avatarUrl: string;
  canEdit: boolean;
  owner: boolean;
};
export type NoteComment = {
  id: string;
  body: string;
  author: Pick<NoteMember, 'id' | 'login' | 'name' | 'avatarUrl'>;
  createdAt: string;
  editedAt?: string;
  deletedAt?: string;
};
export type NoteThread = {
  version: 1;
  kind: 'thread';
  id: string;
  workspaceId: string;
  pageId: string;
  title: string;
  quote?: string;
  blockId?: string;
  resolved: boolean;
  comments: NoteComment[];
};
export type NoteRevision = {
  id: string;
  url: string;
  name: string;
  date: string;
  message: string;
  author: { id: number; login: string; avatar_url: string } | null;
};
export type TeamStatus = {
  team: Team;
  members: NoteMember[];
  activity: unknown[];
};
export type TeamPreparation = {
  stage: 'repository' | 'installation' | 'ready';
  team: Team | null;
  url: string | null;
};
export type NoosnoteTeamsApi = {
  list: () => Promise<Team[]>;
  join: (link: string) => Promise<Team>;
  prepare: (workspaceId: string, create: boolean) => Promise<TeamPreparation>;
  status: (teamId: string) => Promise<TeamStatus>;
  open: (
    teamId: string,
    mode:
      | 'repository'
      | 'access'
      | 'installation'
      | 'setup-repository'
      | 'setup-installation'
      | `revision:${string}`,
  ) => Promise<void>;
  history: (teamId: string, pageId?: string) => Promise<NoteRevision[]>;
  leave: (teamId: string) => Promise<void>;
  notes: (team: Pick<Team, 'id' | 'canEdit'>) => import('./types').NoosnoteApi;
};
