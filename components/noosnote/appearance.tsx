import { lazy, Suspense, useState } from 'react';
import { Popover } from '@base-ui/react/popover';
import { Check, ChevronDown } from 'lucide-react';
import { NOTE_COLORS } from '@/lib/noosnote/model';

const EmojiCatalog = lazy(() =>
  import('./emoji-catalog').then((module) => ({
    default: module.EmojiCatalog,
  })),
);

export function EmojiPicker({
  value,
  onChange,
  label,
  clearable = false,
}: {
  value: string;
  onChange: (value: string) => void;
  label: string;
  clearable?: boolean;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger
        className="note-emoji-trigger"
        aria-label={label}
        title={label}
      >
        <span className="note-system-emoji">{value || '📄'}</span>
        <ChevronDown size={13} />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner className="note-picker-positioner" sideOffset={8}>
          <Popover.Popup className="note-emoji-picker" aria-label={label}>
            <Suspense fallback={<p>Chargement des emojis…</p>}>
              <EmojiCatalog
                value={value}
                clearable={clearable}
                onSelect={(emoji) => {
                  onChange(emoji);
                  setOpen(false);
                }}
              />
            </Suspense>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}

export function ColorPicker({
  value,
  onChange,
  label = 'Couleur du Noosnote',
}: {
  value: string;
  onChange: (value: string) => void;
  label?: string;
}) {
  const [custom, setCustom] = useState(value);
  function select(color: string) {
    setCustom(color);
    onChange(color);
  }
  return (
    <div className="note-color-picker">
      <fieldset className="note-color-grid" aria-label="Palette de couleurs">
        {NOTE_COLORS.map((color) => (
          <button
            key={color}
            type="button"
            aria-label={`Couleur ${color}`}
            aria-pressed={value.toLowerCase() === color}
            style={{ background: color }}
            onClick={() => select(color)}
          >
            {value.toLowerCase() === color && <Check size={13} />}
          </button>
        ))}
      </fieldset>
      <div className="note-custom-color">
        <input
          type="color"
          aria-label={label}
          value={value}
          onChange={(event) => select(event.target.value)}
        />
        <label>
          Couleur personnalisée
          <input
            aria-label="Code couleur hexadécimal"
            value={custom}
            maxLength={7}
            placeholder="#78b89b"
            spellCheck={false}
            onChange={(event) => {
              const color = event.target.value;
              setCustom(color);
              if (/^#[0-9a-f]{6}$/i.test(color)) onChange(color.toLowerCase());
            }}
            onBlur={() => setCustom(value)}
          />
        </label>
      </div>
    </div>
  );
}
