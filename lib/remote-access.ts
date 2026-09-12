import type {
  Conversation,
  GitHubViewer,
  NoosphereUser,
} from './desktop-bridge';

export type RemoteVideoSettings = {
  width: number;
  height: number;
  fps: number;
  bitrate: number;
  codec: 'h264' | 'hevc' | 'av1';
};

export type RemoteSettings = {
  machineName: string;
  display: number;
  video: RemoteVideoSettings;
  audio: boolean;
};

export type RemoteStatus = {
  diagnostic: RemoteDiagnostic;
  session: RemoteSession;
  owner: RemotePrincipal | null;
  protocolVersion: number;
  pid: number;
  hostEnabled: boolean;
  streamingReady: boolean;
  settings: RemoteSettings;
  displays: { id: number; name: string; width: number; height: number }[];
  unavailable: string[];
  logs: RemoteLog[];
};

export type RemoteAccessApi = {
  cancelAccess: (principal: RemotePrincipal) => Promise<RemoteDirectory>;
  directory: () => Promise<RemoteDirectory>;
  syncDirectory: (refresh?: boolean) => Promise<RemoteDirectory>;
  setHost: (enabled: boolean) => Promise<RemoteDirectory>;
  setGrant: (
    principal: RemotePrincipal,
    permissions: RemotePermissions,
    unattended: boolean,
    revoke?: boolean,
  ) => Promise<RemoteDirectory>;
  setUserGrant: (
    githubUserId: number,
    permissions: RemotePermissions,
    unattended: boolean,
    revoke?: boolean,
  ) => Promise<RemoteDirectory>;
  requestAccess: (
    principal: RemotePrincipal,
    permissions: RemotePermissions,
  ) => Promise<RemoteDirectory>;
  connectMachine: (principal: RemotePrincipal) => Promise<RemoteDirectory>;
  stopSession: () => Promise<void>;
  decideAccess: (
    requestId: number[],
    permissions: RemotePermissions,
    remember: boolean,
    unattended: boolean,
  ) => Promise<RemoteDirectory>;
  status: () => Promise<RemoteStatus>;
  saveSettings: (settings: RemoteSettings) => Promise<RemoteStatus>;
  startLocalTest: (settings: RemoteSettings) => Promise<void>;
  stopLocalTest: () => Promise<void>;
};

export type RemotePrincipal = {
  githubUserId: number;
  identityKey: number[];
  machineId: number[];
};
export type RemotePermissions = {
  screen: boolean;
  keyboard: boolean;
  mouse: boolean;
  gamepad: boolean;
  clipboard: boolean;
  audio: boolean;
};
export const NO_PERMISSIONS: RemotePermissions = {
  screen: false,
  keyboard: false,
  mouse: false,
  gamepad: false,
  clipboard: false,
  audio: false,
};
export const DESKTOP_PERMISSIONS: RemotePermissions = {
  ...NO_PERMISSIONS,
  screen: true,
  keyboard: true,
  mouse: true,
};
export function editablePermissions(
  permissions: RemotePermissions,
): RemotePermissions {
  return {
    ...NO_PERMISSIONS,
    screen: permissions.screen,
    keyboard: permissions.keyboard,
    mouse: permissions.mouse,
  };
}
export type RemoteGrant = {
  principal: RemotePrincipal;
  hostMachineId: number[];
  permissions: RemotePermissions;
  unattended: boolean;
};
export type RemoteUserGrant = {
  githubUserId: number;
  permissions: RemotePermissions;
  unattended: boolean;
};
export type RemoteMachine = {
  principal: RemotePrincipal;
  login: string;
  avatarUrl: string;
  name: string;
  platform: string;
  identityVerified: boolean;
  hostEnabled: boolean;
  permissions: RemotePermissions;
  unattended: boolean;
  lastSeen: number;
};
export type RemoteUser = Pick<
  NoosphereUser,
  'id' | 'login' | 'name' | 'avatarUrl'
> & {
  own: boolean;
  machines: RemoteMachine[];
};
export type RemoteRequest = {
  id: number[];
  guest: RemotePrincipal;
  host: RemotePrincipal;
  permissions: RemotePermissions;
  createdAt: number;
  expiresAt: number;
};
export type RemoteDecision = {
  request: RemoteRequest;
  decision: 'pending' | 'approved' | 'declined' | 'revoked';
  permissions: RemotePermissions;
  automatic: boolean;
};
export type RemoteLog = {
  at: number;
  level: string;
  scope: string;
  message: string;
};
export type RemoteDirectory = {
  owner: RemotePrincipal;
  registered: boolean;
  hostEnabled: boolean;
  machines: RemoteMachine[];
  grants: RemoteGrant[];
  userGrants: RemoteUserGrant[];
  incoming: RemoteDecision[];
  outgoing: RemoteRequest[];
  answers: RemoteDecision[];
  streamingReady: boolean;
  connecting: boolean;
  syncErrors: string[];
  logs: RemoteLog[];
};

