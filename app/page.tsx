'use client';

import {
  AlertCircle,
  Bell,
  Check,
  Clipboard,
  Clock3,
  GitBranch,
  LogOut,
  Mic,
  MicOff,
  MessageCircle,
  Phone,
  PhoneOff,
  Plus,
  Search,
  SendHorizontal,
  Settings2,
  Trash2,
  UserRound,
  UserPlus,
  Users,
  Video,
  VideoOff,
  Volume2,
  X,
} from 'lucide-react';
import {
  type SubmitEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';

import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar';
import { BrandLoader, BrandLogo } from '@/components/brand';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { ScrollArea } from '@/components/ui/scroll-area';
import type {
  Conversation,
  FriendRequest,
  GitHubSetupStatus,
  GitHubViewer,
  NoosphereUser,
  SocialState,
  UserLookup,
} from '@/lib/desktop-bridge';
import { captureCallMedia } from '@/lib/media-capture';
import { mergeMessages, type DisplayMessage } from '@/lib/message-list';
import { startSocialRefreshLoop } from '@/lib/social-refresh';
import { useDirectPeer } from '@/lib/use-direct-peer';
import { cn } from '@/lib/utils';
import {
  DEFAULT_MEDIA_DEVICE_SETTINGS,
  normalizeMediaDeviceSettings,
  type MediaDeviceSettings,
  type VoiceCallStatus,
} from '@/lib/voice-call';

type AppNotification = {
  id: string;
  title: string;
  detail: string;
};

const EMPTY_SOCIAL_STATE: SocialState = {
  conversations: [],
  incoming: [],
  outgoing: [],
};

function jitteredInterval(milliseconds: number) {
  return Math.floor(milliseconds * (0.85 + Math.random() * 0.3));
}

function cleanDeviceCode(code: string) {
  return code.replaceAll('-', '');
}

function LoadingScreen() {
  return (
    <main
      className="grid min-h-dvh place-items-center overflow-hidden bg-[#0f0f10]"
      aria-label="Démarrage de Noosphere"
    >
      <img
        className="aspect-video w-[min(80vw,480px)] object-contain"
        src="/brand/loading.webp"
        alt=""
      />
    </main>
  );
}

function readableError(error: unknown) {
  if (typeof error === 'string' && error.trim()) return error;
  if (!(error instanceof Error)) return 'Une erreur est survenue.';
  if (error.message.includes('fetch failed'))
    return 'Impossible de contacter GitHub.';
  const wrappedMessage = error.message.match(
    /Error invoking remote method '[^']+': (?:Error|TypeError): (.+)$/,
  );
  return wrappedMessage?.[1] ?? error.message;
}

function UserAvatar({
  login,
  name,
  avatarUrl,
  size = 'default',
}: {
  login: string;
  name?: string | null;
  avatarUrl: string;
  size?: 'default' | 'sm' | 'lg';
}) {
  return (
    <Avatar size={size}>
      <AvatarImage src={avatarUrl} alt="" />
      <AvatarFallback className="bg-[#3a3a3c] text-[#f5f5f7]">
        {(name || login).slice(0, 1).toUpperCase()}
      </AvatarFallback>
    </Avatar>
  );
}

function LoginScreen({
  onGitHub,
}: {
  onGitHub: (viewer: GitHubViewer) => Promise<void>;
}) {
  const [loading, setLoading] = useState(false);
  const [provisioningConsent, setProvisioningConsent] = useState(false);
  const [setupStatus, setSetupStatus] = useState<GitHubSetupStatus | null>(
    null,
  );
  const [deviceCode, setDeviceCode] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    const desktop = window.noosphereDesktop;
    if (!desktop) return;
    const removeDeviceListener = desktop.github.onDeviceCode((value) => {
      setDeviceCode(value?.userCode ?? null);
      setCopied(Boolean(value));
    });
    const removeStatusListener = desktop.github.onSetupStatus(setSetupStatus);
    return () => {
      removeDeviceListener();
      removeStatusListener();
    };
  }, []);

  async function copyCode() {
    if (!deviceCode) return;
    await navigator.clipboard.writeText(cleanDeviceCode(deviceCode));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1_600);
  }

  async function beginGitHubLogin(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!provisioningConsent) return;
    setLoading(true);
    setError('');
    setDeviceCode(null);
    const desktop = window.noosphereDesktop;
    if (!desktop?.isDesktop) {
      setError('Disponible uniquement dans l’application de bureau Noosphere.');
      setLoading(false);
      return;
    }

    try {
      await onGitHub(
        await desktop.github.connect({
          version: 1,
          createPublicRepository: true,
          restrictInstallationToRepository: true,
        }),
      );
    } catch (loginError) {
      setError(readableError(loginError));
    } finally {
      setLoading(false);
    }
  }

  const statusCopy =
    setupStatus?.stage === 'repository'
      ? {
          title: setupStatus.repositoryName
            ? `Création de ${setupStatus.repositoryName}`
            : 'Création du dépôt',
        }
      : setupStatus?.stage === 'installation'
        ? {
            title: 'Autorisation GitHub',
          }
        : setupStatus?.stage === 'initialization'
          ? {
              title: 'Préparation du profil',
            }
          : {
              title: deviceCode ? 'Validation GitHub' : 'Connexion à GitHub',
            };

  return (
    <main className="min-h-dvh bg-[#0f0f10] px-5 py-8 font-sans text-[#f5f5f7] sm:grid sm:place-items-center">
      <section className="mx-auto w-full max-w-[440px] overflow-hidden rounded-[28px] border border-white/[.09] bg-[#1c1c1e]/95 shadow-[0_28px_90px_rgba(0,0,0,.42)] backdrop-blur-2xl">
        <div className="border-b border-white/[.07] px-7 py-5">
          <div className="flex items-center gap-3">
            <div className="grid size-10 place-items-center rounded-[12px] bg-[#2c2c2e] text-white">
              <BrandLogo className="size-[27px]" />
            </div>
            <div>
              <h1 className="text-[17px] font-semibold tracking-[-.01em]">
                Noosphere
              </h1>
              <p className="text-[12px] text-[#8e8e93]">
                Messagerie privée sur GitHub
              </p>
            </div>
          </div>
        </div>

        <div className="px-7 pb-7 pt-8">
          <div className="mb-7">
            <h2 className="text-[28px] font-semibold tracking-[-.035em] text-white">
              Connexion
            </h2>
          </div>

          {error && (
            <div
              role="alert"
              className="mb-5 flex gap-3 rounded-2xl border border-[#ff453a]/20 bg-[#ff453a]/[.07] p-4"
            >
              <AlertCircle className="mt-0.5 size-[18px] shrink-0 text-[#ff6961]" />
              <div>
                <p className="text-[13px] font-medium text-[#ff9f9a]">
                  Connexion impossible
                </p>
                <p className="mt-1 text-[13px] leading-5 text-[#b9b9bd]">
                  {error}
                </p>
              </div>
            </div>
          )}

          {!loading && (
            <form
              className="mb-5"
              onSubmit={(event) => void beginGitHubLogin(event)}
            >
              <label
                htmlFor="github-provisioning-consent"
                className="flex cursor-pointer items-start gap-3 rounded-[14px] border border-white/[.08] bg-[#242426] p-3.5 text-[12px] leading-[18px] text-[#b9b9bd]"
              >
                <input
                  id="github-provisioning-consent"
                  type="checkbox"
                  required
                  checked={provisioningConsent}
                  onChange={(event) =>
                    setProvisioningConsent(event.target.checked)
                  }
                  className="peer sr-only"
                />
                <span className="mt-0.5 grid size-[18px] shrink-0 place-items-center rounded-[5px] border border-white/20 bg-[#1c1c1e] transition-colors peer-checked:border-[var(--noosphere-accent)] peer-checked:bg-[var(--noosphere-accent)] peer-focus-visible:ring-2 peer-focus-visible:ring-[color:var(--noosphere-accent)]/50 peer-checked:[&>svg]:opacity-100">
                  <Check
                    className="size-3.5 text-white opacity-0 transition-opacity"
                    strokeWidth={3}
                  />
                </span>
                <span>
                  J’accepte la création d’un dépôt public et l’installation de
                  Noosphere uniquement sur ce dépôt.
                </span>
              </label>
              <Button
                size="lg"
                className="mt-4 h-12 w-full rounded-[14px] bg-[var(--noosphere-accent)] text-[15px] font-medium text-[var(--noosphere-on-accent)] hover:bg-[var(--noosphere-accent-hover)]"
                disabled={!provisioningConsent}
                type="submit"
                id="github-connect"
              >
                <GitBranch />
                Continuer avec GitHub
              </Button>
            </form>
          )}

          {loading && (
            <div className="mb-5 rounded-[20px] border border-white/[.08] bg-[#242426] p-4">
              <div className="flex items-center justify-between gap-3">
                <p className="text-[13px] font-medium text-white">
                  {statusCopy.title}
                </p>
                <BrandLoader className="size-4" />
              </div>
            </div>
          )}

          {deviceCode && (
            <div className="mb-5 rounded-[20px] border border-white/[.08] bg-[#242426] p-4">
              <div className="flex items-center justify-between gap-3">
                <p className="text-[13px] font-medium text-[#f5f5f7]">
                  Code GitHub
                </p>
                {copied && (
                  <div className="flex items-center gap-1 text-[11px] text-[var(--noosphere-accent)]">
                    <Check className="size-3.5" />
                    Copié
                  </div>
                )}
              </div>
              <p className="mt-1 text-[12px] text-[#8e8e93]">
                Code à saisir sur GitHub
              </p>
              <div
                className="mt-4 flex items-center gap-2"
                onCopy={(event) => {
                  event.preventDefault();
                  event.clipboardData.setData(
                    'text/plain',
                    cleanDeviceCode(deviceCode),
                  );
                  setCopied(true);
                }}
              >
                <code className="flex min-w-0 flex-1 items-center justify-center rounded-[14px] bg-[#121214] px-4 py-3.5 text-[22px] font-semibold tracking-[.16em] text-white selection:bg-[var(--noosphere-accent-strong)]">
                  <span>{deviceCode.slice(0, 4)}</span>
                  <span
                    aria-hidden="true"
                    className="pointer-events-none mx-1 select-none text-[#636366]"
                  >
                    -
                  </span>
                  <span>{deviceCode.slice(-4)}</span>
                </code>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="size-12 rounded-[14px] border border-white/[.08] bg-[#2c2c2e] text-[#d1d1d6] hover:bg-[#3a3a3c] hover:text-white"
                  aria-label="Copier le code"
                  onClick={() => void copyCode()}
                >
                  {copied ? (
                    <Check className="text-[var(--noosphere-accent)]" />
                  ) : (
                    <Clipboard />
                  )}
                </Button>
              </div>
            </div>
          )}
        </div>
      </section>
    </main>
  );
}

