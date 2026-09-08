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

async function exercisePeerConnection(source: MediaStream): Promise<{
  dataChannel: boolean;
  media: boolean;
}> {
  const caller = new RTCPeerConnection({ iceServers: [] });
  const receiver = new RTCPeerConnection({ iceServers: [] });
  const audioTrack = source.getAudioTracks()[0];
  const videoTrack = source.getVideoTracks()[0];
  if (!audioTrack || !videoTrack) throw new Error('Captured media missing');
  caller.addTrack(audioTrack, source);
  caller.addTrack(videoTrack, source);
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
    const [dataChannel, media] = await Promise.all([received, mediaReceived]);
    return {
      dataChannel,
      media,
    };
  } finally {
    for (const track of source.getTracks()) track.stop();
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

  const animation = new Image();
  animation.src = '/brand/loading.webp';
  try {
    await animation.decode();
    return animation.naturalWidth === 480 && animation.naturalHeight === 270;
  } finally {
    animation.removeAttribute('src');
  }
}

export async function runTauriSmokeTest(): Promise<void> {
  const config = await invoke<SmokeConfig>('system_smoke_config');
  if (!config.enabled || config.instanceProfileSlot === null) return;
  let webRtcDataChannel = false;
  let webRtcMedia = false;
  let mediaPermission = false;
  let brandAssets = false;
  const diagnostics: string[] = [];
  try {
    brandAssets = await verifyBrandAssets();
  } catch (error) {
    diagnostics.push(`brand: ${String(error)}`);
  }
  let captured: MediaStream | null = null;
  try {
    captured = await navigator.mediaDevices.getUserMedia({
      audio: true,
      video: true,
    });
    mediaPermission =
      captured.getAudioTracks().length > 0 &&
      captured.getVideoTracks().length > 0;
  } catch (error) {
    diagnostics.push(`capture: ${String(error)}`);
  }
  if (captured) {
    try {
      const result = await exercisePeerConnection(captured);
      webRtcDataChannel = result.dataChannel;
      webRtcMedia = result.media;
    } catch (error) {
      diagnostics.push(`webrtc: ${String(error)}`);
      for (const track of captured.getTracks()) track.stop();
    }
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
      diagnostics,
      instanceProfileSlot: config.instanceProfileSlot,
    },
  });
}
