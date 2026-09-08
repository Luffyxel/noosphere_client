import type { MediaDeviceSettings, VoiceCallMedia } from '@/lib/voice-call';

type GetUserMedia = (
  constraints: MediaStreamConstraints,
) => Promise<MediaStream>;

type CreateMediaStream = (tracks: MediaStreamTrack[]) => MediaStream;

export type CallMediaCapture = {
  stream: MediaStream;
  microphone: boolean;
  camera: boolean;
  warning: string;
};

type TrackCapture = {
  track: MediaStreamTrack | null;
  usedDefaultDevice: boolean;
};

function audioConstraints(
  settings: MediaDeviceSettings,
): MediaTrackConstraints {
  const constraints: MediaTrackConstraints & { voiceIsolation?: boolean } = {
    channelCount: 1,
    echoCancellation: settings.echoCancellation,
    noiseSuppression: settings.noiseSuppression !== 'off',
    autoGainControl: settings.autoGainControl,
    ...(settings.audioInputId
      ? { deviceId: { exact: settings.audioInputId } }
      : {}),
  };
  const supported =
    typeof navigator === 'undefined'
      ? {}
      : (navigator.mediaDevices?.getSupportedConstraints?.() ?? {});
  if (
    settings.noiseSuppression === 'strong' &&
    'voiceIsolation' in supported &&
    supported.voiceIsolation
  ) {
    constraints.voiceIsolation = true;
  }
  return constraints;
}

function videoConstraints(
  settings: MediaDeviceSettings,
): MediaTrackConstraints {
  return {
    width: { ideal: 1280 },
    height: { ideal: 720 },
    frameRate: { ideal: 30, max: 30 },
    ...(settings.videoInputId
      ? { deviceId: { exact: settings.videoInputId } }
      : {}),
  };
}

function withoutDeviceId(
  constraints: MediaTrackConstraints,
): MediaTrackConstraints {
  const { deviceId: _deviceId, ...fallback } = constraints;
  return fallback;
}

async function captureTrack(
  kind: 'audio' | 'video',
  constraints: MediaTrackConstraints,
  hasSelectedDevice: boolean,
  getUserMedia: GetUserMedia,
): Promise<TrackCapture> {
  const request = (next: MediaTrackConstraints) =>
    getUserMedia({
      audio: kind === 'audio' ? next : false,
      video: kind === 'video' ? next : false,
    });

  let stream: MediaStream;
  let usedDefaultDevice = false;
  try {
    stream = await request(constraints);
  } catch {
    if (!hasSelectedDevice) return { track: null, usedDefaultDevice: false };
    try {
      stream = await request(withoutDeviceId(constraints));
      usedDefaultDevice = true;
    } catch {
      return { track: null, usedDefaultDevice: false };
    }
  }

  const expectedTracks =
    kind === 'audio' ? stream.getAudioTracks() : stream.getVideoTracks();
  const track = expectedTracks[0] ?? null;
  for (const candidate of stream.getTracks()) {
    if (candidate !== track) candidate.stop();
  }
  return { track, usedDefaultDevice };
}

export async function captureCallMedia(
  media: VoiceCallMedia,
  settings: MediaDeviceSettings,
  getUserMedia: GetUserMedia = navigator.mediaDevices.getUserMedia.bind(
    navigator.mediaDevices,
  ),
  createStream: CreateMediaStream = (tracks) => new MediaStream(tracks),
): Promise<CallMediaCapture> {
  const microphoneConstraints = audioConstraints(settings);
  const cameraConstraints = videoConstraints(settings);

  const [audio, video] = await Promise.all([
    captureTrack(
      'audio',
      microphoneConstraints,
      Boolean(settings.audioInputId),
      getUserMedia,
    ),
    media === 'video'
      ? captureTrack(
          'video',
          cameraConstraints,
          Boolean(settings.videoInputId),
          getUserMedia,
        )
      : Promise.resolve<TrackCapture>({
          track: null,
          usedDefaultDevice: false,
        }),
  ]);

  if (!audio.track && !video.track) {
    throw new Error(
      media === 'video'
        ? 'Aucun microphone ni aucune caméra ne sont accessibles. Vérifie les périphériques sélectionnés et les autorisations système.'
        : 'Aucun microphone n’est accessible. Vérifie le périphérique sélectionné et les autorisations système.',
    );
  }

  const tracks = [audio.track, video.track].filter(
    (track): track is MediaStreamTrack => Boolean(track),
  );
  let warning = '';
  if (media === 'video' && !video.track) {
    warning = 'Caméra indisponible : l’appel continue avec le microphone.';
  } else if (media === 'video' && !audio.track) {
    warning = 'Microphone indisponible : l’appel continue avec la caméra.';
  } else if (audio.usedDefaultDevice || video.usedDefaultDevice) {
    warning =
      'Le périphérique sélectionné était indisponible : le périphérique par défaut est utilisé.';
  }

  return {
    stream: createStream(tracks),
    microphone: Boolean(audio.track),
    camera: Boolean(video.track),
    warning,
  };
}

export async function captureCamera(
  settings: MediaDeviceSettings,
  getUserMedia: GetUserMedia = navigator.mediaDevices.getUserMedia.bind(
    navigator.mediaDevices,
  ),
  createStream: CreateMediaStream = (tracks) => new MediaStream(tracks),
): Promise<{ stream: MediaStream; warning: string }> {
  const capture = await captureTrack(
    'video',
    videoConstraints(settings),
    Boolean(settings.videoInputId),
    getUserMedia,
  );
  if (!capture.track) {
    throw new Error(
      'Aucune caméra n’est accessible. Vérifie le périphérique sélectionné et les autorisations système.',
    );
  }
  return {
    stream: createStream([capture.track]),
    warning: capture.usedDefaultDevice
      ? 'La caméra sélectionnée était indisponible : la caméra par défaut est utilisée.'
      : '',
  };
}