function AddFriendDialog({
  open,
  onOpenChange,
  onRequest,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onRequest: (request: FriendRequest) => void;
}) {
  const [query, setQuery] = useState('');
  const [lookup, setLookup] = useState<UserLookup | null>(null);
  const [loading, setLoading] = useState(false);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState('');

  async function searchUser(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!query.trim() || !window.noosphereDesktop) return;
    setLoading(true);
    setLookup(null);
    setError('');
    try {
      setLookup(await window.noosphereDesktop.noosphere.lookupUser(query));
    } catch (lookupError) {
      setError(readableError(lookupError));
    } finally {
      setLoading(false);
    }
  }

  async function addFriend(user: NoosphereUser) {
    if (!window.noosphereDesktop) return;
    setCreating(true);
    setError('');
    try {
      const request = await window.noosphereDesktop.noosphere.sendFriendRequest(
        user.login,
      );
      onRequest(request);
      onOpenChange(false);
      setQuery('');
      setLookup(null);
    } catch (requestError) {
      setError(readableError(requestError));
    } finally {
      setCreating(false);
    }
  }

  const registeredUser =
    lookup?.found && lookup.registered && lookup.user.repository
      ? ({
          ...lookup.user,
          repository: lookup.user.repository,
        } satisfies NoosphereUser)
      : null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="border-white/[.09] bg-[#202022] text-[#f5f5f7] sm:max-w-[440px]">
        <DialogHeader>
          <DialogTitle>Ajouter un ami</DialogTitle>
        </DialogHeader>

        <form
          className="flex gap-2"
          onSubmit={(event) => void searchUser(event)}
        >
          <Input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Pseudo GitHub"
            autoComplete="off"
            className="h-11 rounded-[12px] border-white/[.09] bg-[#151517] text-white placeholder:text-[#636366]"
          />
          <Button
            type="submit"
            size="icon"
            className="size-11 rounded-[12px] bg-[var(--noosphere-accent)] text-[var(--noosphere-on-accent)] hover:bg-[var(--noosphere-accent-hover)]"
            disabled={loading || !query.trim()}
          >
            {loading ? <BrandLoader className="size-5" /> : <Search />}
          </Button>
        </form>

        {error && <p className="text-[13px] text-[#ff6961]">{error}</p>}

        {lookup && !lookup.found && (
          <div className="rounded-[16px] bg-[#151517] p-4 text-[13px] text-[#8e8e93]">
            Compte GitHub introuvable.
          </div>
        )}

        {lookup?.found && (
          <div className="flex items-center gap-3 rounded-[16px] border border-white/[.07] bg-[#18181a] p-3">
            <UserAvatar
              login={lookup.user.login}
              name={lookup.user.name}
              avatarUrl={lookup.user.avatarUrl}
              size="lg"
            />
            <div className="min-w-0 flex-1">
              <p className="truncate text-[14px] font-medium">
                {lookup.user.name || lookup.user.login}
              </p>
              <p className="truncate text-[12px] text-[#8e8e93]">
                @{lookup.user.login}
              </p>
            </div>
            {registeredUser ? (
              <Button
                className="h-9 rounded-[10px] bg-[var(--noosphere-accent)] px-3 text-[12px] text-[var(--noosphere-on-accent)] hover:bg-[var(--noosphere-accent-hover)]"
                disabled={creating}
                onClick={() => void addFriend(registeredUser)}
              >
                {creating ? <BrandLoader className="size-4" /> : <UserPlus />}
                Ajouter
              </Button>
            ) : (
              <span className="rounded-full bg-white/[.06] px-2.5 py-1 text-[11px] text-[#8e8e93]">
                Pas sur Noosphere
              </span>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

function FriendProfileDialog({
  conversation,
  open,
  removing,
  onOpenChange,
  onMessage,
  onRemove,
}: {
  conversation: Conversation | null;
  open: boolean;
  removing: boolean;
  onOpenChange: (open: boolean) => void;
  onMessage: () => void;
  onRemove: () => Promise<void>;
}) {
  const [confirming, setConfirming] = useState(false);

  if (!conversation) return null;
  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) setConfirming(false);
        onOpenChange(nextOpen);
      }}
    >
      <DialogContent className="border-white/[.09] bg-[#202022] text-[#f5f5f7] sm:max-w-[400px]">
        <DialogHeader>
          <DialogTitle>Profil</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col items-center py-3 text-center">
          <UserAvatar
            login={conversation.peer.login}
            name={conversation.peer.name}
            avatarUrl={conversation.peer.avatarUrl}
            size="lg"
          />
          <p className="mt-4 text-[18px] font-semibold text-white">
            {conversation.peer.name || conversation.peer.login}
          </p>
          <p className="mt-1 text-[12px] text-[#8e8e93]">
            @{conversation.peer.login}
          </p>
        </div>
        <Button
          className="h-10 rounded-[11px] bg-[var(--noosphere-accent)] text-[var(--noosphere-on-accent)] hover:bg-[var(--noosphere-accent-hover)]"
          onClick={onMessage}
        >
          <MessageCircle />
          Envoyer un message
        </Button>
        {confirming ? (
          <div className="rounded-[13px] border border-[#ff453a]/25 bg-[#321f20] p-3">
            <p className="text-[12px] text-[#ffb3af]">
              Supprimer @{conversation.peer.login} de tes amis ?
            </p>
            <div className="mt-3 flex justify-end gap-2">
              <Button
                variant="ghost"
                className="h-8 text-[#d1d1d6] hover:bg-white/[.06] hover:text-white"
                disabled={removing}
                onClick={() => setConfirming(false)}
              >
                Annuler
              </Button>
              <Button
                className="h-8 bg-[#ff453a] text-white hover:bg-[#ff6961]"
                disabled={removing}
                onClick={() => void onRemove()}
              >
                {removing ? <BrandLoader className="size-4" /> : <Trash2 />}
                Confirmer
              </Button>
            </div>
          </div>
        ) : (
          <Button
            variant="ghost"
            className="h-10 rounded-[11px] text-[#ff6961] hover:bg-[#ff453a]/10 hover:text-[#ff8a84]"
            onClick={() => setConfirming(true)}
          >
            <Trash2 />
            Supprimer de mes amis
          </Button>
        )}
      </DialogContent>
    </Dialog>
  );
}

type MediaDeviceLists = {
  microphones: MediaDeviceInfo[];
  speakers: MediaDeviceInfo[];
  cameras: MediaDeviceInfo[];
};

const EMPTY_MEDIA_DEVICES: MediaDeviceLists = {
  microphones: [],
  speakers: [],
  cameras: [],
};

const MEDIA_SETTINGS_STORAGE_KEY = 'noosphere.media-settings.v1';

