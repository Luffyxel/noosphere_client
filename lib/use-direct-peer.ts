'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

import type {
  Conversation,
  GitHubViewer,
  RealtimeSignal,
} from '@/lib/desktop-bridge';
import { captureCallMedia, captureCamera } from '@/lib/media-capture';
import {
  createVoiceCallId,
  createVoiceCallPacket,
  parseVoiceCallPacket,
  type MediaDeviceSettings,
  type PendingCallAction,
  type VoiceCallMedia,
  type VoiceCallPacketType,
  type VoiceCallStatus,
} from '@/lib/voice-call';

export type DirectPeerStatus = 'connecting' | 'direct' | 'github';

const SIGNAL_POLL_INTERVAL_MS = 3_000;
const SIGNAL_POLL_MAX_INTERVAL_MS = 20_000;
const DIRECT_CONNECTION_GRACE_MS = 9_000;
const OFFER_RENEWAL_MS = 60_000;
const SIGNAL_LIFETIME_MS = 2 * 60_000;
const CALL_RESPONSE_TIMEOUT_MS = 45_000;

type WakePacket = {
  version: 1;
  type: 'message';
  conversationId: string;
  id: string;
};

function waitForIceGathering(peer: RTCPeerConnection) {
  if (peer.iceGatheringState === 'complete') return Promise.resolve();
  return new Promise<void>((resolve) => {
    const timeout = window.setTimeout(finish, 8_000);
    function finish() {
      window.clearTimeout(timeout);
      peer.removeEventListener('icegatheringstatechange', changed);
      resolve();
    }
    function changed() {
      if (peer.iceGatheringState === 'complete') finish();
    }
    peer.addEventListener('icegatheringstatechange', changed);
  });
}

function createSessionId() {
  return `rtc-${crypto.randomUUID().replaceAll('-', '')}`;
}

function isWakePacket(
  value: unknown,
  conversationId: string,
): value is WakePacket {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const packet = value as Record<string, unknown>;
  return (
    Object.keys(packet).sort().join(',') === 'conversationId,id,type,version' &&
    packet.version === 1 &&
    packet.type === 'message' &&
    packet.conversationId === conversationId &&
    typeof packet.id === 'string' &&
    /^msg-[a-f0-9]{32}$/.test(packet.id)
  );
}

async function selectAudioOutput(audio: HTMLAudioElement, deviceId: string) {
  const element = audio as HTMLAudioElement & {
    setSinkId?: (sinkId: string) => Promise<void>;
  };
  if (deviceId && element.setSinkId) await element.setSinkId(deviceId);
}

