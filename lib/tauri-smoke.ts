import { invoke } from '@tauri-apps/api/core';

type SmokeConfig = {
  enabled: boolean;
  instanceProfileSlot: number | null;
};

function waitForIceGathering(peer: RTCPeerConnection): Promise<void> {
  if (peer.iceGatheringState === 'complete') return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      peer.removeEventListener('icegatheringstatechange', changed);
      reject(new Error('ICE gathering timeout'));
    }, 10_000);
    function changed() {
      if (peer.iceGatheringState !== 'complete') return;
      window.clearTimeout(timeout);
      peer.removeEventListener('icegatheringstatechange', changed);
      resolve();
    }
    peer.addEventListener('icegatheringstatechange', changed);
  });
}

async function exercisePeerConnection(): Promise<{
  dataChannel: boolean;
  media: boolean;
}> {
  const caller = new RTCPeerConnection({ iceServers: [] });
  const receiver = new RTCPeerConnection({ iceServers: [] });
  const audioContext = new AudioContext();
  const oscillator = audioContext.createOscillator();
  const audioDestination = audioContext.createMediaStreamDestination();
  oscillator.connect(audioDestination);
  oscillator.start();
  const canvas = document.createElement('canvas');
  canvas.width = 16;
  canvas.height = 16;
  const canvasContext = canvas.getContext('2d');
  if (!canvasContext) throw new Error('Synthetic canvas unavailable');
  let frame = 0;
  const paintTimer = window.setInterval(() => {
    canvasContext.fillStyle = frame % 2 === 0 ? '#798f80' : '#263029';
    canvasContext.fillRect(0, 0, 16, 16);
    frame += 1;
  }, 100);
  canvasContext.fillRect(0, 0, 16, 16);
  const videoStream = canvas.captureStream(5);
  const audioTrack = audioDestination.stream.getAudioTracks()[0];
  const videoTrack = videoStream.getVideoTracks()[0];
  if (!audioTrack || !videoTrack) throw new Error('Synthetic media missing');
  const sourceTracks = [audioTrack, videoTrack];
  const audioSender = caller.addTransceiver('audio', {
    direction: 'sendrecv',
  }).sender;
  const videoSender = caller.addTransceiver('video', {
    direction: 'sendrecv',
  }).sender;
  await audioSender.replaceTrack(audioTrack);
  try {
    const received = new Promise<boolean>((resolve, reject) => {
      const timeout = window.setTimeout(
        () => reject(new Error('WebRTC Data Channel timeout')),
        15_000,
      );
      receiver.ondatachannel = ({ channel }) => {
        channel.onmessage = ({ data }) => {
          window.clearTimeout(timeout);
          resolve(data === 'noosphere-webrtc-smoke');
        };
      };
    });
    const channel = caller.createDataChannel('noosphere-smoke', {
      ordered: true,
    });
    const receivedKinds = new Set<string>();
    const mediaReceived = new Promise<boolean>((resolve, reject) => {
      const timeout = window.setTimeout(
        () => reject(new Error('WebRTC media timeout')),
        15_000,
      );
      receiver.ontrack = ({ track }) => {
        const markLive = () => {
          if (track.muted || track.readyState !== 'live') return;
          receivedKinds.add(track.kind);
          if (receivedKinds.has('audio') && receivedKinds.has('video')) {
            window.clearTimeout(timeout);
            resolve(true);
          }
        };
        track.onunmute = markLive;
        markLive();
      };
    });
    await caller.setLocalDescription(await caller.createOffer());
    await waitForIceGathering(caller);
    const callerDescription = caller.localDescription;
    if (!callerDescription) throw new Error('WebRTC offer missing');
    await receiver.setRemoteDescription(callerDescription);
    await receiver.setLocalDescription(await receiver.createAnswer());
    await waitForIceGathering(receiver);
    const receiverDescription = receiver.localDescription;
    if (!receiverDescription) throw new Error('WebRTC answer missing');
    await caller.setRemoteDescription(receiverDescription);
    await new Promise<void>((resolve, reject) => {
      const timeout = window.setTimeout(
        () => reject(new Error('WebRTC channel open timeout')),
        15_000,
      );
      channel.onopen = () => {
        window.clearTimeout(timeout);
        resolve();
      };
      channel.onerror = () => {
        window.clearTimeout(timeout);
        reject(new Error('WebRTC channel error'));
      };
    });
    channel.send('noosphere-webrtc-smoke');
    await videoSender.replaceTrack(videoTrack);
    const [dataChannel, media] = await Promise.all([received, mediaReceived]);
    return {
      dataChannel,
      media,
    };
  } finally {
    window.clearInterval(paintTimer);
    oscillator.stop();
    for (const track of sourceTracks) track.stop();
    await audioContext.close();
    caller.close();
    receiver.close();
  }
}

async function verifyBrandAssets(): Promise<boolean> {
  await Promise.all(
    ['/brand/logo-white.png', '/brand/logo-black.png'].map(
      (source) =>
        new Promise<void>((resolve, reject) => {
          const image = new Image();
          image.onload = () => resolve();
          image.onerror = () => reject(new Error('Brand image failed to load'));
          image.src = source;
        }),
    ),
  );

  const video = document.createElement('video');
  video.muted = true;
  video.playsInline = true;
  video.src = '/brand/loading.mp4';
  try {
    await new Promise<void>((resolve, reject) => {
      const timeout = window.setTimeout(
        () => reject(new Error('Brand animation timeout')),
        10_000,
      );
      video.onloadeddata = () => {
        window.clearTimeout(timeout);
        resolve();
      };
      video.onerror = () => {
        window.clearTimeout(timeout);
        reject(new Error('Brand animation failed to decode'));
      };
      video.load();
    });
    await video.play();
    return video.videoWidth === 1920 && video.videoHeight === 1080;
  } finally {
    video.pause();
    video.removeAttribute('src');
    video.load();
  }
}

export async function runTauriSmokeTest(): Promise<void> {
  const config = await invoke<SmokeConfig>('system_smoke_config');
  if (!config.enabled || config.instanceProfileSlot === null) return;
  let webRtcDataChannel = false;
  let webRtcMedia = false;
  let mediaPermission = false;
  let brandAssets = false;
  try {
    brandAssets = await verifyBrandAssets();
  } catch {
    // The native smoke runner records the failed capability and exits cleanly.
  }
  try {
    const captured = await navigator.mediaDevices.getUserMedia({
      audio: true,
      video: true,
    });
    mediaPermission =
      captured.getAudioTracks().length > 0 &&
      captured.getVideoTracks().length > 0;
    for (const track of captured.getTracks()) track.stop();
    const result = await exercisePeerConnection();
    webRtcDataChannel = result.dataChannel;
    webRtcMedia = result.media;
  } catch {
    // The native smoke runner records the failed capability and exits cleanly.
  }
  await invoke('system_smoke_complete', {
    result: {
      version: 1,
      nativeBridge: window.noosphereDesktop?.isDesktop === true,
      socialBridge:
        typeof window.noosphereDesktop?.noosphere.syncState === 'function' &&
        typeof window.noosphereDesktop?.noosphere.pollWakeSignals ===
          'function' &&
        typeof window.noosphereDesktop?.noosphere.sendMessage === 'function' &&
        typeof window.noosphereDesktop?.noosphere.signalWake === 'function' &&
        typeof window.noosphereDesktop?.noosphere.publishRealtimeSignal ===
          'function',
      webRtcDataChannel,
      webRtcMedia,
      mediaPermission,
      brandAssets,
      instanceProfileSlot: config.instanceProfileSlot,
    },
  });
}