function StreamVideo({
  stream,
  className,
  label,
}: {
  stream: MediaStream | null;
  className: string;
  label: string;
}) {
  const videoRef = useRef<HTMLVideoElement>(null);
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.srcObject = stream;
    if (stream) void video.play().catch(() => {});
    return () => {
      if (video.srcObject === stream) video.srcObject = null;
    };
  }, [stream]);
  return (
    <video
      ref={videoRef}
      aria-label={label}
      autoPlay
      muted
      playsInline
      className={className}
    />
  );
}

function MicrophoneTest({ settings }: { settings: MediaDeviceSettings }) {
  const [status, setStatus] = useState<'idle' | 'starting' | 'active'>('idle');
  const [level, setLevel] = useState(0);
  const [message, setMessage] = useState('');
  const resources = useRef<{
    stream: MediaStream;
    context: AudioContext;
    frame: number;
  } | null>(null);

  const stop = useCallback((updateState = true) => {
    const current = resources.current;
    resources.current = null;
    if (current) {
      window.cancelAnimationFrame(current.frame);
      for (const track of current.stream.getTracks()) track.stop();
      void current.context.close();
    }
    if (updateState) {
      setStatus('idle');
      setLevel(0);
    }
  }, []);

  useEffect(() => () => stop(false), [stop]);

  useEffect(() => {
    if (resources.current) stop();
  }, [settings.audioInputId, settings.noiseSuppression, stop]);

  async function start() {
    if (resources.current) {
      stop();
      return;
    }
    setStatus('starting');
    setMessage('');
    try {
      const capture = await captureCallMedia('audio', settings);
      const context = new AudioContext();
      await context.resume();
      const source = context.createMediaStreamSource(capture.stream);
      const analyser = context.createAnalyser();
      analyser.fftSize = 512;
      analyser.smoothingTimeConstant = 0.72;
      source.connect(analyser);
      const samples = new Uint8Array(analyser.fftSize);
      const current = { stream: capture.stream, context, frame: 0 };
      resources.current = current;
      const updateMeter = () => {
        if (resources.current !== current) return;
        analyser.getByteTimeDomainData(samples);
        let sum = 0;
        for (const sample of samples) {
          const normalized = (sample - 128) / 128;
          sum += normalized * normalized;
        }
        const rms = Math.sqrt(sum / samples.length);
        setLevel(Math.min(100, Math.round(rms * 420)));
        current.frame = window.requestAnimationFrame(updateMeter);
      };
      updateMeter();
      setStatus('active');
      setMessage(capture.warning);
    } catch (error) {
      stop();
      setMessage(readableError(error));
    }
  }

  return (
    <div className="rounded-[12px] border border-white/[.07] bg-[#19191b] p-3">
      <div className="flex items-center justify-between gap-3">
        <p className="text-[12px] font-medium text-[#d1d1d6]">Test du micro</p>
        <Button
          type="button"
          size="sm"
          variant="secondary"
          className="bg-white/[.07] text-white hover:bg-white/[.1]"
          disabled={status === 'starting'}
          onClick={() => void start()}
        >
          {status === 'starting' ? (
            <BrandLoader className="size-4" />
          ) : status === 'active' ? (
            <MicOff />
          ) : (
            <Mic />
          )}
          {status === 'active' ? 'Arrêter' : 'Tester'}
        </Button>
      </div>
      <meter
        aria-label="Niveau du microphone"
        min={0}
        max={100}
        value={level}
        className="sr-only"
      />
      <div
        aria-hidden="true"
        className="mt-3 h-2 overflow-hidden rounded-full bg-white/[.07]"
      >
        <div
          className="h-full rounded-full bg-[var(--noosphere-accent)] transition-[width] duration-75"
          style={{ width: `${level}%` }}
        />
      </div>
      {message && <p className="mt-2 text-[11px] text-[#ff9f9a]">{message}</p>}
    </div>
  );
}

