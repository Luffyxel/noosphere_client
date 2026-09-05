import assert from 'node:assert/strict';
import test from 'node:test';

import {
  createVoiceCallPacket,
  normalizeMediaDeviceSettings,
  parseVoiceCallPacket,
} from '../lib/voice-call.ts';

const conversationId = 'dm-0123456789abcdef0123456789abcdef';
const callId = 'call-0123456789abcdef0123456789abcdef';

void test('les quatre événements vocaux autorisés sont acceptés', () => {
  for (const type of [
    'call-offer',
    'call-accept',
    'call-decline',
    'call-end',
  ]) {
    const packet = createVoiceCallPacket(type, conversationId, callId, 'audio');
    assert.deepEqual(parseVoiceCallPacket(packet, conversationId), packet);
  }
});

void test('un événement vocal injecté ou destiné à une autre discussion est refusé', () => {
  assert.equal(
    parseVoiceCallPacket(
      {
        ...createVoiceCallPacket('call-offer', conversationId, callId, 'video'),
        admin: true,
      },
      conversationId,
    ),
    null,
  );
  assert.equal(
    parseVoiceCallPacket(
      createVoiceCallPacket(
        'call-offer',
        'dm-ffffffffffffffffffffffffffffffff',
        callId,
        'audio',
      ),
      conversationId,
    ),
    null,
  );
  assert.equal(
    parseVoiceCallPacket(
      createVoiceCallPacket(
        'call-offer',
        conversationId,
        'call-invalid',
        'video',
      ),
      conversationId,
    ),
    null,
  );
});

void test('la configuration média locale est bornée et normalisée', () => {
  assert.deepEqual(
    normalizeMediaDeviceSettings({
      audioInputId: 'microphone-1',
      audioOutputId: 'casque-1',
      videoInputId: 'camera-1',
      noiseSuppression: 'off',
      echoCancellation: true,
      autoGainControl: false,
      ignored: 'value',
    }),
    {
      audioInputId: 'microphone-1',
      audioOutputId: 'casque-1',
      videoInputId: 'camera-1',
      noiseSuppression: 'off',
      echoCancellation: true,
      autoGainControl: false,
    },
  );
  assert.equal(
    normalizeMediaDeviceSettings({ audioInputId: 'x'.repeat(513) })
      .audioInputId,
    '',
  );
  assert.equal(
    normalizeMediaDeviceSettings({ noiseSuppression: 'strong' })
      .noiseSuppression,
    'strong',
  );
});