export function useDirectPeer(
  viewer: GitHubViewer,
  conversation: Conversation | null,
  onMessageSignal: () => void,
  mediaSettings: MediaDeviceSettings,
  onIncomingCall: (callId: string) => PendingCallAction = () => null,
) {
  const [status, setStatus] = useState<DirectPeerStatus>('github');
  const [callStatus, setCallStatus] = useState<VoiceCallStatus>('idle');
  const [callError, setCallError] = useState('');
  const [localStream, setLocalStream] = useState<MediaStream | null>(null);
  const [remoteStream, setRemoteStream] = useState<MediaStream | null>(null);
  const [microphoneEnabled, setMicrophoneEnabled] = useState(false);
  const [cameraEnabled, setCameraEnabled] = useState(false);
  const channelRef = useRef<RTCDataChannel | null>(null);
  const peerRef = useRef<RTCPeerConnection | null>(null);
  const audioSenderRef = useRef<RTCRtpSender | null>(null);
  const videoSenderRef = useRef<RTCRtpSender | null>(null);
  const localStreamRef = useRef<MediaStream | null>(null);
  const remoteStreamRef = useRef<MediaStream | null>(null);
  const remoteAudioRef = useRef<HTMLAudioElement | null>(null);
  const activeCallIdRef = useRef<string | null>(null);
  const activeCallModeRef = useRef<VoiceCallMedia>('audio');
  const callStatusRef = useRef<VoiceCallStatus>('idle');
  const callTimerRef = useRef<number | null>(null);
  const mediaSettingsRef = useRef(mediaSettings);
  const onMessageSignalRef = useRef(onMessageSignal);
  const onIncomingCallRef = useRef(onIncomingCall);
  const sendVoicePacketRef = useRef<
    (
      type: VoiceCallPacketType,
      callId: string,
      media: VoiceCallMedia,
    ) => boolean
  >(() => false);
  const conversationId = conversation?.id ?? null;
  const peerId = conversation?.peer.id ?? null;

  const updateCallStatus = useCallback((next: VoiceCallStatus) => {
    callStatusRef.current = next;
    setCallStatus(next);
  }, []);

  const clearCallTimer = useCallback(() => {
    if (callTimerRef.current !== null) {
      window.clearTimeout(callTimerRef.current);
      callTimerRef.current = null;
    }
  }, []);

  const stopLocalMedia = useCallback(() => {
    void audioSenderRef.current?.replaceTrack(null);
    void videoSenderRef.current?.replaceTrack(null);
    for (const track of localStreamRef.current?.getTracks() ?? []) track.stop();
    localStreamRef.current = null;
    setLocalStream(null);
    setMicrophoneEnabled(false);
    setCameraEnabled(false);
  }, []);

  const resetVoiceCall = useCallback(
    (error = '') => {
      clearCallTimer();
      stopLocalMedia();
      activeCallIdRef.current = null;
      activeCallModeRef.current = 'audio';
      updateCallStatus('idle');
      setCallError(error);
    },
    [clearCallTimer, stopLocalMedia, updateCallStatus],
  );

  const enableLocalMedia = useCallback(async (media: VoiceCallMedia) => {
    if (!peerRef.current || !audioSenderRef.current) {
      throw new Error('Connexion vocale indisponible.');
    }
    const capture = await captureCallMedia(media, mediaSettingsRef.current);
    const audioTrack = capture.stream.getAudioTracks()[0] ?? null;
    const videoTrack = capture.stream.getVideoTracks()[0] ?? null;
    try {
      await audioSenderRef.current.replaceTrack(audioTrack);
      if (videoSenderRef.current) {
        await videoSenderRef.current.replaceTrack(videoTrack);
      }
    } catch (error) {
      for (const track of capture.stream.getTracks()) track.stop();
      throw error;
    }
    for (const track of localStreamRef.current?.getTracks() ?? []) track.stop();
    localStreamRef.current = capture.stream;
    setLocalStream(capture.stream);
    setMicrophoneEnabled(capture.microphone);
    setCameraEnabled(capture.camera);
    return capture.warning;
  }, []);

  useEffect(() => {
    onMessageSignalRef.current = onMessageSignal;
  }, [onMessageSignal]);

  useEffect(() => {
    onIncomingCallRef.current = onIncomingCall;
  }, [onIncomingCall]);

  useEffect(() => {
    mediaSettingsRef.current = mediaSettings;
    const audio = remoteAudioRef.current;
    if (audio) void selectAudioOutput(audio, mediaSettings.audioOutputId);
  }, [mediaSettings]);

  const acceptCall = useCallback(async () => {
    const callId = activeCallIdRef.current;
    const media = activeCallModeRef.current;
    if (!callId || callStatusRef.current !== 'incoming') return false;
    clearCallTimer();
    updateCallStatus('connecting');
    setCallError('');
    try {
      const warning = await enableLocalMedia(media);
      if (activeCallIdRef.current !== callId) return false;
      if (!sendVoicePacketRef.current('call-accept', callId, media)) {
        throw new Error('Connexion interrompue.');
      }
      setCallError(warning);
      updateCallStatus('connected');
      return true;
    } catch (error) {
      sendVoicePacketRef.current('call-decline', callId, media);
      resetVoiceCall(
        error instanceof Error
          ? error.message
          : 'Aucun périphérique multimédia n’est accessible.',
      );
      return false;
    }
  }, [clearCallTimer, enableLocalMedia, resetVoiceCall, updateCallStatus]);

  useEffect(() => {
    const desktop = window.noosphereDesktop;
    if (
      !desktop ||
      !conversationId ||
      !peerId ||
      typeof RTCPeerConnection === 'undefined'
    ) {
      channelRef.current = null;
      const statusTimer = window.setTimeout(() => {
        setStatus('github');
        resetVoiceCall();
      }, 0);
      return () => window.clearTimeout(statusTimer);
    }
    const desktopApi = desktop;
    const activeConversationId = conversationId;
    const activePeerId = peerId;
    const remoteAudio = new Audio();
    remoteAudio.autoplay = true;
    remoteAudioRef.current = remoteAudio;
    void selectAudioOutput(remoteAudio, mediaSettingsRef.current.audioOutputId);

    let active = true;
    let polling = false;
    let peer: RTCPeerConnection | null = null;
    let channel: RTCDataChannel | null = null;
    let offerSessionId: string | null = null;
    let offerPublishedAt = 0;
    let handledOfferId: string | null = null;
    let consecutiveFailures = 0;
    let pollTimer: number | null = null;
    const initiator = viewer.id < activePeerId;

    const connectingTimer = window.setTimeout(() => {
      setStatus('connecting');
      resetVoiceCall();
    }, 0);

    function stopRemoteMedia() {
      for (const track of remoteStreamRef.current?.getTracks() ?? []) {
        track.onended = null;
        track.onmute = null;
        track.onunmute = null;
        track.stop();
      }
      remoteStreamRef.current = null;
      remoteAudio.srcObject = null;
      remoteAudio.pause();
      setRemoteStream(null);
    }

    function closePeer() {
      if (channel) {
        channel.onopen = null;
        channel.onclose = null;
        channel.onerror = null;
        channel.onmessage = null;
        channel.close();
      }
      if (peer) {
        peer.onconnectionstatechange = null;
        peer.ondatachannel = null;
        peer.ontrack = null;
        peer.close();
      }
      channel = null;
      peer = null;
      channelRef.current = null;
      peerRef.current = null;
      audioSenderRef.current = null;
      videoSenderRef.current = null;
      stopRemoteMedia();
      stopLocalMedia();
    }

    function sendVoicePacket(
      type: VoiceCallPacketType,
      callId: string,
      media: VoiceCallMedia,
    ) {
      if (channel?.readyState !== 'open') return false;
      channel.send(
        JSON.stringify(
          createVoiceCallPacket(type, activeConversationId, callId, media),
        ),
      );
      return true;
    }

    sendVoicePacketRef.current = sendVoicePacket;

    function receiveVoicePacket(value: unknown) {
      const packet = parseVoiceCallPacket(value, activeConversationId);
      if (!packet) return false;
      const currentCallId = activeCallIdRef.current;
      const currentStatus = callStatusRef.current;
      if (packet.type === 'call-offer') {
        if (currentStatus === 'outgoing') {
          if (viewer.id < activePeerId) {
            sendVoicePacket('call-decline', packet.callId, packet.media);
            return true;
          }
          if (currentCallId) {
            sendVoicePacket(
              'call-decline',
              currentCallId,
              activeCallModeRef.current,
            );
          }
        } else if (
          currentStatus !== 'idle' &&
          currentCallId !== packet.callId
        ) {
          sendVoicePacket('call-decline', packet.callId, packet.media);
          return true;
        }
        activeCallIdRef.current = packet.callId;
        activeCallModeRef.current = packet.media;
        setCallError('');
        updateCallStatus('incoming');
        const queuedAction = onIncomingCallRef.current(packet.callId);
        if (queuedAction === 'accept') {
          void acceptCall();
          return true;
        }
        if (queuedAction === 'decline') {
          sendVoicePacket('call-decline', packet.callId, packet.media);
          resetVoiceCall();
          return true;
        }
        clearCallTimer();
        callTimerRef.current = window.setTimeout(() => {
          sendVoicePacket('call-decline', packet.callId, packet.media);
          resetVoiceCall();
        }, CALL_RESPONSE_TIMEOUT_MS);
        return true;
      }
      if (packet.callId !== currentCallId) return true;
      if (packet.type === 'call-accept' && currentStatus === 'outgoing') {
        clearCallTimer();
        updateCallStatus('connecting');
        void enableLocalMedia(packet.media)
          .then((warning) => {
            if (activeCallIdRef.current === packet.callId) {
              setCallError(warning);
              updateCallStatus('connected');
            }
          })
          .catch((error: unknown) => {
            sendVoicePacket('call-end', packet.callId, packet.media);
            resetVoiceCall(
              error instanceof Error
                ? error.message
                : 'Aucun périphérique multimédia n’est accessible.',
            );
          });
      } else if (packet.type === 'call-decline') {
        resetVoiceCall('Appel refusé.');
      } else if (packet.type === 'call-end') {
        resetVoiceCall();
      }
      return true;
    }

    function attachChannel(nextChannel: RTCDataChannel) {
      channel = nextChannel;
      nextChannel.onopen = () => {
        if (!active) return;
        channelRef.current = nextChannel;
        setStatus('direct');
        const pendingCallId = activeCallIdRef.current;
        if (callStatusRef.current === 'outgoing' && pendingCallId) {
          sendVoicePacket(
            'call-offer',
            pendingCallId,
            activeCallModeRef.current,
          );
        }
      };
      nextChannel.onclose = () => {
        if (!active) return;
        channelRef.current = null;
        if (activeCallIdRef.current) resetVoiceCall('Appel interrompu.');
        setStatus('github');
        scheduleTick(0);
      };
      nextChannel.onerror = () => {
        if (active && nextChannel.readyState !== 'open') {
          if (activeCallIdRef.current) resetVoiceCall('Appel interrompu.');
          setStatus('github');
          scheduleTick(0);
        }
      };
      nextChannel.onmessage = (event) => {
        if (
          !active ||
          typeof event.data !== 'string' ||
          event.data.length > 512
        )
          return;
        try {
          const packet: unknown = JSON.parse(event.data);
          if (isWakePacket(packet, activeConversationId)) {
            onMessageSignalRef.current();
          } else {
            receiveVoicePacket(packet);
          }
        } catch {}
      };
    }

    function attachRemoteTrack(event: RTCTrackEvent) {
      if (!active || !['audio', 'video'].includes(event.track.kind)) return;
      const current = remoteStreamRef.current ?? new MediaStream();
      if (!current.getTracks().some((track) => track.id === event.track.id)) {
        current.addTrack(event.track);
      }
      remoteStreamRef.current = current;
      const refreshRemoteStream = () => {
        if (!active || !remoteStreamRef.current) return;
        const snapshot = new MediaStream(remoteStreamRef.current.getTracks());
        setRemoteStream(snapshot);
        remoteAudio.srcObject = snapshot;
      };
      event.track.onmute = refreshRemoteStream;
      event.track.onunmute = refreshRemoteStream;
      event.track.onended = () => {
        remoteStreamRef.current?.removeTrack(event.track);
        refreshRemoteStream();
      };
      refreshRemoteStream();
      void selectAudioOutput(
        remoteAudio,
        mediaSettingsRef.current.audioOutputId,
      )
        .then(() => remoteAudio.play())
        .catch(() => {});
    }

    function assignRemoteTransceivers(currentPeer: RTCPeerConnection) {
      audioSenderRef.current =
        currentPeer
          .getTransceivers()
          .find((value) => value.receiver.track.kind === 'audio')?.sender ??
        null;
      videoSenderRef.current =
        currentPeer
          .getTransceivers()
          .find((value) => value.receiver.track.kind === 'video')?.sender ??
        null;
    }

    function createPeer() {
      closePeer();
      const nextPeer = new RTCPeerConnection({
        iceServers: [{ urls: ['stun:stun.cloudflare.com:3478'] }],
        iceCandidatePoolSize: 2,
      });
      nextPeer.ontrack = attachRemoteTrack;
      nextPeer.onconnectionstatechange = () => {
        if (!active) return;
        if (nextPeer.connectionState === 'connected') setStatus('direct');
        if (['failed', 'closed'].includes(nextPeer.connectionState)) {
          channelRef.current = null;
          if (activeCallIdRef.current) resetVoiceCall('Appel interrompu.');
          setStatus('github');
          scheduleTick(0);
        }
      };
      if (initiator) {
        audioSenderRef.current = nextPeer.addTransceiver('audio', {
          direction: 'sendrecv',
        }).sender;
        videoSenderRef.current = nextPeer.addTransceiver('video', {
          direction: 'sendrecv',
        }).sender;
      } else {
        nextPeer.ondatachannel = (event) => attachChannel(event.channel);
      }
      peer = nextPeer;
      peerRef.current = nextPeer;
      return nextPeer;
    }

    async function publish(
      kind: RealtimeSignal['kind'],
      sessionId: string,
      currentPeer: RTCPeerConnection,
    ) {
      await waitForIceGathering(currentPeer);
      if (!active || !currentPeer.localDescription?.sdp) return;
      const createdAt = new Date().toISOString();
      await desktopApi.noosphere.publishRealtimeSignal(activeConversationId, {
        version: 1,
        kind,
        sessionId,
        createdAt,
        expiresAt: new Date(Date.now() + SIGNAL_LIFETIME_MS).toISOString(),
        sdp: currentPeer.localDescription.sdp,
      });
    }

    async function makeOffer() {
      const currentPeer = createPeer();
      const sessionId = createSessionId();
      offerSessionId = sessionId;
      offerPublishedAt = Date.now();
      attachChannel(
        currentPeer.createDataChannel('noosphere', { ordered: true }),
      );
      await currentPeer.setLocalDescription(await currentPeer.createOffer());
      await publish('offer', sessionId, currentPeer);
    }

    async function answerOffer(signal: RealtimeSignal) {
      handledOfferId = signal.sessionId;
      const currentPeer = createPeer();
      await currentPeer.setRemoteDescription({
        type: 'offer',
        sdp: signal.sdp,
      });
      assignRemoteTransceivers(currentPeer);
      await currentPeer.setLocalDescription(await currentPeer.createAnswer());
      await publish('answer', signal.sessionId, currentPeer);
    }

    async function tick() {
      if (!active || polling || directChannelOpen()) return;
      polling = true;
      try {
        if (
          initiator &&
          (!peer ||
            ['failed', 'closed'].includes(peer.connectionState) ||
            !offerSessionId ||
            Date.now() - offerPublishedAt >= OFFER_RENEWAL_MS)
        ) {
          await makeOffer();
        }
        const signal =
          await desktopApi.noosphere.readRealtimeSignal(activeConversationId);
        consecutiveFailures = 0;
        if (!active || !signal) return;
        if (
          initiator &&
          signal.kind === 'answer' &&
          signal.sessionId === offerSessionId &&
          peer &&
          !peer.remoteDescription
        ) {
          await peer.setRemoteDescription({ type: 'answer', sdp: signal.sdp });
        } else if (
          !initiator &&
          signal.kind === 'offer' &&
          signal.sessionId !== handledOfferId
        ) {
          await answerOffer(signal);
        }
      } catch {
        if (active) {
          consecutiveFailures += 1;
          setStatus('github');
        }
      } finally {
        polling = false;
        if (active && !directChannelOpen()) {
          scheduleTick(
            Math.min(
              SIGNAL_POLL_INTERVAL_MS * 2 ** consecutiveFailures,
              SIGNAL_POLL_MAX_INTERVAL_MS,
            ),
          );
        }
      }
    }

    function directChannelOpen() {
      return channelRef.current?.readyState === 'open';
    }

    function scheduleTick(delay: number) {
      if (!active) return;
      if (pollTimer !== null) window.clearTimeout(pollTimer);
      const jitter =
        delay === 0 ? 0 : Math.floor(delay * (0.85 + Math.random() * 0.3));
      pollTimer = window.setTimeout(() => void tick(), jitter);
    }

    scheduleTick(0);
    const fallbackTimer = window.setTimeout(() => {
      if (active && channelRef.current?.readyState !== 'open') {
        setStatus('github');
      }
    }, DIRECT_CONNECTION_GRACE_MS);

    return () => {
      active = false;
      window.clearTimeout(connectingTimer);
      window.clearTimeout(fallbackTimer);
      if (pollTimer !== null) window.clearTimeout(pollTimer);
      sendVoicePacketRef.current = () => false;
      clearCallTimer();
      closePeer();
      remoteAudioRef.current = null;
    };
  }, [
    clearCallTimer,
    acceptCall,
    conversationId,
    enableLocalMedia,
    peerId,
    resetVoiceCall,
    stopLocalMedia,
    updateCallStatus,
    viewer.id,
  ]);

  const startCall = useCallback(
    (media: VoiceCallMedia) => {
      if (!conversationId || callStatusRef.current !== 'idle') return null;
      const callId = createVoiceCallId();
      activeCallIdRef.current = callId;
      activeCallModeRef.current = media;
      setCallError('');
      updateCallStatus('outgoing');
      sendVoicePacketRef.current('call-offer', callId, media);
      clearCallTimer();
      callTimerRef.current = window.setTimeout(() => {
        sendVoicePacketRef.current('call-end', callId, media);
        resetVoiceCall('Pas de réponse.');
      }, CALL_RESPONSE_TIMEOUT_MS);
      return callId;
    },
    [clearCallTimer, conversationId, resetVoiceCall, updateCallStatus],
  );

  const declineCall = useCallback(() => {
    const callId = activeCallIdRef.current;
    if (!callId) return;
    sendVoicePacketRef.current(
      'call-decline',
      callId,
      activeCallModeRef.current,
    );
    resetVoiceCall();
  }, [resetVoiceCall]);

  const endCall = useCallback(() => {
    const callId = activeCallIdRef.current;
    if (callId) {
      sendVoicePacketRef.current('call-end', callId, activeCallModeRef.current);
    }
    resetVoiceCall();
  }, [resetVoiceCall]);

  const toggleMicrophone = useCallback(() => {
    const track = localStreamRef.current?.getAudioTracks()[0];
    if (!track) return;
    track.enabled = !track.enabled;
    setMicrophoneEnabled(track.enabled);
  }, []);

  const toggleCamera = useCallback(async () => {
    if (callStatusRef.current !== 'connected' || !videoSenderRef.current)
      return;
    const current = localStreamRef.current;
    const existing = current?.getVideoTracks()[0];
    if (existing) {
      await videoSenderRef.current.replaceTrack(null);
      current?.removeTrack(existing);
      existing.stop();
      const next = new MediaStream(current?.getTracks() ?? []);
      localStreamRef.current = next;
      setLocalStream(next);
      setCameraEnabled(false);
      return;
    }
    setCallError('');
    try {
      const capture = await captureCamera(mediaSettingsRef.current);
      const track = capture.stream.getVideoTracks()[0];
      if (!track) throw new Error('Caméra indisponible.');
      await videoSenderRef.current.replaceTrack(track);
      const next = new MediaStream([
        ...(localStreamRef.current?.getTracks() ?? []),
        track,
      ]);
      localStreamRef.current = next;
      setLocalStream(next);
      setCameraEnabled(true);
      setCallError(capture.warning);
    } catch (error) {
      setCallError(
        error instanceof Error ? error.message : 'Caméra indisponible.',
      );
    }
  }, []);

  const sendMessageSignal = useCallback(
    (nextConversationId: string, messageId: string) => {
      if (nextConversationId !== conversationId) return false;
      const channel = channelRef.current;
      if (channel?.readyState !== 'open') return false;
      channel.send(
        JSON.stringify({
          version: 1,
          type: 'message',
          conversationId: nextConversationId,
          id: messageId,
        }),
      );
      return true;
    },
    [conversationId],
  );

  return {
    status,
    callStatus,
    callError,
    localStream,
    remoteStream,
    microphoneEnabled,
    cameraEnabled,
    startCall,
    acceptCall,
    declineCall,
    endCall,
    toggleMicrophone,
    toggleCamera,
    sendMessageSignal,
  };
}
