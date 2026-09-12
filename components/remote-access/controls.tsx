import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';
import type { RemotePermissions } from '@/lib/remote-access';

export const accentButton =
  'rounded-[10px] bg-[var(--noosphere-accent)] text-[var(--noosphere-on-accent)] hover:bg-[var(--noosphere-accent-hover)]';
export const fieldClass =
  'w-full rounded-[9px] border border-white/[.08] bg-[#18181a] px-3 py-2 text-[13px] text-[#f5f5f7] outline-none focus:border-[var(--noosphere-accent)]';

export function Toggle({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        'relative h-[22px] w-[38px] shrink-0 rounded-full border border-white/[.06] transition-colors focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[var(--noosphere-accent)] disabled:opacity-40',
        checked ? 'bg-[var(--noosphere-accent-strong)]' : 'bg-[#4b4b50]',
      )}
    >
      <span
        className={cn(
          'absolute left-[2px] top-[2px] size-4 rounded-full bg-white shadow transition-transform',
          checked && 'translate-x-4',
        )}
      />
    </button>
  );
}
export function SettingRow({
  title,
  detail,
  children,
}: {
  title: string;
  detail?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex min-h-[54px] items-center justify-between gap-5 px-5 py-3">
      <div className="min-w-0">
        <p className="text-[13px] text-[#e5e5ea]">{title}</p>
        {detail && (
          <p className="mt-1 max-w-[390px] text-[11px] leading-[1.6] text-[#8e8e93]">
            {detail}
          </p>
        )}
      </div>
      {children}
    </div>
  );
}
export function SettingsGroup({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section>
      <h3 className="mb-2 px-1 text-[12px] font-medium text-[#a1a1a6]">
        {title}
      </h3>
      <div className="divide-y divide-white/[.06] overflow-hidden rounded-[14px] border border-white/[.07] bg-[#242426]">
        {children}
      </div>
    </section>
  );
}
export function PermissionControls({
  value,
  onChange,
  requested,
  disabled = false,
}: {
  value: RemotePermissions;
  onChange: (value: RemotePermissions) => void;
  requested?: RemotePermissions;
  disabled?: boolean;
}) {
  const labels: [keyof RemotePermissions, string][] = [
    ['screen', 'Voir l’écran'],
    ['keyboard', 'Utiliser le clavier'],
    ['mouse', 'Utiliser la souris'],
  ];
  return (
    <div className="divide-y divide-white/[.06]">
      {labels.map(([key, label]) => (
        <SettingRow key={key} title={label}>
          <Toggle
            label={label}
            checked={value[key]}
            disabled={disabled || (requested && !requested[key])}
            onChange={(next) => onChange({ ...value, [key]: next })}
          />
        </SettingRow>
      ))}
    </div>
  );
}
