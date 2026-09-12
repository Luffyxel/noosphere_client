import { invoke } from '@tauri-apps/api/core';
import type { RemoteDirectory, RemoteStatus } from '@/lib/remote-access';
import {
  listen,
  type EventCallback,
  type UnlistenFn,
} from '@tauri-apps/api/event';

import type {
  CallSignal,
  Conversation,
  FriendRequest,
  GitHubProvisioningConsent,
  GitHubSetupStatus,
  GitHubViewer,
  Message,
  NoosphereDesktopApi,
  RealtimeSignal,
  SocialState,
  UserLookup,
  WakeSignals,
} from '@/lib/desktop-bridge';

function subscribe<T>(event: string, callback: EventCallback<T>): () => void {
  let active = true;
  let remove: UnlistenFn | undefined;
  void listen<T>(event, callback).then((unlisten) => {
    if (active) remove = unlisten;
    else unlisten();
  });
  return () => {
    active = false;
    remove?.();
  };
}

export function installTauriBridge(): void {
  const bridge: NoosphereDesktopApi = {
    remoteAccess: {
      cancelAccess: (principal) =>
        invoke<RemoteDirectory>('remote_cancel_access', { principal }),
      directory: () => invoke<RemoteDirectory>('remote_directory'),
      syncDirectory: (refresh = false) =>
        invoke<RemoteDirectory>('remote_sync_directory', { refresh }),
      setHost: (enabled) =>
        invoke<RemoteDirectory>('remote_set_host', { enabled }),
      setGrant: (principal, permissions, unattended, revoke = false) =>
        invoke<RemoteDirectory>('remote_set_grant', {
          principal,
          permissions,
          unattended,
          revoke,
        }),
      setUserGrant: (githubUserId, permissions, unattended, revoke = false) =>
        invoke<RemoteDirectory>('remote_set_user_grant', {
          githubUserId,
          permissions,
          unattended,
          revoke,
        }),
      requestAccess: (principal, permissions) =>
        invoke<RemoteDirectory>('remote_request_access', {
          principal,
          permissions,
        }),
      connectMachine: (principal) =>
        invoke<RemoteDirectory>('remote_connect_machine', { principal }),
      stopSession: () => invoke<void>('remote_stop_session'),
      decideAccess: (requestId, permissions, remember, unattended) =>
        invoke<RemoteDirectory>('remote_decide_access', {
          requestId,
          permissions,
          remember,
          unattended,
        }),
      status: () => invoke<RemoteStatus>('remote_status'),
      saveSettings: (settings) =>
        invoke<RemoteStatus>('remote_save_settings', { settings }),
      startLocalTest: (settings) =>
        invoke<void>('remote_start_local_test', { settings }),
      stopLocalTest: () => invoke<void>('remote_stop_local_test'),
    },
    isDesktop: true,
    platform: navigator.userAgent.includes('Windows') ? 'win32' : 'linux',
    github: {
      restore: () => invoke<GitHubViewer | null>('github_restore'),
      validateSession: () =>
        invoke<GitHubViewer | null>('github_validate_session'),
      connect: (consent: GitHubProvisioningConsent) =>
        invoke<GitHubViewer>('github_connect', { consent }),
      onDeviceCode: (callback) =>
        subscribe<{ userCode: string } | null>('github-device-code', (event) =>
          callback(event.payload),
        ),
      onSetupStatus: (callback) =>
        subscribe<GitHubSetupStatus | null>('github-setup-status', (event) =>
          callback(event.payload),
        ),
      signOut: () => invoke<{ signedOut: true }>('github_sign_out'),
    },
    noosphere: {
      lookupUser: (login: string) =>
        invoke<UserLookup>('noosphere_lookup_user', { login }),
      sendFriendRequest: (login: string) =>
        invoke<FriendRequest>('noosphere_send_friend_request', { login }),
      cachedState: () => invoke<SocialState>('noosphere_cached_state'),
      syncState: () => invoke<SocialState>('noosphere_sync_state'),
      syncRequests: () => invoke<SocialState>('noosphere_sync_requests'),
      pollWakeSignals: () => invoke<WakeSignals>('noosphere_poll_wake_signals'),
      acceptFriendRequest: (login: string, conversationId: string) =>
        invoke<Conversation>('noosphere_accept_friend_request', {
          login,
          conversationId,
        }),
      declineFriendRequest: (conversationId: string) =>
        invoke<{ declined: true }>('noosphere_decline_friend_request', {
          conversationId,
        }),
      removeFriend: (conversationId: string) =>
        invoke<{ removed: true }>('noosphere_remove_friend', {
          conversationId,
        }),
      sendMessage: (conversationId: string, text: string) =>
        invoke<Message>('noosphere_send_message', { conversationId, text }),
      signalWake: (conversationId: string) =>
        invoke<{ published: true }>('noosphere_signal_wake', {
          conversationId,
        }),
      signalCall: (conversationId: string, callId: string) =>
        invoke<CallSignal>('noosphere_signal_call', {
          conversationId,
          callId,
        }),
      readCallSignal: (conversationId: string) =>
        invoke<CallSignal | null>('noosphere_read_call_signal', {
          conversationId,
        }),
      listMessages: (conversationId: string) =>
        invoke<Message[]>('noosphere_list_messages', { conversationId }),
      publishRealtimeSignal: (conversationId: string, signal: RealtimeSignal) =>
        invoke<{ published: true }>('noosphere_publish_realtime_signal', {
          conversationId,
          signal,
        }),
      readRealtimeSignal: (conversationId: string) =>
        invoke<RealtimeSignal | null>('noosphere_read_realtime_signal', {
          conversationId,
        }),
    },
    system: {
      notify: (title: string, body: string) =>
        invoke<{ shown: boolean }>('system_notify', { title, body }),
    },
  };

  Object.defineProperty(window, 'noosphereDesktop', {
    configurable: false,
    enumerable: false,
    value: Object.freeze(bridge),
    writable: false,
  });
}
