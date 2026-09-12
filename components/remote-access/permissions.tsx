import { useState } from 'react';
import { Fingerprint, Monitor, ShieldCheck, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import {
  NO_PERMISSIONS,
  editablePermissions,
  machineId,
  samePrincipal,
  type RemoteMachine,
  type RemoteUser,
  type RemoteGrant,
  type RemoteUserGrant,
  userGrants,
} from '@/lib/remote-access';
import type { RemoteAccessModel } from '@/lib/use-remote-access';
import {
  accentButton,
  PermissionControls,
  SettingRow,
  SettingsGroup,
  Toggle,
} from './controls';

function IdentityDetail({ machine }: { machine: RemoteMachine }) {
  return (
    <details className="text-[11px] text-[#8e8e93]">
      <summary className="flex cursor-pointer items-center gap-2 py-2">
        <Fingerprint className="size-3.5" />
        {machine.identityVerified ? 'Identité reconnue' : 'Identité à vérifier'}
      </summary>
      <p className="mt-2 leading-5">
        Compte GitHub #{machine.principal.githubUserId} · Machine{' '}
        {machineId(machine.principal)}
      </p>
      <p className="mt-1 break-all font-mono leading-5">
        {machine.principal.identityKey
          .map((n) => n.toString(16).padStart(2, '0'))
          .join(' ')}
      </p>
    </details>
  );
}

export function UserGrantEditor({
  user,
  grants,
  userPolicies,
  remote,
  onClose,
}: {
  user: RemoteUser;
  grants: RemoteGrant[];
  userPolicies: RemoteUserGrant[];
  remote: RemoteAccessModel;
  onClose: () => void;
}) {
  const current = userGrants(user, grants, userPolicies);
  const [permissions, setPermissions] = useState(
    editablePermissions(
      current.allowedMachines
        ? current.permissions
        : { ...NO_PERMISSIONS, screen: true },
    ),
  );
  const [unattended, setUnattended] = useState(current.unattended);
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent
        showCloseButton={false}
        className="max-h-[90vh] overflow-auto rounded-[20px] border-white/[.09] bg-[#202022] p-6 text-[#f5f5f7] sm:max-w-[480px]"
      >
        <div className="flex items-start justify-between">
          <div>
            <DialogTitle className="text-[18px] tracking-tight">
              {user.own ? 'Mon accès' : `Accès de @${user.login}`}
            </DialogTitle>
            <DialogDescription className="mt-1 text-[12px] text-[#8e8e93]">
              {user.own
                ? 'Mes machines → cet ordinateur'
                : 'Ses machines → cet ordinateur'}
            </DialogDescription>
          </div>
          <Button
            variant="ghost"
            size="icon"
            aria-label="Fermer"
            onClick={onClose}
          >
            <X />
          </Button>
        </div>
        <SettingsGroup title="Permissions">
          <PermissionControls
            value={permissions}
            onChange={(next) => {
              setPermissions(next);
              if (!next.screen) setUnattended(false);
            }}
          />
        </SettingsGroup>
        <SettingsGroup title="Approbation">
          <SettingRow title="Se connecter sans confirmation">
            <Toggle
              label="Se connecter sans confirmation"
              checked={unattended}
              disabled={!permissions.screen}
              onChange={setUnattended}
            />
          </SettingRow>
        </SettingsGroup>
        {remote.error && (
          <p role="alert" className="text-xs text-[#ff9f9a]">
            {remote.error}
          </p>
        )}
        <div className="flex items-center justify-between gap-3">
          {current.configured || current.allowedMachines ? (
            <Button
              variant="ghost"
              className="text-[#ff9f9a]"
              disabled={remote.busy}
              onClick={() =>
                void remote
                  .run((api) =>
                    api.setUserGrant(user.id, NO_PERMISSIONS, false, true),
                  )
                  .then((ok) => {
                    if (ok) onClose();
                  })
              }
            >
              Révoquer l’accès
            </Button>
          ) : (
            <span />
          )}
          <Button
            className={accentButton}
            disabled={remote.busy}
            onClick={() =>
              void remote
                .run((api) =>
                  api.setUserGrant(user.id, permissions, unattended),
                )
                .then((ok) => {
                  if (ok) onClose();
                })
            }
          >
            Enregistrer
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

export function IncomingRemoteRequest({
  remote,
}: {
  remote: RemoteAccessModel;
}) {
  const incoming = remote.directory?.incoming.find(
    (r) => r.decision === 'pending' && r.request.expiresAt * 1000 > remote.time,
  );
  if (!incoming) return null;
  const machine = remote.directory?.machines.find((m) =>
    samePrincipal(m.principal, incoming.request.guest),
  );
  if (!machine) return null;
  return (
    <AccessPrompt
      key={incoming.request.id.join('.')}
      remote={remote}
      machine={machine}
      request={incoming}
    />
  );
}

function AccessPrompt({
  remote,
  machine,
  request,
}: {
  remote: RemoteAccessModel;
  machine: RemoteMachine;
  request: NonNullable<RemoteAccessModel['directory']>['incoming'][number];
}) {
  const [permissions, setPermissions] = useState(
    editablePermissions(request.request.permissions),
  );
  const [remember, setRemember] = useState(false);
  const [unattended, setUnattended] = useState(false);
  return (
    <Dialog open>
      <DialogContent
        showCloseButton={false}
        className="max-h-[90vh] overflow-auto rounded-[22px] border-white/10 bg-[#202022] p-6 text-[#f5f5f7] sm:max-w-[470px]"
      >
        <div className="mx-auto grid size-14 place-items-center rounded-[16px] border border-[var(--noosphere-accent)]/20 bg-[var(--noosphere-accent)]/[.08] text-[var(--noosphere-accent)]">
          <Monitor className="size-7" strokeWidth={1.5} />
        </div>
        <div className="text-center">
          <DialogTitle className="text-[20px] tracking-tight">
            Demande de connexion
          </DialogTitle>
          <DialogDescription className="mt-2 text-[13px] leading-5 text-[#a1a1a6]">
            <span className="font-medium text-[#e5e5ea]">@{machine.login}</span>
            {' · '}
            <span className="text-[#e5e5ea]">{machine.name}</span>
          </DialogDescription>
        </div>
        <SettingsGroup title="Permissions">
          <PermissionControls
            value={permissions}
            onChange={setPermissions}
            requested={request.request.permissions}
          />
        </SettingsGroup>
        <div className="space-y-3 px-1">
          <label className="flex items-center gap-2.5 text-[12px] text-[#c7c7cc]">
            <input
              type="checkbox"
              className="accent-[var(--noosphere-accent)]"
              checked={remember}
              onChange={(e) => {
                setRemember(e.target.checked);
                if (!e.target.checked) setUnattended(false);
              }}
            />
            Mémoriser ces permissions
          </label>
          <label className="flex items-center gap-2.5 text-[12px] text-[#c7c7cc]">
            <input
              type="checkbox"
              className="accent-[var(--noosphere-accent)]"
              checked={unattended}
              onChange={(e) => {
                setUnattended(e.target.checked);
                if (e.target.checked) setRemember(true);
              }}
            />
            Prochaines connexions sans confirmation
          </label>
        </div>
        <IdentityDetail machine={machine} />
        {remote.error && (
          <p role="alert" className="text-xs text-[#ff9f9a]">
            {remote.error}
          </p>
        )}
        <div className="grid grid-cols-2 gap-3">
          <Button
            variant="outline"
            className="rounded-[10px] border-white/10 bg-white/[.03]"
            disabled={remote.busy}
            onClick={() =>
              void remote.run((api) =>
                api.decideAccess(
                  request.request.id,
                  NO_PERMISSIONS,
                  false,
                  false,
                ),
              )
            }
          >
            Refuser
          </Button>
          <Button
            className={accentButton}
            disabled={remote.busy || !permissions.screen}
            onClick={() =>
              void remote.run((api) =>
                api.decideAccess(
                  request.request.id,
                  permissions,
                  remember,
                  unattended,
                ),
              )
            }
          >
            <ShieldCheck className="size-4" />
            Autoriser
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
