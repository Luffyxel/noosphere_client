export type GitHubViewer = {
  id: number;
  login: string;
  avatarUrl: string;
  name: string | null;
  repository: {
    id: number;
    name: string;
    url: string;
    created: boolean;
  };
};

export type NoosphereUser = {
  id: number;
  login: string;
  name: string | null;
  avatarUrl: string;
  repository: string;
};

export type UserLookup =
  | { found: false }
  | {
      found: true;
      registered: boolean;
      user: Omit<NoosphereUser, 'repository'> & { repository: string | null };
    };

export type Conversation = {
  version: 1;
  id: string;
  type: 'direct';
  createdAt: string;
  peer: NoosphereUser;
};

export type FriendRequest = {
  version: 1;
  id: string;
  direction: 'incoming' | 'outgoing';
  user: NoosphereUser;
};

export type SocialState = {
  conversations: Conversation[];
  incoming: FriendRequest[];
  outgoing: FriendRequest[];
};

export type WakeSignals = {
  conversationIds: string[];
  socialChanged: boolean;
};

export type Message = {
  version: 1;
  id: string;
  conversationId: string;
  sentAt: string;
  text: string;
  senderId: number;
  own: boolean;
};

export type RealtimeSignal = {
  version: 1;
  kind: 'offer' | 'answer';
  sessionId: string;
  createdAt: string;
  expiresAt: string;
  sdp: string;
};

export type NoosphereDesktopApi = {
  isDesktop: true;
  platform: string;
  github: {
    restore: () => Promise<GitHubViewer | null>;
    validateSession: () => Promise<GitHubViewer | null>;
    connect: (consent: GitHubProvisioningConsent) => Promise<GitHubViewer>;
    onDeviceCode: (
      callback: (value: { userCode: string } | null) => void,
    ) => () => void;
    onSetupStatus: (
      callback: (value: GitHubSetupStatus | null) => void,
    ) => () => void;
    signOut: () => Promise<{ signedOut: true }>;
  };
  noosphere: {
    lookupUser: (login: string) => Promise<UserLookup>;
    sendFriendRequest: (login: string) => Promise<FriendRequest>;
    cachedState: () => Promise<SocialState>;
    syncState: () => Promise<SocialState>;
    pollWakeSignals: () => Promise<WakeSignals>;
    acceptFriendRequest: (
      login: string,
      conversationId: string,
    ) => Promise<Conversation>;
    declineFriendRequest: (
      conversationId: string,
    ) => Promise<{ declined: true }>;
    removeFriend: (conversationId: string) => Promise<{ removed: true }>;
    sendMessage: (conversationId: string, text: string) => Promise<Message>;
    signalWake: (conversationId: string) => Promise<{ published: true }>;
    listMessages: (conversationId: string) => Promise<Message[]>;
    publishRealtimeSignal: (
      conversationId: string,
      signal: RealtimeSignal,
    ) => Promise<{ published: true }>;
    readRealtimeSignal: (
      conversationId: string,
    ) => Promise<RealtimeSignal | null>;
  };
  system: {
    notify: (title: string, body: string) => Promise<{ shown: boolean }>;
  };
};

export type GitHubProvisioningConsent = {
  version: 1;
  createPublicRepository: true;
  restrictInstallationToRepository: true;
};

export type GitHubSetupStatus = {
  stage: 'authorization' | 'repository' | 'installation' | 'initialization';
  repositoryName?: string;
};

declare global {
  interface Window {
    noosphereDesktop?: NoosphereDesktopApi;
  }
}
