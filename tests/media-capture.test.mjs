import assert from 'node:assert/strict';
import test from 'node:test';

import { captureCallMedia, captureCamera } from '../lib/media-capture.ts';
import { DEFAULT_MEDIA_DEVICE_SETTINGS } from '../lib/voice-call.ts';

class FakeTrack {
  constructor(kind) {
    this.kind = kind;
    this.stopped = false;
  }

  stop() {
    this.stopped = true;
  }
}

class FakeStream {
  constructor(tracks = []) {
    this.tracks = tracks;
  }

  getTracks() {
    return this.tracks;
  }

  getAudioTracks() {
    return this.tracks.filter((track) => track.kind === 'audio');
  }

  getVideoTracks() {
    return this.tracks.filter((track) => track.kind === 'video');
  }
}

const createStream = (tracks) => new FakeStream(tracks);

void test('un appel vidéo continue avec le micro si la caméra échoue', async () => {
  const result = await captureCallMedia(
    'video',
    DEFAULT_MEDIA_DEVICE_SETTINGS,
    async ({ audio }) => {
      if (audio) return new FakeStream([new FakeTrack('audio')]);
      throw new Error('camera unavailable');
    },
    createStream,
  );

  assert.equal(result.microphone, true);
  assert.equal(result.camera, false);
  assert.match(result.warning, /continue avec le microphone/);
});

void test('un appel vidéo continue avec la caméra si le micro échoue', async () => {
  const result = await captureCallMedia(
    'video',
    DEFAULT_MEDIA_DEVICE_SETTINGS,
    async ({ video }) => {
      if (video) return new FakeStream([new FakeTrack('video')]);
      throw new Error('microphone unavailable');
    },
    createStream,
  );

  assert.equal(result.microphone, false);
  assert.equal(result.camera, true);
  assert.match(result.warning, /continue avec la caméra/);
});

void test('un identifiant de micro périmé retombe sur le périphérique par défaut', async () => {
  const requests = [];
  const result = await captureCallMedia(
    'audio',
    { ...DEFAULT_MEDIA_DEVICE_SETTINGS, audioInputId: 'ancien-micro' },
    async (constraints) => {
      requests.push(constraints);
      if (constraints.audio.deviceId) throw new Error('device not found');
      return new FakeStream([new FakeTrack('audio')]);
    },
    createStream,
  );

  assert.equal(requests.length, 2);
  assert.equal(result.microphone, true);
  assert.match(result.warning, /périphérique par défaut/);
});

void test('un appel échoue seulement quand aucun média demandé n’est accessible', async () => {
  await assert.rejects(
    captureCallMedia(
      'video',
      DEFAULT_MEDIA_DEVICE_SETTINGS,
      async () => {
        throw new Error('unavailable');
      },
      createStream,
    ),
    /Aucun microphone ni aucune caméra.*autorisations système/,
  );
});

void test('la réduction du bruit peut être désactivée', async () => {
  let requestedAudio;
  await captureCallMedia(
    'audio',
    { ...DEFAULT_MEDIA_DEVICE_SETTINGS, noiseSuppression: 'off' },
    async ({ audio }) => {
      requestedAudio = audio;
      return new FakeStream([new FakeTrack('audio')]);
    },
    createStream,
  );
  assert.equal(requestedAudio.noiseSuppression, false);
});

void test('une caméra peut être ajoutée après le début de l’appel', async () => {
  const result = await captureCamera(
    DEFAULT_MEDIA_DEVICE_SETTINGS,
    async ({ audio, video }) => {
      assert.equal(audio, false);
      assert.ok(video);
      return new FakeStream([new FakeTrack('video')]);
    },
    createStream,
  );
  assert.equal(result.stream.getVideoTracks().length, 1);
});
