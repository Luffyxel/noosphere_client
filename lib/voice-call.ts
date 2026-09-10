export type VoiceCallPacketType =
  | 'call-offer'
  | 'call-accept'
  | 'call-decline'
  | 'call-end';

export type VoiceCallPacket = {
  version: 1;
  type: VoiceCallPacketType;
  conversationId: string;
  callId: string;
  media: VoiceCallMedia;
};

export type VoiceCallMedia = 'audio' | 'video';

export type NoiseSuppressionMode = 'off' | 'standard' | 'strong';

export type VoiceCallStatus =
  | 'idle'
  | 'outgoing'
  | 'incoming'
  | 'connecting'
  | 'connected';

export type PendingCallAction = 'accept' | 'decline' | null;

export function resolveVoiceCallStatus(
  status: VoiceCallStatus,
  hasIncomingSignal: boolean,
  pendingAction: PendingCallAction,
): VoiceCallStatus {
  if (!hasIncomingSignal || status !== 'idle') return status;
  if (pendingAction === 'accept') return 'connecting';
  if (pendingAction === 'decline') return 'idle';
  return 'incoming';
}

export type MediaDeviceSettings = {
  audioInputId: string;
  audioOutputId: string;
  videoInputId: string;
  noiseSuppression: NoiseSuppressionMode;
  echoCancellation: boolean;
  autoGainControl: boolean;
};

export const DEFAULT_MEDIA_DEVICE_SETTINGS: MediaDeviceSettings = {
  audioInputId: '',
  audioOutputId: '',
  videoInputId: '',
  noiseSuppression: 'standard',
  echoCancellation: true,
  autoGainControl: true,
};

export function normalizeMediaDeviceSettings(
  value: unknown,
): MediaDeviceSettings {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return { ...DEFAULT_MEDIA_DEVICE_SETTINGS };
  }
  const settings = value as Record<string, unknown>;
  const deviceId = (name: string) => {
    const candidate = settings[name];
    return typeof candidate === 'string' && candidate.length <= 512
      ? candidate
      : '';
  };
  const enabled = (name: string, fallback: boolean) =>
    typeof settings[name] === 'boolean'
      ? (settings[name] as boolean)
      : fallback;
  const noiseSuppression = (() => {
    const candidate = settings.noiseSuppression;
    if (
      candidate === 'off' ||
      candidate === 'standard' ||
      candidate === 'strong'
    ) {
      return candidate;
    }
    return candidate === false ? 'off' : 'standard';
  })();
  return {
    audioInputId: deviceId('audioInputId'),
    audioOutputId: deviceId('audioOutputId'),
    videoInputId: deviceId('videoInputId'),
    noiseSuppression,
    echoCancellation: enabled('echoCancellation', true),
    autoGainControl: enabled('autoGainControl', true),
  };
}

const PACKET_TYPES = new Set<VoiceCallPacketType>([
  'call-offer',
  'call-accept',
  'call-decline',
  'call-end',
]);

export function createVoiceCallId(): string {
  return `call-${crypto.randomUUID().replaceAll('-', '')}`;
}

export function createVoiceCallPacket(
  type: VoiceCallPacketType,
  conversationId: string,
  callId: string,
  media: VoiceCallMedia,
): VoiceCallPacket {
  return { version: 1, type, conversationId, callId, media };
}

export function parseVoiceCallPacket(
  value: unknown,
  conversationId: string,
): VoiceCallPacket | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const packet = value as Record<string, unknown>;
  if (
    Object.keys(packet).sort().join(',') !==
      'callId,conversationId,media,type,version' ||
    packet.version !== 1 ||
    typeof packet.type !== 'string' ||
    !PACKET_TYPES.has(packet.type as VoiceCallPacketType) ||
    packet.conversationId !== conversationId ||
    (packet.media !== 'audio' && packet.media !== 'video') ||
    typeof packet.callId !== 'string' ||
    !/^call-[a-f0-9]{32}$/.test(packet.callId)
  ) {
    return null;
  }
  return packet as VoiceCallPacket;
}