function MediaSettingsDialog({
  open,
  onOpenChange,
  settings,
  devices,
  loading,
  error,
  onChange,
  onRequestAccess,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  settings: MediaDeviceSettings;
  devices: MediaDeviceLists;
  loading: boolean;
  error: string;
  onChange: (patch: Partial<MediaDeviceSettings>) => void;
  onRequestAccess: () => void;
}) {
  const selectClass =
    'h-10 w-full rounded-[10px] border border-white/[.08] bg-[#1c1c1e] px-3 text-[12px] text-white outline-none focus:border-[var(--noosphere-accent)]';
  const deviceLabel = (device: MediaDeviceInfo, index: number, kind: string) =>
    device.label || `${kind} ${index + 1}`;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="border-white/[.09] bg-[#202022] text-[#f5f5f7] sm:max-w-[500px]">
        <DialogHeader>
          <DialogTitle>Audio et vidéo</DialogTitle>
        </DialogHeader>
        <div className="space-y-4">
          <label className="block space-y-1.5 text-[12px] text-[#a1a1a6]">
            <span className="flex items-center gap-2">
              <Mic className="size-3.5" /> Microphone
            </span>
            <select
              className={selectClass}
              value={settings.audioInputId}
              onChange={(event) =>
                onChange({ audioInputId: event.target.value })
              }
            >
              <option value="">Périphérique par défaut</option>
              {devices.microphones.map((device, index) => (
                <option key={device.deviceId} value={device.deviceId}>
                  {deviceLabel(device, index, 'Microphone')}
                </option>
              ))}
            </select>
          </label>
          <MicrophoneTest settings={settings} />
          <label className="block space-y-1.5 text-[12px] text-[#a1a1a6]">
            <span className="flex items-center gap-2">
              <Volume2 className="size-3.5" /> Casque ou haut-parleurs
            </span>
            <select
              className={selectClass}
              value={settings.audioOutputId}
              onChange={(event) =>
                onChange({ audioOutputId: event.target.value })
              }
            >
              <option value="">Périphérique par défaut</option>
              {devices.speakers.map((device, index) => (
                <option key={device.deviceId} value={device.deviceId}>
                  {deviceLabel(device, index, 'Sortie audio')}
                </option>
              ))}
            </select>
          </label>
          <label className="block space-y-1.5 text-[12px] text-[#a1a1a6]">
            <span className="flex items-center gap-2">
              <Video className="size-3.5" /> Caméra
            </span>
            <select
              className={selectClass}
              value={settings.videoInputId}
              onChange={(event) =>
                onChange({ videoInputId: event.target.value })
              }
            >
              <option value="">Périphérique par défaut</option>
              {devices.cameras.map((device, index) => (
                <option key={device.deviceId} value={device.deviceId}>
                  {deviceLabel(device, index, 'Caméra')}
                </option>
              ))}
            </select>
          </label>
          <label className="block space-y-1.5 text-[12px] text-[#a1a1a6]">
            <span className="flex items-center gap-2">
              <Volume2 className="size-3.5" /> Réduction du bruit
            </span>
            <select
              className={selectClass}
              value={settings.noiseSuppression}
              onChange={(event) =>
                onChange({
                  noiseSuppression: event.target
                    .value as MediaDeviceSettings['noiseSuppression'],
                })
              }
            >
              <option value="off">Désactivée</option>
              <option value="standard">Standard</option>
              <option value="strong">Renforcée</option>
            </select>
            <span className="block text-[11px] leading-4 text-[#737378]">
              Le mode renforcé utilise l’isolation vocale lorsqu’elle est prise
              en charge par le micro.
            </span>
          </label>
          <div className="grid gap-2 rounded-[12px] border border-white/[.07] bg-[#19191b] p-3 text-[12px] text-[#d1d1d6] sm:grid-cols-2">
            {[
              ['echoCancellation', 'Annulation de l’écho'],
              ['autoGainControl', 'Gain automatique'],
            ].map(([name, label]) => (
              <label key={name} className="flex items-center gap-2.5">
                <input
                  type="checkbox"
                  checked={settings[name as keyof MediaDeviceSettings] === true}
                  onChange={(event) =>
                    onChange({ [name]: event.target.checked })
                  }
                  className="size-4 accent-[var(--noosphere-accent)]"
                />
                {label}
              </label>
            ))}
          </div>
          {error && <p className="text-[11px] text-[#ff9f9a]">{error}</p>}
          <Button
            type="button"
            variant="secondary"
            className="w-full bg-white/[.07] text-white hover:bg-white/[.1]"
            disabled={loading}
            onClick={onRequestAccess}
          >
            {loading ? <BrandLoader className="size-5" /> : <Settings2 />}
            Détecter les périphériques
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function VoiceCallOverlay({
  status,
  peer,
  localStream,
  remoteStream,
  microphoneEnabled,
  cameraEnabled,
  onAccept,
  onDecline,
  onEnd,
  onToggleMicrophone,
  onToggleCamera,
}: {
  status: VoiceCallStatus;
  peer: NoosphereUser;
  localStream: MediaStream | null;
  remoteStream: MediaStream | null;
  microphoneEnabled: boolean;
  cameraEnabled: boolean;
  onAccept: () => void;
  onDecline: () => void;
  onEnd: () => void;
  onToggleMicrophone: () => void;
  onToggleCamera: () => void;
}) {
  if (status === 'idle') return null;
  const title =
    status === 'incoming'
      ? 'Appel entrant'
      : status === 'outgoing'
        ? 'Appel en cours…'
        : status === 'connecting'
          ? 'Connexion…'
          : 'Appel vocal';
  const localVideo = localStream
    ?.getVideoTracks()
    .some((track) => track.enabled && track.readyState === 'live');
  const remoteVideo = remoteStream
    ?.getVideoTracks()
    .some((track) => !track.muted && track.readyState === 'live');
  const videoCall =
    status === 'connected' && Boolean(localVideo || remoteVideo);
  return (
    <div
      className={cn(
        'absolute z-30 overflow-hidden border border-white/[.1] bg-[#202022]/98 shadow-2xl backdrop-blur-2xl',
        videoCall
          ? 'inset-x-5 top-[74px] mx-auto max-w-[760px] rounded-[20px]'
          : 'right-5 top-[70px] w-[310px] rounded-[18px] p-4',
      )}
    >
      {videoCall ? (
        <div className="relative aspect-video bg-black">
          {remoteVideo ? (
            <StreamVideo
              stream={remoteStream}
              label="Vidéo distante"
              className="size-full object-cover"
            />
          ) : (
            <div className="grid size-full place-items-center">
              <UserAvatar
                login={peer.login}
                name={peer.name}
                avatarUrl={peer.avatarUrl}
                size="lg"
              />
            </div>
          )}
          {localVideo ? (
            <StreamVideo
              stream={localStream}
              label="Aperçu de ma caméra"
              className="absolute bottom-4 right-4 aspect-video w-36 rounded-[12px] border border-white/15 bg-black object-cover shadow-xl"
            />
          ) : null}
          <div className="absolute inset-x-0 bottom-0 flex items-center justify-center gap-2 bg-gradient-to-t from-black/80 to-transparent pb-4 pt-12">
            <Button
              type="button"
              size="icon"
              className="rounded-full bg-white/15 text-white hover:bg-white/25"
              aria-label={
                microphoneEnabled ? 'Couper le micro' : 'Activer le micro'
              }
              onClick={onToggleMicrophone}
            >
              {microphoneEnabled ? <Mic /> : <MicOff />}
            </Button>
            <Button
              type="button"
              size="icon"
              className="rounded-full bg-white/15 text-white hover:bg-white/25"
              aria-label={
                cameraEnabled ? 'Couper la caméra' : 'Activer la caméra'
              }
              onClick={onToggleCamera}
            >
              {cameraEnabled ? <Video /> : <VideoOff />}
            </Button>
            <Button
              type="button"
              size="icon"
              className="rounded-full bg-[#ff453a] text-white hover:bg-[#ff6961]"
              aria-label="Raccrocher"
              onClick={onEnd}
            >
              <PhoneOff />
            </Button>
          </div>
        </div>
      ) : (
        <>
          <div className="flex items-center gap-3">
            <UserAvatar
              login={peer.login}
              name={peer.name}
              avatarUrl={peer.avatarUrl}
            />
            <div className="min-w-0 flex-1">
              <p className="truncate text-[13px] font-medium text-white">
                {peer.name || peer.login}
              </p>
              <p className="text-[11px] text-[#8e8e93]">{title}</p>
            </div>
          </div>
          <div className="mt-4 flex justify-center gap-2">
            {status === 'incoming' ? (
              <>
                <Button
                  type="button"
                  size="icon"
                  className="rounded-full bg-[var(--noosphere-accent)] text-[var(--noosphere-on-accent)] hover:bg-[var(--noosphere-accent-hover)]"
                  aria-label="Accepter l’appel"
                  onClick={onAccept}
                >
                  <Phone />
                </Button>
                <Button
                  type="button"
                  size="icon"
                  className="rounded-full bg-[#ff453a] text-white hover:bg-[#ff6961]"
                  aria-label="Refuser l’appel"
                  onClick={onDecline}
                >
                  <PhoneOff />
                </Button>
              </>
            ) : (
              <>
                {status === 'connected' && (
                  <>
                    <Button
                      type="button"
                      size="icon"
                      className="rounded-full bg-white/[.1] text-white hover:bg-white/[.15]"
                      aria-label={
                        microphoneEnabled
                          ? 'Couper le micro'
                          : 'Activer le micro'
                      }
                      onClick={onToggleMicrophone}
                    >
                      {microphoneEnabled ? <Mic /> : <MicOff />}
                    </Button>
                    <Button
                      type="button"
                      size="icon"
                      className="rounded-full bg-white/[.1] text-white hover:bg-white/[.15]"
                      aria-label="Activer la caméra"
                      onClick={onToggleCamera}
                    >
                      <VideoOff />
                    </Button>
                  </>
                )}
                <Button
                  type="button"
                  size="icon"
                  className="rounded-full bg-[#ff453a] text-white hover:bg-[#ff6961]"
                  aria-label="Raccrocher"
                  onClick={onEnd}
                >
                  <PhoneOff />
                </Button>
              </>
            )}
          </div>
        </>
      )}
    </div>
  );
}

function NoosphereApp({
  viewer,
  initialSocialState,
  onSignOut,
}: {
  viewer: GitHubViewer;
  initialSocialState: SocialState;
  onSignOut: () => void;
}) {
  const [conversations, setConversations] = useState<Conversation[]>(
    initialSocialState.conversations,
  );
  const [incomingRequests, setIncomingRequests] = useState<FriendRequest[]>(
    initialSocialState.incoming,
  );
  const [outgoingRequests, setOutgoingRequests] = useState<FriendRequest[]>(
    initialSocialState.outgoing,
  );
  const [messages, setMessages] = useState<DisplayMessage[]>([]);
  const [messageDraft, setMessageDraft] = useState('');
  const [sendingMessage, setSendingMessage] = useState(false);
  const [acceptingRequestId, setAcceptingRequestId] = useState<string | null>(
    null,
  );
  const [decliningRequestId, setDecliningRequestId] = useState<string | null>(
    null,
  );
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [friendProfileId, setFriendProfileId] = useState<string | null>(null);
  const [removingFriend, setRemovingFriend] = useState(false);
  const [addFriendOpen, setAddFriendOpen] = useState(false);
  const [notificationsOpen, setNotificationsOpen] = useState(false);
  const [mediaSettingsOpen, setMediaSettingsOpen] = useState(false);
  const [mediaSettings, setMediaSettings] = useState<MediaDeviceSettings>({
    ...DEFAULT_MEDIA_DEVICE_SETTINGS,
  });
  const [mediaDevices, setMediaDevices] =
    useState<MediaDeviceLists>(EMPTY_MEDIA_DEVICES);
  const [mediaDevicesLoading, setMediaDevicesLoading] = useState(false);
  const [mediaDevicesError, setMediaDevicesError] = useState('');
  const [loading, setLoading] = useState(false);
  const [syncError, setSyncError] = useState('');
  const [notifications, setNotifications] = useState<AppNotification[]>(() =>
    viewer.repository.created
      ? [
          {
            id: 'repository-created',
            title: 'Noosphere prêt',
            detail: viewer.repository.name,
          },
        ]
      : [],
  );
  const [unreadCount, setUnreadCount] = useState(
    viewer.repository.created ? 1 : 0,
  );
  const knownMessageIds = useRef(new Map<string, Set<string>>());
  const messageCache = useRef(new Map<string, DisplayMessage[]>());
  const socialSyncing = useRef(false);
  const wakeSyncing = useRef(false);
  const messagesSyncing = useRef(new Set<string>());
  const conversationsRef = useRef(conversations);
  const selectedIdRef = useRef(selectedId);
  const previousCallStatus = useRef<VoiceCallStatus>('idle');

  const updateMediaSettings = useCallback(
    (patch: Partial<MediaDeviceSettings>) => {
      const next = { ...mediaSettings, ...patch };
      setMediaSettings(next);
      try {
        window.localStorage.setItem(
          MEDIA_SETTINGS_STORAGE_KEY,
          JSON.stringify(next),
        );
      } catch {}
    },
    [mediaSettings],
  );

  const refreshMediaDevices = useCallback(async (requestAccess: boolean) => {
    if (!navigator.mediaDevices?.enumerateDevices) {
      setMediaDevicesError('Les périphériques multimédias sont indisponibles.');
      return;
    }
    setMediaDevicesLoading(true);
    setMediaDevicesError('');
    const previews: MediaStream[] = [];
    try {
      let accessWarning = '';
      if (requestAccess) {
        const [microphone, camera] = await Promise.allSettled([
          navigator.mediaDevices.getUserMedia({ audio: true, video: false }),
          navigator.mediaDevices.getUserMedia({ audio: false, video: true }),
        ]);
        if (microphone.status === 'fulfilled') previews.push(microphone.value);
        if (camera.status === 'fulfilled') previews.push(camera.value);
        if (microphone.status === 'rejected' && camera.status === 'rejected') {
          accessWarning =
            'Ni le microphone ni la caméra ne sont accessibles. Vérifie les périphériques et les autorisations système.';
        } else if (microphone.status === 'rejected') {
          accessWarning =
            'Microphone indisponible. La caméra reste utilisable pour les appels vidéo.';
        } else if (camera.status === 'rejected') {
          accessWarning =
            'Caméra indisponible. Le microphone reste utilisable pour les appels.';
        }
      }
      const available = await navigator.mediaDevices.enumerateDevices();
      setMediaDevices({
        microphones: available.filter((device) => device.kind === 'audioinput'),
        speakers: available.filter((device) => device.kind === 'audiooutput'),
        cameras: available.filter((device) => device.kind === 'videoinput'),
      });
      setMediaDevicesError(accessWarning);
    } catch {
      setMediaDevicesError(
        'Impossible de lire la liste des périphériques multimédias.',
      );
    } finally {
      for (const preview of previews) {
        for (const track of preview.getTracks()) track.stop();
      }
      setMediaDevicesLoading(false);
    }
  }, []);

  useEffect(() => {
    const initializeTimer = window.setTimeout(() => {
      try {
        const saved = window.localStorage.getItem(MEDIA_SETTINGS_STORAGE_KEY);
        if (saved)
          setMediaSettings(normalizeMediaDeviceSettings(JSON.parse(saved)));
      } catch {
        window.localStorage.removeItem(MEDIA_SETTINGS_STORAGE_KEY);
      }
      void refreshMediaDevices(false);
    }, 0);
    const devicesChanged = () => void refreshMediaDevices(false);
    navigator.mediaDevices?.addEventListener('devicechange', devicesChanged);
    return () => {
      window.clearTimeout(initializeTimer);
      navigator.mediaDevices?.removeEventListener(
        'devicechange',
        devicesChanged,
      );
    };
  }, [refreshMediaDevices]);

  useEffect(() => {
    conversationsRef.current = conversations;
    selectedIdRef.current = selectedId;
  }, [conversations, selectedId]);

  const selectedConversation = useMemo(
    () =>
      conversations.find((conversation) => conversation.id === selectedId) ??
      null,
    [conversations, selectedId],
  );
  const profileConversation = useMemo(
    () =>
      conversations.find(
        (conversation) => conversation.id === friendProfileId,
      ) ?? null,
    [conversations, friendProfileId],
  );

  const addNotification = useCallback((notification: AppNotification) => {
    setNotifications((current) => [
      notification,
      ...current.filter((item) => item.id !== notification.id),
    ]);
    setUnreadCount((current) => current + 1);
  }, []);

  const notifyUser = useCallback(
    (notification: AppNotification) => {
      addNotification(notification);
      if (document.hidden || !document.hasFocus()) {
        void window.noosphereDesktop?.system.notify(
          notification.title,
          notification.detail,
        );
      }
    },
    [addNotification],
  );

  const syncSocialState = useCallback(
    async (announceNew: boolean, requestsOnly = false) => {
      const desktop = window.noosphereDesktop;
      if (!desktop || socialSyncing.current) return false;
      socialSyncing.current = true;
      try {
        const nextState = requestsOnly
          ? await desktop.noosphere.syncRequests()
          : await desktop.noosphere.syncState();
        setConversations((current) => {
          if (announceNew) {
            const knownIds = new Set(
              current.map((conversation) => conversation.id),
            );
            for (const conversation of nextState.conversations) {
              if (!knownIds.has(conversation.id)) {
                notifyUser({
                  id: `conversation-${conversation.id}`,
                  title: 'Nouvel ami',
                  detail: `@${conversation.peer.login}`,
                });
              }
            }
          }
          return nextState.conversations;
        });
        setIncomingRequests((current) => {
          if (announceNew) {
            const knownIds = new Set(current.map((request) => request.id));
            for (const request of nextState.incoming) {
              if (!knownIds.has(request.id)) {
                notifyUser({
                  id: `friend-request-${request.id}`,
                  title: 'Demande d’ami',
                  detail: `@${request.user.login}`,
                });
              }
            }
          }
          return nextState.incoming;
        });
        setOutgoingRequests(nextState.outgoing);
        setSyncError('');
        return true;
      } catch (error) {
        setSyncError(readableError(error));
        return false;
      } finally {
        socialSyncing.current = false;
        setLoading(false);
      }
    },
    [notifyUser],
  );

  useEffect(() => startSocialRefreshLoop(syncSocialState), [syncSocialState]);

  const syncMessages = useCallback(
    async (conversationId: string, announceNew: boolean) => {
      const desktop = window.noosphereDesktop;
      if (!desktop || messagesSyncing.current.has(conversationId)) return;
      messagesSyncing.current.add(conversationId);
      try {
        const nextMessages =
          await desktop.noosphere.listMessages(conversationId);
        const previouslyKnown = knownMessageIds.current.get(conversationId);
        knownMessageIds.current.set(
          conversationId,
          new Set(nextMessages.map((message) => message.id)),
        );
        setMessages((current) => {
          const cached = messageCache.current.get(conversationId) ?? [];
          if (announceNew) {
            const knownIds =
              previouslyKnown ?? new Set(cached.map((message) => message.id));
            const received = nextMessages.filter(
              (message) => !message.own && !knownIds.has(message.id),
            );
            if (received.length > 0) {
              const conversation = conversationsRef.current.find(
                (candidate) => candidate.id === conversationId,
              );
              notifyUser({
                id: `message-${received.at(-1)?.id}`,
                title:
                  conversation?.peer.name ||
                  conversation?.peer.login ||
                  'Nouveau message',
                detail: received.at(-1)?.text || 'Nouveau message',
              });
            }
          }
          const merged = mergeMessages(cached, nextMessages);
          messageCache.current.set(conversationId, merged);
          return selectedIdRef.current === conversationId ? merged : current;
        });
      } catch (error) {
        setSyncError(readableError(error));
      } finally {
        messagesSyncing.current.delete(conversationId);
      }
    },
    [notifyUser],
  );

  const {
    status: directStatus,
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
  } = useDirectPeer(
    viewer,
    selectedConversation,
    () => {
      const conversationId = selectedIdRef.current;
      if (conversationId) void syncMessages(conversationId, true);
    },
    mediaSettings,
  );

  useEffect(() => {
    const previous = previousCallStatus.current;
    previousCallStatus.current = callStatus;
    if (
      callStatus === 'incoming' &&
      previous !== 'incoming' &&
      selectedConversation
    ) {
      notifyUser({
        id: `incoming-call-${selectedConversation.id}-${Date.now()}`,
        title: 'Appel entrant',
        detail: `@${selectedConversation.peer.login}`,
      });
    }
  }, [callStatus, notifyUser, selectedConversation]);

  useEffect(() => {
    let active = true;
    let socialRefreshPending = false;
    async function pollWakeSignals() {
      const desktop = window.noosphereDesktop;
      if (!desktop || wakeSyncing.current) return;
      wakeSyncing.current = true;
      try {
        const wake = await desktop.noosphere.pollWakeSignals();
        if (!active) return;
        if (wake.socialChanged) socialRefreshPending = true;
        if (socialRefreshPending) {
          socialRefreshPending = !(await syncSocialState(true, true));
        }
        await Promise.all(
          wake.conversationIds.map((conversationId) =>
            syncMessages(conversationId, true),
          ),
        );
      } catch {
      } finally {
        wakeSyncing.current = false;
      }
    }
    const pollTimer = window.setInterval(() => void pollWakeSignals(), 3_000);
    return () => {
      active = false;
      window.clearInterval(pollTimer);
    };
  }, [syncMessages, syncSocialState]);

  useEffect(() => {
    const conversationId = selectedId;
    const loadTimer = window.setTimeout(() => {
      if (!conversationId) {
        setMessages([]);
        return;
      }
      setMessages(messageCache.current.get(conversationId) ?? []);
      void syncMessages(conversationId, false);
    }, 0);
    return () => window.clearTimeout(loadTimer);
  }, [selectedId, syncMessages]);

  useEffect(() => {
    if (!selectedId) return;
    const pollTimer = window.setInterval(
      () => void syncMessages(selectedId, true),
      jitteredInterval(directStatus === 'direct' ? 60_000 : 5_000),
    );
    return () => {
      window.clearInterval(pollTimer);
    };
  }, [directStatus, selectedId, syncMessages]);

  useEffect(() => {
    let active = true;
    let running = false;
    async function synchronizeBackgroundMessages(announceNew: boolean) {
      const desktop = window.noosphereDesktop;
      if (!desktop || running) return;
      running = true;
      try {
        for (const conversation of conversationsRef.current) {
          if (!active) return;
          if (conversation.id === selectedIdRef.current) continue;
          try {
            const nextMessages = await desktop.noosphere.listMessages(
              conversation.id,
            );
            const previous = knownMessageIds.current.get(conversation.id);
            knownMessageIds.current.set(
              conversation.id,
              new Set(nextMessages.map((message) => message.id)),
            );
            if (!announceNew || !previous) continue;
            const received = nextMessages.filter(
              (message) => !message.own && !previous.has(message.id),
            );
            if (received.length > 0) {
              const latest = received.at(-1);
              notifyUser({
                id: `message-${latest?.id}`,
                title: conversation.peer.name || conversation.peer.login,
                detail: latest?.text || 'Nouveau message',
              });
            }
          } catch {}
        }
      } finally {
        running = false;
      }
    }
    const initialTimer = window.setTimeout(
      () => void synchronizeBackgroundMessages(false),
      0,
    );
    const pollTimer = window.setInterval(
      () => void synchronizeBackgroundMessages(true),
      jitteredInterval(45_000),
    );
    return () => {
      active = false;
      window.clearTimeout(initialTimer);
      window.clearInterval(pollTimer);
    };
  }, [notifyUser]);

  function handleFriendRequest(request: FriendRequest) {
    setOutgoingRequests((current) => [
      request,
      ...current.filter((item) => item.id !== request.id),
    ]);
    addNotification({
      id: `friend-request-${request.id}`,
      title: 'Demande envoyée',
      detail: `@${request.user.login}`,
    });
    void syncSocialState(true);
  }

  async function acceptFriendRequest(request: FriendRequest) {
    if (!window.noosphereDesktop) return;
    setAcceptingRequestId(request.id);
    setSyncError('');
    try {
      const conversation =
        await window.noosphereDesktop.noosphere.acceptFriendRequest(
          request.user.login,
          request.id,
        );
      setIncomingRequests((current) =>
        current.filter((item) => item.id !== request.id),
      );
      setConversations((current) => [
        conversation,
        ...current.filter((item) => item.id !== conversation.id),
      ]);
      setSelectedId(conversation.id);
      addNotification({
        id: `friend-accepted-${conversation.id}`,
        title: 'Ami ajouté',
        detail: `@${conversation.peer.login}`,
      });
    } catch (error) {
      setSyncError(readableError(error));
    } finally {
      setAcceptingRequestId(null);
    }
  }

  async function declineFriendRequest(request: FriendRequest) {
    const desktop = window.noosphereDesktop;
    if (!desktop) return;
    setDecliningRequestId(request.id);
    setSyncError('');
    try {
      await desktop.noosphere.declineFriendRequest(request.id);
      setIncomingRequests((current) =>
        current.filter((item) => item.id !== request.id),
      );
    } catch (error) {
      setSyncError(readableError(error));
    } finally {
      setDecliningRequestId(null);
    }
  }

  async function removeFriend(conversation: Conversation) {
    const desktop = window.noosphereDesktop;
    if (!desktop) return;
    setRemovingFriend(true);
    setSyncError('');
    try {
      await desktop.noosphere.removeFriend(conversation.id);
      if (selectedIdRef.current === conversation.id) {
        endCall();
        setSelectedId(null);
        setMessages([]);
      }
      messageCache.current.delete(conversation.id);
      knownMessageIds.current.delete(conversation.id);
      setConversations((current) =>
        current.filter((candidate) => candidate.id !== conversation.id),
      );
      setFriendProfileId(null);
    } catch (error) {
      setSyncError(readableError(error));
    } finally {
      setRemovingFriend(false);
    }
  }

  function beginCall() {
    if (!selectedConversation || !startCall('audio')) return;
    void window.noosphereDesktop?.noosphere
      .signalWake(selectedConversation.id)
      .catch(() => {});
  }

  async function submitMessage(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    const text = messageDraft.trim();
    if (!selectedConversation || !text || !window.noosphereDesktop) return;
    const conversationId = selectedConversation.id;
    const renderKey = `pending-${crypto.randomUUID()}`;
    const optimisticMessage: DisplayMessage = {
      version: 1,
      id: renderKey,
      conversationId,
      sentAt: new Date().toISOString(),
      text,
      senderId: viewer.id,
      own: true,
      pending: true,
      renderKey,
    };
    const optimisticMessages = [
      ...(messageCache.current.get(conversationId) ?? []),
      optimisticMessage,
    ];
    messageCache.current.set(conversationId, optimisticMessages);
    setMessages(optimisticMessages);
    setMessageDraft('');
    setSendingMessage(true);
    setSyncError('');
    try {
      const message = await window.noosphereDesktop.noosphere.sendMessage(
        conversationId,
        text,
      );
      const publishedMessages = (
        messageCache.current.get(conversationId) ?? []
      ).map((item) =>
        item.renderKey === renderKey
          ? { ...message, renderKey: item.renderKey }
          : item,
      );
      messageCache.current.set(conversationId, publishedMessages);
      if (selectedIdRef.current === conversationId) {
        setMessages(publishedMessages);
      }
      knownMessageIds.current.set(
        conversationId,
        new Set([
          ...(knownMessageIds.current.get(conversationId) ?? []),
          message.id,
        ]),
      );
      sendMessageSignal(conversationId, message.id);
    } catch (error) {
      const restoredMessages = (
        messageCache.current.get(conversationId) ?? []
      ).filter((item) => item.renderKey !== renderKey);
      messageCache.current.set(conversationId, restoredMessages);
      if (selectedIdRef.current === conversationId) {
        setMessages(restoredMessages);
        setMessageDraft((current) => current || text);
      }
      setSyncError(readableError(error));
    } finally {
      setSendingMessage(false);
    }
  }

  return (
    <main className="flex h-dvh min-h-[620px] overflow-hidden bg-[#1c1c1e] font-sans text-[#f5f5f7]">
      <aside className="flex w-[72px] shrink-0 flex-col items-center border-r border-white/[.07] bg-[#151517] py-3">
        <button
          type="button"
          aria-label="Messages directs"
          className={cn(
            'grid size-11 place-items-center rounded-[13px] text-white transition',
            selectedId === null
              ? 'bg-[var(--noosphere-accent-strong)]'
              : 'bg-[#2c2c2e] hover:bg-[#3a3a3c]',
          )}
          onClick={() => setSelectedId(null)}
        >
          <BrandLogo className="size-7" />
        </button>
        <div className="my-3 h-px w-7 bg-white/[.08]" />
        <button
          type="button"
          className="grid size-10 place-items-center rounded-[12px] bg-[#242426] text-[var(--noosphere-accent)] transition hover:bg-[#303033]"
          aria-label="Nouvelle discussion"
          onClick={() => setAddFriendOpen(true)}
        >
          <Plus className="size-5" />
        </button>
      </aside>

      <aside className="hidden w-[272px] shrink-0 flex-col border-r border-white/[.07] bg-[#202022] md:flex">
        <header className="flex h-[58px] items-center justify-between border-b border-white/[.07] px-4">
          <p className="text-[15px] font-semibold tracking-[-.01em]">
            Messages
          </p>
          <Button
            variant="ghost"
            size="icon"
            className="size-8 rounded-[9px] text-[#8e8e93] hover:bg-white/[.06] hover:text-white"
            aria-label="Ajouter un ami"
            onClick={() => setAddFriendOpen(true)}
          >
            <UserPlus />
          </Button>
        </header>

        <ScrollArea className="min-h-0 flex-1 p-2">
          {loading ? (
            <div className="grid h-24 place-items-center text-[#636366]">
              <BrandLoader className="size-4" />
            </div>
          ) : conversations.length === 0 ? (
            <div className="px-4 py-8 text-center">
              <MessageCircle className="mx-auto mb-3 size-6 text-[#636366]" />
              <p className="text-[13px] text-[#8e8e93]">Aucune discussion</p>
            </div>
          ) : (
            <div className="space-y-0.5">
              {conversations.map((conversation) => (
                <button
                  key={conversation.id}
                  type="button"
                  className={cn(
                    'flex w-full items-center gap-2.5 rounded-[11px] px-3 py-2.5 text-left transition',
                    selectedId === conversation.id
                      ? 'bg-white/[.09]'
                      : 'hover:bg-white/[.05]',
                  )}
                  onClick={() => setSelectedId(conversation.id)}
                >
                  <UserAvatar
                    login={conversation.peer.login}
                    name={conversation.peer.name}
                    avatarUrl={conversation.peer.avatarUrl}
                  />
                  <div className="min-w-0">
                    <p className="truncate text-[13px] font-medium text-[#f5f5f7]">
                      {conversation.peer.name || conversation.peer.login}
                    </p>
                    <p className="truncate text-[11px] text-[#737378]">
                      @{conversation.peer.login}
                    </p>
                  </div>
                </button>
              ))}
            </div>
          )}
          {syncError && (
            <p className="px-3 py-2 text-[11px] text-[#ff6961]">{syncError}</p>
          )}
        </ScrollArea>

        <div className="flex h-[66px] items-center gap-2.5 border-t border-white/[.07] bg-[#19191b] px-3">
          <div className="relative">
            <UserAvatar
              login={viewer.login}
              name={viewer.name}
              avatarUrl={viewer.avatarUrl}
            />
            <span className="absolute bottom-0 right-0 size-2.5 rounded-full border-2 border-[#19191b] bg-[var(--noosphere-accent)]" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="truncate text-[13px] font-medium text-white">
              {viewer.name || viewer.login}
            </p>
            <p className="truncate text-[11px] text-[#737378]">
              @{viewer.login}
            </p>
          </div>
          <Button
            variant="ghost"
            size="icon"
            className="size-9 rounded-[10px] text-[#8e8e93] hover:bg-white/[.06] hover:text-white"
            aria-label="Se déconnecter"
            onClick={onSignOut}
          >
            <LogOut />
          </Button>
        </div>
      </aside>

      <section className="relative flex min-w-0 flex-1 flex-col">
        <header className="flex h-[58px] shrink-0 items-center justify-between border-b border-white/[.07] px-5">
          <div className="flex min-w-0 items-center gap-2.5">
            {selectedConversation ? (
              <button
                type="button"
                className="flex min-w-0 items-center gap-2.5 rounded-[10px] px-1.5 py-1 text-left transition hover:bg-white/[.05]"
                aria-label={`Voir le profil de ${selectedConversation.peer.login}`}
                onClick={() => setFriendProfileId(selectedConversation.id)}
              >
                <UserAvatar
                  login={selectedConversation.peer.login}
                  name={selectedConversation.peer.name}
                  avatarUrl={selectedConversation.peer.avatarUrl}
                  size="sm"
                />
                <p className="truncate text-[14px] font-medium text-[#d1d1d6]">
                  {selectedConversation.peer.name ||
                    selectedConversation.peer.login}
                </p>
              </button>
            ) : (
              <p className="text-[14px] font-medium text-[#d1d1d6]">Amis</p>
            )}
          </div>
          <div className="flex items-center gap-2">
            {selectedConversation && callStatus === 'idle' && (
              <>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-9 rounded-[10px] text-[#a1a1a6] hover:bg-white/[.06] hover:text-white"
                  aria-label="Démarrer un appel vocal"
                  title={
                    directStatus === 'direct'
                      ? 'Appel vocal'
                      : 'Connexion avec cet ami en cours'
                  }
                  disabled={directStatus !== 'direct'}
                  onClick={beginCall}
                >
                  <Phone />
                </Button>
              </>
            )}
            {selectedConversation && callStatus !== 'idle' && (
              <Button
                variant="ghost"
                size="icon"
                className="size-9 rounded-[10px] bg-[#ff453a]/15 text-[#ff6961] hover:bg-[#ff453a]/25 hover:text-[#ff8a84]"
                aria-label="Raccrocher"
                onClick={endCall}
              >
                <PhoneOff />
              </Button>
            )}
            <Button
              variant="ghost"
              size="icon"
              className="size-9 rounded-[10px] text-[#a1a1a6] hover:bg-white/[.06] hover:text-white"
              aria-label="Paramètres audio et vidéo"
              onClick={() => setMediaSettingsOpen(true)}
            >
              <Settings2 />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="relative size-9 rounded-[10px] text-[#a1a1a6] hover:bg-white/[.06] hover:text-white"
              aria-label="Notifications"
              onClick={() => {
                setNotificationsOpen((current) => !current);
                setUnreadCount(0);
              }}
            >
              <Bell />
              {unreadCount > 0 && (
                <span className="absolute right-1.5 top-1.5 min-w-3.5 rounded-full bg-[#ff453a] px-1 text-[9px] leading-3.5 text-white">
                  {unreadCount}
                </span>
              )}
            </Button>
          </div>
        </header>

        {notificationsOpen && (
          <div className="absolute right-4 top-[66px] z-20 w-[310px] overflow-hidden rounded-[18px] border border-white/[.09] bg-[#28282a]/95 shadow-2xl backdrop-blur-2xl">
            <div className="border-b border-white/[.07] px-4 py-3 text-[13px] font-medium">
              Notifications
            </div>
            {notifications.length === 0 ? (
              <p className="px-4 py-8 text-center text-[12px] text-[#8e8e93]">
                Aucune notification
              </p>
            ) : (
              <div className="max-h-72 overflow-y-auto p-2">
                {notifications.map((notification) => (
                  <div
                    key={notification.id}
                    className="rounded-[12px] px-3 py-2.5 hover:bg-white/[.05]"
                  >
                    <p className="text-[12px] font-medium">
                      {notification.title}
                    </p>
                    <p className="mt-0.5 truncate text-[11px] text-[#8e8e93]">
                      {notification.detail}
                    </p>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {selectedConversation && (
          <VoiceCallOverlay
            status={callStatus}
            peer={selectedConversation.peer}
            localStream={localStream}
            remoteStream={remoteStream}
            microphoneEnabled={microphoneEnabled}
            cameraEnabled={cameraEnabled}
            onAccept={() => void acceptCall()}
            onDecline={declineCall}
            onEnd={endCall}
            onToggleMicrophone={toggleMicrophone}
            onToggleCamera={toggleCamera}
          />
        )}

        {callError && (
          <p
            className={cn(
              'absolute right-5 top-[70px] z-20 max-w-[320px] rounded-[12px] px-3 py-2 text-[11px] shadow-xl',
              callStatus === 'idle'
                ? 'border border-[#ff453a]/25 bg-[#321f20] text-[#ff9f9a]'
                : 'border border-[var(--noosphere-accent)]/25 bg-[#1b2b22] text-[#a8d6bb]',
            )}
          >
            {callError}
          </p>
        )}

        {selectedConversation ? (
          <div className="flex min-h-0 flex-1 flex-col">
            <ScrollArea className="min-h-0 flex-1 px-5 py-5">
              <div className="mx-auto flex w-full max-w-[760px] flex-col gap-2">
                {messages.length === 0 ? (
                  <div className="py-16 text-center">
                    <UserAvatar
                      login={selectedConversation.peer.login}
                      name={selectedConversation.peer.name}
                      avatarUrl={selectedConversation.peer.avatarUrl}
                      size="lg"
                    />
                    <h1 className="mt-4 text-[20px] font-semibold text-white">
                      {selectedConversation.peer.name ||
                        selectedConversation.peer.login}
                    </h1>
                  </div>
                ) : (
                  messages.map((message) => (
                    <div
                      key={message.renderKey}
                      className={cn(
                        'noosphere-message-enter flex',
                        message.own ? 'justify-end' : 'justify-start',
                      )}
                    >
                      <div
                        className={cn(
                          'max-w-[72%] rounded-[18px] px-3.5 py-2.5 text-[13px] leading-5 shadow-sm transition-opacity duration-200',
                          message.own
                            ? 'rounded-br-[6px] bg-[var(--noosphere-accent-strong)] text-white'
                            : 'rounded-bl-[6px] bg-[#2c2c2e] text-[#f5f5f7]',
                          message.pending && 'opacity-75',
                        )}
                      >
                        <p className="whitespace-pre-wrap break-words">
                          {message.text}
                        </p>
                        <p
                          className={cn(
                            'mt-1 text-right text-[9px]',
                            message.own ? 'text-white/60' : 'text-[#737378]',
                          )}
                        >
                          {new Date(message.sentAt).toLocaleTimeString(
                            'fr-FR',
                            {
                              hour: '2-digit',
                              minute: '2-digit',
                            },
                          )}
                        </p>
                      </div>
                    </div>
                  ))
                )}
              </div>
            </ScrollArea>
            <form
              className="shrink-0 border-t border-white/[.07] px-5 py-4"
              onSubmit={(event) => void submitMessage(event)}
            >
              <div className="mx-auto flex max-w-[760px] items-end gap-2 rounded-[16px] border border-white/[.08] bg-[#242426] p-2">
                <Input
                  value={messageDraft}
                  onChange={(event) => setMessageDraft(event.target.value)}
                  placeholder={`Message à @${selectedConversation.peer.login}`}
                  maxLength={4000}
                  autoComplete="off"
                  className="min-h-10 flex-1 border-0 bg-transparent text-[13px] text-white shadow-none placeholder:text-[#636366] focus-visible:ring-0"
                />
                <Button
                  type="submit"
                  size="icon"
                  aria-label="Envoyer le message"
                  disabled={sendingMessage || !messageDraft.trim()}
                  className="size-10 shrink-0 rounded-[12px] bg-[var(--noosphere-accent)] text-[var(--noosphere-on-accent)] hover:bg-[var(--noosphere-accent-hover)]"
                >
                  {sendingMessage ? (
                    <BrandLoader className="size-5" />
                  ) : (
                    <SendHorizontal />
                  )}
                </Button>
              </div>
            </form>
          </div>
        ) : (
          <ScrollArea className="min-h-0 flex-1 px-6 py-7">
            <div className="mx-auto max-w-[760px]">
              <div className="flex items-center justify-between gap-4">
                <div>
                  <h1 className="text-[24px] font-semibold tracking-[-.03em] text-white">
                    Amis
                  </h1>
                </div>
                <Button
                  className="h-10 rounded-[12px] bg-[var(--noosphere-accent)] px-4 text-[13px] text-[var(--noosphere-on-accent)] hover:bg-[var(--noosphere-accent-hover)]"
                  onClick={() => setAddFriendOpen(true)}
                >
                  <UserPlus />
                  Ajouter
                </Button>
              </div>

              {incomingRequests.length > 0 && (
                <section className="mt-8">
                  <h2 className="mb-3 text-[11px] font-semibold uppercase tracking-[.08em] text-[#737378]">
                    Demandes reçues — {incomingRequests.length}
                  </h2>
                  <div className="space-y-2">
                    {incomingRequests.map((request) => (
                      <div
                        key={request.id}
                        className="flex items-center gap-3 rounded-[16px] border border-white/[.07] bg-[#242426] p-3"
                      >
                        <UserAvatar
                          login={request.user.login}
                          name={request.user.name}
                          avatarUrl={request.user.avatarUrl}
                        />
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-[13px] font-medium">
                            {request.user.name || request.user.login}
                          </p>
                          <p className="truncate text-[11px] text-[#737378]">
                            @{request.user.login}
                          </p>
                        </div>
                        <div className="flex items-center gap-2">
                          <Button
                            variant="ghost"
                            className="h-9 rounded-[10px] px-3 text-[12px] text-[#ff6961] hover:bg-[#ff453a]/10 hover:text-[#ff8a84]"
                            disabled={
                              acceptingRequestId === request.id ||
                              decliningRequestId === request.id
                            }
                            onClick={() => void declineFriendRequest(request)}
                          >
                            {decliningRequestId === request.id ? (
                              <BrandLoader className="size-4" />
                            ) : (
                              <X />
                            )}
                            Refuser
                          </Button>
                          <Button
                            className="h-9 rounded-[10px] bg-[var(--noosphere-accent)] px-3 text-[12px] text-[var(--noosphere-on-accent)] hover:bg-[var(--noosphere-accent-hover)]"
                            disabled={
                              acceptingRequestId === request.id ||
                              decliningRequestId === request.id
                            }
                            onClick={() => void acceptFriendRequest(request)}
                          >
                            {acceptingRequestId === request.id ? (
                              <BrandLoader className="size-4" />
                            ) : (
                              <Check />
                            )}
                            Accepter
                          </Button>
                        </div>
                      </div>
                    ))}
                  </div>
                </section>
              )}

              <section className="mt-8">
                <h2 className="mb-3 text-[11px] font-semibold uppercase tracking-[.08em] text-[#737378]">
                  Amis — {conversations.length}
                </h2>
                {conversations.length === 0 ? (
                  <div className="rounded-[18px] border border-dashed border-white/[.09] py-12 text-center">
                    <Users className="mx-auto size-6 text-[#636366]" />
                    <p className="mt-3 text-[13px] text-[#8e8e93]">
                      Aucun ami pour le moment
                    </p>
                  </div>
                ) : (
                  <div className="space-y-2">
                    {conversations.map((conversation) => (
                      <button
                        key={conversation.id}
                        type="button"
                        onClick={() => setFriendProfileId(conversation.id)}
                        className="flex w-full items-center gap-3 rounded-[16px] border border-white/[.07] bg-[#242426] p-3 text-left transition hover:bg-[#2c2c2e]"
                      >
                        <UserAvatar
                          login={conversation.peer.login}
                          name={conversation.peer.name}
                          avatarUrl={conversation.peer.avatarUrl}
                        />
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-[13px] font-medium">
                            {conversation.peer.name || conversation.peer.login}
                          </p>
                          <p className="truncate text-[11px] text-[#737378]">
                            @{conversation.peer.login}
                          </p>
                        </div>
                        <UserRound className="size-4 text-[var(--noosphere-accent)]" />
                      </button>
                    ))}
                  </div>
                )}
              </section>

              {outgoingRequests.length > 0 && (
                <section className="mt-8">
                  <h2 className="mb-3 text-[11px] font-semibold uppercase tracking-[.08em] text-[#737378]">
                    En attente — {outgoingRequests.length}
                  </h2>
                  <div className="space-y-2">
                    {outgoingRequests.map((request) => (
                      <div
                        key={request.id}
                        className="flex items-center gap-3 rounded-[16px] bg-[#202022] p-3"
                      >
                        <UserAvatar
                          login={request.user.login}
                          name={request.user.name}
                          avatarUrl={request.user.avatarUrl}
                        />
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-[13px]">
                            {request.user.name || request.user.login}
                          </p>
                          <p className="truncate text-[11px] text-[#737378]">
                            @{request.user.login}
                          </p>
                        </div>
                        <span className="flex items-center gap-1 text-[11px] text-[#8e8e93]">
                          <Clock3 className="size-3.5" />
                          Envoyée
                        </span>
                      </div>
                    ))}
                  </div>
                </section>
              )}
            </div>
          </ScrollArea>
        )}
      </section>

      <AddFriendDialog
        open={addFriendOpen}
        onOpenChange={setAddFriendOpen}
        onRequest={handleFriendRequest}
      />
      <FriendProfileDialog
        key={profileConversation?.id ?? 'closed'}
        conversation={profileConversation}
        open={profileConversation !== null}
        removing={removingFriend}
        onOpenChange={(open) => {
          if (!open) setFriendProfileId(null);
        }}
        onMessage={() => {
          if (!profileConversation) return;
          setSelectedId(profileConversation.id);
          setFriendProfileId(null);
        }}
        onRemove={async () => {
          if (profileConversation) await removeFriend(profileConversation);
        }}
      />
      <MediaSettingsDialog
        open={mediaSettingsOpen}
        onOpenChange={setMediaSettingsOpen}
        settings={mediaSettings}
        devices={mediaDevices}
        loading={mediaDevicesLoading}
        error={mediaDevicesError}
        onChange={updateMediaSettings}
        onRequestAccess={() => void refreshMediaDevices(true)}
      />
    </main>
  );
}

export default function Home() {
  const [session, setSession] = useState<
    { viewer: GitHubViewer; social: SocialState } | null | undefined
  >(undefined);

  async function openSession(viewer: GitHubViewer) {
    const desktop = window.noosphereDesktop;
    let social = EMPTY_SOCIAL_STATE;
    if (desktop) {
      try {
        social = await desktop.noosphere.cachedState();
      } catch {}
    }
    setSession({ viewer, social });
  }

  useEffect(() => {
    let active = true;
    let validationTimer: number | undefined;
    const desktop = window.noosphereDesktop;
    const restoredSession = desktop?.github.restore() ?? Promise.resolve(null);
    void restoredSession
      .then(async (restored) => {
        if (!active) return;
        if (!restored || !desktop) {
          setSession(null);
          return;
        }
        let social = EMPTY_SOCIAL_STATE;
        try {
          social = await desktop.noosphere.cachedState();
        } catch {}
        if (!active) return;
        setSession({ viewer: restored, social });
        validationTimer = window.setTimeout(() => {
          void desktop.github
            .validateSession()
            .then((validated) => {
              if (!active) return;
              if (!validated) {
                setSession(null);
                return;
              }
              setSession((current) =>
                current ? { ...current, viewer: validated } : current,
              );
            })
            .catch(() => {});
        }, 1_500);
      })
      .catch(() => {
        if (active) setSession(null);
      });
    return () => {
      active = false;
      if (validationTimer !== undefined) window.clearTimeout(validationTimer);
    };
  }, []);

  async function signOut() {
    await window.noosphereDesktop?.github.signOut();
    setSession(null);
  }

  if (session === undefined) {
    return <LoadingScreen />;
  }
  if (!session) return <LoginScreen onGitHub={openSession} />;
  return (
    <NoosphereApp
      viewer={session.viewer}
      initialSocialState={session.social}
      onSignOut={() => void signOut()}
    />
  );
}
