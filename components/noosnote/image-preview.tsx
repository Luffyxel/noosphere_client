import { useEffect, useRef, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { base64Bytes, imageMediaType } from '@/lib/noosnote/files';
import { useNoteContext } from './context';

export function ImagePreview({
  pageId,
  name,
  onOpen,
}: {
  pageId: string;
  name: string;
  onOpen?: () => void;
}) {
  const { notes, workspaceId } = useNoteContext();
  const holder = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const [source, setSource] = useState('');
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: '200px' },
    );
    if (holder.current) observer.observe(holder.current);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (!visible) return;
    let active = true;
    let url = '';
    void notes
      .api!.readFile(workspaceId, pageId)
      .then((content) => {
        if (!active) return;
        const bytes = base64Bytes(content);
        const type = imageMediaType(bytes);
        if (!type)
          throw new Error('Ce fichier ne contient pas une image reconnue.');
        url = URL.createObjectURL(
          new Blob([bytes.buffer as ArrayBuffer], { type }),
        );
        setSource(url);
      })
      .catch((reason) => {
        if (active)
          setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => {
      active = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [visible, workspaceId, pageId, attempt, notes.api]);
  const picture = source && (
    <img
      src={source}
      alt={name}
      draggable={false}
      onError={() =>
        setError(
          'Impossible d’afficher cette image. Le fichier original reste disponible.',
        )
      }
    />
  );
  return (
    <div ref={holder} className="note-image-preview">
      {error ? (
        <div className="note-file-error" role="alert">
          <p>{error}</p>
          <button
            onClick={() => {
              setSource('');
              setError('');
              setAttempt((value) => value + 1);
            }}
          >
            Réessayer
          </button>
        </div>
      ) : picture ? (
        onOpen ? (
          <button
            className="note-image-open"
            onClick={onOpen}
            aria-label={`Agrandir ${name}`}
          >
            {picture}
          </button>
        ) : (
          picture
        )
      ) : (
        <span className="note-muted">
          <Loader2 size={18} className="animate-spin" /> Chargement de l’image…
        </span>
      )}
    </div>
  );
}
