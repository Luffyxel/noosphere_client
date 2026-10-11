import { useMemo, useState } from 'react';
import data from '@/lib/noosnote/emoji-data.json';

type Emoji = {
  unicode: string;
  label: string;
  group?: number;
  order?: number;
  tags?: string[];
  skins?: Emoji[];
};
const groups = [
  { id: 0, label: 'Visages', icon: '😀' },
  { id: 1, label: 'Personnes', icon: '👋' },
  { id: 3, label: 'Nature', icon: '🌿' },
  { id: 4, label: 'Cuisine', icon: '🍎' },
  { id: 5, label: 'Voyages', icon: '🚀' },
  { id: 6, label: 'Activités', icon: '⚽' },
  { id: 7, label: 'Objets', icon: '💡' },
  { id: 8, label: 'Symboles', icon: '❤️' },
  { id: 9, label: 'Drapeaux', icon: '🏳️' },
];
const catalog = data as Emoji[];
const normalize = (text: string) =>
  text.toLocaleLowerCase('fr').normalize('NFD').replace(/\p{M}/gu, '');

export function EmojiCatalog({
  value,
  onSelect,
  clearable,
}: {
  value: string;
  onSelect: (value: string) => void;
  clearable: boolean;
}) {
  const [query, setQuery] = useState('');
  const [group, setGroup] = useState<number | null>(null);
  const [tone, setTone] = useState(-1);
  const [limit, setLimit] = useState(120);
  const emojis = useMemo(() => {
    const words = normalize(query).trim().split(/\s+/).filter(Boolean);
    return catalog
      .filter(
        (emoji) =>
          (group === null || emoji.group === group) &&
          words.every((word) =>
            normalize(
              `${emoji.label} ${emoji.tags?.join(' ') ?? ''} ${emoji.unicode}`,
            ).includes(word),
          ),
      )
      .map((emoji) => (tone >= 0 ? (emoji.skins?.[tone] ?? emoji) : emoji));
  }, [query, group, tone]);
  return (
    <>
      <input
        className="note-emoji-search"
        aria-label="Rechercher un emoji"
        placeholder="Rechercher un emoji…"
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
          setLimit(120);
        }}
      />
      <fieldset
        className="note-emoji-categories"
        aria-label="Catégories d’emojis"
      >
        <button
          type="button"
          aria-pressed={group === null}
          onClick={() => {
            setGroup(null);
            setLimit(120);
          }}
        >
          Tous
        </button>
        {groups.map((entry) => (
          <button
            key={entry.id}
            type="button"
            className="note-system-emoji"
            title={entry.label}
            aria-label={entry.label}
            aria-pressed={group === entry.id}
            onClick={() => {
              setGroup(entry.id);
              setLimit(120);
            }}
          >
            {entry.icon}
          </button>
        ))}
      </fieldset>
      <label className="note-emoji-tone">
        Teinte
        <select
          aria-label="Teinte de peau"
          value={tone}
          onChange={(event) => setTone(Number(event.target.value))}
        >
          <option value={-1}>Par défaut</option>
          <option value={0}>Claire</option>
          <option value={1}>Moyennement claire</option>
          <option value={2}>Légèrement mate</option>
          <option value={3}>Mate</option>
          <option value={4}>Foncée</option>
        </select>
      </label>
      <div className="note-emoji-results">
        <div className="note-emoji-grid">
          {emojis.slice(0, limit).map((emoji) => (
            <button
              key={emoji.unicode}
              type="button"
              className="note-system-emoji"
              aria-label={emoji.label}
              title={emoji.label}
              aria-pressed={value === emoji.unicode}
              onClick={() => onSelect(emoji.unicode)}
            >
              {emoji.unicode}
            </button>
          ))}
        </div>
        {!emojis.length && <p>Aucun emoji trouvé.</p>}
        {emojis.length > limit && (
          <button
            type="button"
            className="note-emoji-more"
            onClick={() => setLimit(limit + 120)}
          >
            Afficher plus d’emojis
          </button>
        )}
      </div>
      <label className="note-emoji-paste">
        Ou colle un emoji du système
        <input
          aria-label="Emoji du système"
          placeholder="😊"
          maxLength={32}
          onChange={(event) => {
            const text = event.target.value.trim();
            const segments = [
              ...new Intl.Segmenter('fr', { granularity: 'grapheme' }).segment(
                text,
              ),
            ];
            if (
              segments.length === 1 &&
              /\p{Extended_Pictographic}|\p{Regional_Indicator}|\u20e3/u.test(
                text,
              )
            )
              onSelect(text);
          }}
        />
      </label>
      {clearable && (
        <button
          type="button"
          className="note-emoji-clear"
          onClick={() => onSelect('')}
        >
          Retirer l’icône
        </button>
      )}
    </>
  );
}
