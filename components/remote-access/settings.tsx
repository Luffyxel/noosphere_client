import { useEffect, useState } from 'react';
import {
  Check,
  ChevronRight,
  Monitor,
  ShieldCheck,
  SlidersHorizontal,
  Activity,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { APP_VERSION } from '@/lib/app-version';
import type { Conversation, GitHubViewer } from '@/lib/desktop-bridge';
import {
  remoteUsers,
  userGrants,
  type RemoteSettings,
  type RemoteStatus,
  type RemoteUser,
} from '@/lib/remote-access';
import type { RemoteAccessModel } from '@/lib/use-remote-access';
import { cn } from '@/lib/utils';
import { LocalRemoteTest } from './local-test';
import {
  accentButton,
  fieldClass,
  SettingRow,
  SettingsGroup,
  Toggle,
} from './controls';
import { UserGrantEditor } from './permissions';

export function RemoteAccessSettings({
  remote,
  friends,
  viewer,
}: {
  remote: RemoteAccessModel;
  friends: Conversation[];
  viewer: GitHubViewer;
}) {
  const [tab, setTab] = useState<'general' | 'access' | 'media' | 'diagnostic'>(
    'general',
  );
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [settings, setSettings] = useState<RemoteSettings | null>(null);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [editing, setEditing] = useState<RemoteUser | null>(null);
  const data = remote.directory;
  const accessUsers = remoteUsers(viewer, friends, data?.machines ?? []);
  const allowedUsers = new Set([
    ...(data?.userGrants
      .filter((grant) => grant.permissions.screen)
      .map((grant) => grant.githubUserId) ?? []),
    ...(data?.grants
      .filter((grant) => grant.permissions.screen)
      .map((grant) => grant.principal.githubUserId) ?? []),
  ]).size;
  useEffect(() => {
    let active = true;
    void window.noosphereDesktop?.remoteAccess
      .status()
      .then((next) => {
        if (active) {
          setStatus(next);
          setSettings(next.settings);
        }
      })
      .catch((failure) => {
        if (active) setError(String(failure));
      });
    return () => {
      active = false;
    };
  }, []);
  function update(next: RemoteSettings) {
    setSettings(next);
    setSaved(false);
  }
  async function save() {
    if (!settings || saving) return;
    setSaving(true);
    setError('');
    setSaved(false);
    try {
      const next =
        await window.noosphereDesktop?.remoteAccess.saveSettings(settings);
      if (next) {
        setStatus(next);
        setSettings(next.settings);
        setSaved(true);
      }
    } catch (failure) {
      setError(String(failure));
    } finally {
      setSaving(false);
    }
  }
  const tabs = [
    { id: 'general', title: 'Général', icon: Monitor },
    { id: 'access', title: 'Accès autorisés', icon: ShieldCheck },
    { id: 'media', title: 'Image et son', icon: SlidersHorizontal },
    { id: 'diagnostic', title: 'Diagnostic', icon: Activity },
  ] as const;
  return (
    <div className="mx-auto w-full max-w-[850px] px-6 py-8 lg:px-10">
      <div className="mb-7">
        <h1 className="text-[25px] font-semibold tracking-[-.035em]">
          Réglages du bureau à distance
        </h1>
      </div>
      <div
        className="mb-7 flex flex-wrap gap-1 rounded-[12px] border border-white/[.06] bg-[#141416]/50 p-1"
        role="tablist"
        aria-label="Réglages du bureau à distance"
      >
        {tabs.map(({ id, title, icon: Icon }) => (
          <button
            key={id}
            id={`remote-tab-${id}`}
            role="tab"
            aria-selected={tab === id}
            aria-controls="remote-settings-panel"
            tabIndex={tab === id ? 0 : -1}
            onKeyDown={(event) => {
              if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
                event.preventDefault();
                const index = tabs.findIndex((t) => t.id === tab);
                const next =
                  tabs[
                    (index +
                      (event.key === 'ArrowRight' ? 1 : -1) +
                      tabs.length) %
                      tabs.length
                  ];
                setTab(next.id);
                document.getElementById(`remote-tab-${next.id}`)?.focus();
              }
            }}
            onClick={() => setTab(id)}
            className={cn(
              'flex flex-1 items-center justify-center gap-2 whitespace-nowrap rounded-[9px] px-3 py-2 text-[12px] transition',
              tab === id
                ? 'bg-[#363638] text-white shadow-sm'
                : 'text-[#8e8e93] hover:text-[#d1d1d6]',
            )}
          >
            <Icon className="size-3.5" />
            {title}
          </button>
        ))}
      </div>
      {error && (
        <p
          role="alert"
          className="mb-5 rounded-xl bg-[#ff453a]/10 p-3 text-[12px] text-[#ff9f9a]"
        >
          {error}
        </p>
      )}
      <div
        id="remote-settings-panel"
        role="tabpanel"
        aria-labelledby={`remote-tab-${tab}`}
        className="space-y-6"
      >
        {tab === 'general' && (
          <>
            <div className="flex items-center gap-4 px-1 pb-2">
              <div className="grid size-[68px] place-items-center rounded-[18px] border border-white/[.07] bg-gradient-to-b from-[#343c36] to-[#232725] text-[var(--noosphere-accent)]">
                <Monitor className="size-9" strokeWidth={1.25} />
              </div>
              <div>
                <h2 className="text-[18px] font-medium tracking-tight">
                  {settings?.machineName ?? 'Cet ordinateur'}
                </h2>
              </div>
            </div>
            <SettingsGroup title="Accès à cet ordinateur">
              <SettingRow title="Autoriser les demandes d’accès">
                <Toggle
                  label="Autoriser les demandes d’accès"
                  checked={data?.hostEnabled ?? false}
                  disabled={!data || remote.busy}
                  onChange={(enabled) =>
                    void remote.run((api) => api.setHost(enabled))
                  }
                />
              </SettingRow>
              <SettingRow title="Utilisateurs autorisés">
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-[var(--noosphere-accent)]"
                  onClick={() => setTab('access')}
                >
                  Gérer ({allowedUsers})
                  <ChevronRight className="size-4" />
                </Button>
              </SettingRow>
            </SettingsGroup>
            {settings && (
              <SettingsGroup title="Identification">
                <SettingRow title="Nom de l’ordinateur">
                  <input
                    aria-label="Nom de l’ordinateur"
                    className={cn(fieldClass, 'max-w-[250px]')}
                    value={settings.machineName}
                    maxLength={64}
                    onChange={(e) =>
                      update({ ...settings, machineName: e.target.value })
                    }
                  />
                </SettingRow>
              </SettingsGroup>
            )}
            <SettingsGroup title="Noosphere">
              <SettingRow title="Version">
                <span className="text-[12px] tabular-nums text-[#a1a1a6]">
                  {APP_VERSION}
                </span>
              </SettingRow>
            </SettingsGroup>
          </>
        )}
        {tab === 'access' && (
          <>
            <SettingsGroup title="Utilisateurs">
              {accessUsers.map((user) => {
                const access = userGrants(
                  user,
                  data?.grants ?? [],
                  data?.userGrants ?? [],
                );
                const detail = access.configured
                  ? access.unattended
                    ? 'Sans confirmation'
                    : 'Sur demande'
                  : access.allowedMachines
                    ? 'Accès partiel'
                    : 'Accès désactivé';
                return (
                  <SettingRow
                    key={user.id}
                    title={user.own ? 'Moi' : `@${user.login}`}
                    detail={detail}
                  >
                    <Button
                      variant="ghost"
                      size="sm"
                      className="text-[var(--noosphere-accent)]"
                      disabled={remote.busy}
                      onClick={() => setEditing(user)}
                    >
                      Gérer
                      <ChevronRight className="size-4" />
                    </Button>
                  </SettingRow>
                );
              })}
            </SettingsGroup>
          </>
        )}
        {tab === 'media' && settings && (
          <>
            <SettingsGroup title="Affichage">
              <SettingRow title="Écran partagé">
                <select
                  aria-label="Écran partagé"
                  className={cn(fieldClass, 'max-w-[240px]')}
                  value={settings.display}
                  onChange={(e) =>
                    update({ ...settings, display: Number(e.target.value) })
                  }
                >
                  {status?.displays.length ? (
                    status.displays.map((d) => (
                      <option key={d.id} value={d.id}>
                        Écran {d.id + 1} · {d.width} × {d.height}
                      </option>
                    ))
                  ) : (
                    <option value={settings.display}>
                      Aucun écran détecté
                    </option>
                  )}
                </select>
              </SettingRow>
              <SettingRow title="Résolution">
                <select
                  aria-label="Résolution"
                  className={cn(fieldClass, 'max-w-[200px]')}
                  value={`${settings.video.width}x${settings.video.height}`}
                  onChange={(e) => {
                    const [width, height] = e.target.value
                      .split('x')
                      .map(Number);
                    update({
                      ...settings,
                      video: { ...settings.video, width, height },
                    });
                  }}
                >
                  {['1280x720', '1920x1080', '2560x1440', '3840x2160'].map(
                    (r) => (
                      <option key={r} value={r}>
                        {r.replace('x', ' × ')}
                      </option>
                    ),
                  )}
                </select>
              </SettingRow>
              <SettingRow title="Fréquence d’images">
                <select
                  aria-label="Fréquence d’images"
                  className={cn(fieldClass, 'max-w-[130px]')}
                  value={settings.video.fps}
                  onChange={(e) =>
                    update({
                      ...settings,
                      video: { ...settings.video, fps: Number(e.target.value) },
                    })
                  }
                >
                  {[30, 60, 120, 144, 240].map((fps) => (
                    <option key={fps} value={fps}>
                      {fps} FPS
                    </option>
                  ))}
                </select>
              </SettingRow>
            </SettingsGroup>
            <SettingsGroup title="Qualité">
              <SettingRow title="Codec préféré">
                <select
                  aria-label="Codec préféré"
                  className={cn(fieldClass, 'max-w-[130px]')}
                  value={settings.video.codec}
                  onChange={(e) =>
                    update({
                      ...settings,
                      video: {
                        ...settings.video,
                        codec: e.target
                          .value as RemoteSettings['video']['codec'],
                      },
                    })
                  }
                >
                  <option value="h264">H.264</option>
                  <option value="hevc">HEVC</option>
                  <option value="av1">AV1</option>
                </select>
              </SettingRow>
              <SettingRow title="Débit maximal">
                <div className="flex flex-wrap items-center justify-end gap-3">
                  <output className="text-[12px] tabular-nums text-[#a1a1a6]">
                    {Math.round(settings.video.bitrate / 1_000_000)} Mbit/s
                  </output>
                  <input
                    aria-label="Débit maximal"
                    type="range"
                    min={1}
                    max={100}
                    value={settings.video.bitrate / 1_000_000}
                    className="w-[180px] accent-[var(--noosphere-accent)]"
                    onChange={(e) =>
                      update({
                        ...settings,
                        video: {
                          ...settings.video,
                          bitrate: Number(e.target.value) * 1_000_000,
                        },
                      })
                    }
                  />
                </div>
              </SettingRow>
            </SettingsGroup>
          </>
        )}
        {tab === 'diagnostic' && settings && (
          <LocalRemoteTest
            settings={settings}
            initial={status?.diagnostic ?? { state: 'idle' }}
          />
        )}
        {!settings &&
          (tab === 'general' || tab === 'media' || tab === 'diagnostic') &&
          !error && (
            <output className="block text-[13px] text-[#8e8e93]">
              Chargement des réglages…
            </output>
          )}
        {(tab === 'general' || tab === 'media') && settings && (
          <div className="flex items-center justify-end gap-4 pt-1">
            <output className="text-[12px] text-[var(--noosphere-accent)]">
              {saved && (
                <span className="flex items-center gap-1">
                  <Check className="size-3.5" />
                  Enregistré
                </span>
              )}
            </output>
            <Button
              className={accentButton}
              disabled={saving || !settings.machineName.trim()}
              onClick={() => void save()}
            >
              {saving ? 'Enregistrement…' : 'Enregistrer'}
            </Button>
          </div>
        )}
      </div>
      {editing && (
        <UserGrantEditor
          key={editing.id}
          user={editing}
          grants={data?.grants ?? []}
          userPolicies={data?.userGrants ?? []}
          remote={remote}
          onClose={() => setEditing(null)}
        />
      )}
    </div>
  );
}