export type RemoteSession =
  | { state: 'idle' | 'preparing' | 'waiting' | 'connecting' }
  | {
      state: 'streaming';
      peer: RemotePrincipal;
      frames: number;
      rttUs: number;
    }
  | { state: 'failed'; error: string };
export function machineId(principal: RemotePrincipal): string {
  return principal.machineId
    .map((n) => n.toString(16).padStart(2, '0'))
    .join('');
}
export function principalId(principal: RemotePrincipal): string {
  return `${principal.githubUserId}:${machineId(principal)}:${principal.identityKey.join('.')}`;
}
export function samePrincipal(a: RemotePrincipal, b: RemotePrincipal): boolean {
  return principalId(a) === principalId(b);
}
export function machineOnline(
  machine: RemoteMachine,
  now = Date.now(),
): boolean {
  return machine.lastSeen * 1000 + 120_000 > now;
}
export function groupMachines(
  machines: RemoteMachine[],
  owner: RemotePrincipal,
) {
  const others = machines.filter(
    (m) => machineId(m.principal) !== machineId(owner),
  );
  return {
    own: others.filter((m) => m.principal.githubUserId === owner.githubUserId),
    friends: others.filter(
      (m) => m.principal.githubUserId !== owner.githubUserId,
    ),
  };
}

export function remoteUsers(
  viewer: Pick<GitHubViewer, 'id' | 'login' | 'name' | 'avatarUrl'>,
  friends: Pick<Conversation, 'peer'>[],
  machines: RemoteMachine[],
): RemoteUser[] {
  const accounts = new Map<number, typeof viewer | Conversation['peer']>([
    [viewer.id, viewer],
    ...friends.map(({ peer }) => [peer.id, peer] as const),
  ]);
  return Array.from(accounts.values()).map((peer) => ({
    id: peer.id,
    login: peer.login,
    name: peer.name,
    avatarUrl: peer.avatarUrl,
    own: peer.id === viewer.id,
    machines: machines.filter(
      (machine) => machine.principal.githubUserId === peer.id,
    ),
  }));
}

export function userGrants(
  user: Pick<RemoteUser, 'id' | 'machines'>,
  grants: RemoteGrant[],
  policies: RemoteUserGrant[] = [],
) {
  const policy = policies.find((grant) => grant.githubUserId === user.id);
  const known = user.machines
    .map((machine) =>
      grants.find((grant) => samePrincipal(grant.principal, machine.principal)),
    )
    .filter((grant): grant is RemoteGrant => Boolean(grant));
  if (policy) {
    return {
      permissions: editablePermissions(policy.permissions),
      unattended: policy.unattended,
      allowedMachines: known.filter((grant) => grant.permissions.screen).length,
      configured: policy.permissions.screen,
    };
  }
  const allAllowed =
    user.machines.length > 0 &&
    known.length === user.machines.length &&
    known.every((grant) => grant.permissions.screen);
  return {
    permissions: {
      ...NO_PERMISSIONS,
      screen: allAllowed,
      keyboard:
        allAllowed && known.every((grant) => grant.permissions.keyboard),
      mouse: allAllowed && known.every((grant) => grant.permissions.mouse),
    },
    unattended: allAllowed && known.every((grant) => grant.unattended),
    allowedMachines: known.filter((grant) => grant.permissions.screen).length,
    configured: allAllowed,
  };
}

export type RemoteDiagnostic =
  | { state: 'idle' | 'running' }
  | { state: 'failed'; error: string }
  | { state: 'complete'; report: RemoteDiagnosticReport };

export type RemoteDiagnosticReport = {
  encoder: string;
  width: number;
  height: number;
  encodedFrames: number;
  deliveredFrames: number;
  presentedFrames: number;
  droppedFrames: number;
  encodeP50Us: number;
  encodeP95Us: number;
  networkP50Us: number;
  networkP95Us: number;
  decodeSubmitP50Us: number;
  decodeSubmitP95Us: number;
  rttUs: number;
  durationMs: number;
  inputEventsReceived: number;
  inputKeyObserved: boolean;
  inputMouseObserved: boolean;
  revocationVerified: boolean;
  inputToPhotonUs: number | null;
};
