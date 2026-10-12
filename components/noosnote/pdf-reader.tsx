import { useEffect, useRef, useState } from 'react';
import {
  ChevronLeft,
  ChevronRight,
  Download,
  Loader2,
  Minus,
  Plus,
  RotateCw,
} from 'lucide-react';
import {
  GlobalWorkerOptions,
  getDocument,
  TextLayer,
  type PDFDocumentProxy,
} from 'pdfjs-dist';
import 'pdfjs-dist/web/pdf_viewer.css';
import { base64Bytes, saveBlob } from '@/lib/noosnote/files';
import type { NotePage } from '@/lib/noosnote/types';
import { useNoteContext } from './context';

GlobalWorkerOptions.workerSrc = new URL(
  '../../node_modules/pdfjs-dist/build/pdf.worker.min.mjs',
  import.meta.url,
).href;

export function PdfReader({ page }: { page: NotePage }) {
  const { workspaceId } = useNoteContext();
  const [pdf, setPdf] = useState<PDFDocumentProxy | null>(null);
  const [number, setNumber] = useState(1);
  const [zoom, setZoom] = useState(1);
  const [rotation, setRotation] = useState(0);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const [password, setPassword] = useState('');
  const [needsPassword, setNeedsPassword] = useState(false);
  const passwordCallback = useRef<((password: string) => void) | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const text = useRef<HTMLDivElement>(null);
  const holder = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(700);
  const bytes = useRef<Uint8Array | null>(null);
  useEffect(() => {
    let active = true;
    let destroy: (() => Promise<void>) | undefined;
    window
      .noosphereDesktop!.noosnote.readFile(workspaceId, page.id)
      .then(async (content) => {
        if (!active) return;
        bytes.current = base64Bytes(content);
        const base = import.meta.env.DEV
          ? '/node_modules/pdfjs-dist/'
          : '/pdf/';
        const task = getDocument({
          data: bytes.current.slice(),
          isEvalSupported: false,
          useSystemFonts: true,
          maxImageSize: 20_000_000,
          cMapUrl: `${base}cmaps/`,
          cMapPacked: true,
          standardFontDataUrl: `${base}standard_fonts/`,
          wasmUrl: `${base}wasm/`,
        });
        destroy = () => task.destroy();
        task.onPassword = (callback: (password: string) => void) => {
          if (active) {
            passwordCallback.current = callback;
            setNeedsPassword(true);
            setLoading(false);
          }
        };
        const document = await task.promise;
        if (active) {
          setPdf(document);
          setNeedsPassword(false);
        } else await task.destroy();
      })
      .catch((reason: unknown) => {
        if (active) {
          setError(reason instanceof Error ? reason.message : String(reason));
          setLoading(false);
        }
      });
    return () => {
      active = false;
      bytes.current = null;
      void destroy?.();
    };
  }, [workspaceId, page.id]);
  useEffect(() => {
    if (!holder.current) return;
    const observer = new ResizeObserver(([entry]) =>
      setWidth(Math.max(240, entry.contentRect.width - 24)),
    );
    observer.observe(holder.current);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (!pdf || !canvas.current || !text.current) return;
    let active = true;
    let cancel: (() => void) | undefined;
    let cancelText: (() => void) | undefined;
    pdf
      .getPage(number)
      .then(async (page) => {
        if (!active || !canvas.current || !text.current) return;
        setLoading(true);
        setError('');
        const angle = (page.rotate + rotation) % 360;
        const initial = page.getViewport({ scale: 1, rotation: angle });
        const viewport = page.getViewport({
          scale: Math.min(1.4, width / initial.width) * zoom,
          rotation: angle,
        });
        const ratio = Math.min(2, window.devicePixelRatio || 1);
        const surface = canvas.current;
        surface.width = Math.floor(viewport.width * ratio);
        surface.height = Math.floor(viewport.height * ratio);
        surface.style.width = `${viewport.width}px`;
        surface.style.height = `${viewport.height}px`;
        const context = surface.getContext('2d');
        if (!context) throw new Error('Impossible d’afficher le PDF.');
        const render = page.render({
          canvas: surface,
          canvasContext: context,
          viewport,
          transform: ratio === 1 ? undefined : [ratio, 0, 0, ratio, 0, 0],
        });
        cancel = () => render.cancel();
        text.current.replaceChildren();
        text.current.style.width = `${viewport.width}px`;
        text.current.style.height = `${viewport.height}px`;
        text.current.style.setProperty(
          '--scale-factor',
          String(viewport.scale),
        );
        const layer = new TextLayer({
          textContentSource: page.streamTextContent(),
          container: text.current,
          viewport,
        });
        cancelText = () => layer.cancel();
        await Promise.all([render.promise, layer.render()]);
        if (active) setLoading(false);
      })
      .catch((reason: unknown) => {
        if (active) {
          setError(reason instanceof Error ? reason.message : String(reason));
          setLoading(false);
        }
      });
    return () => {
      active = false;
      cancel?.();
      cancelText?.();
    };
  }, [pdf, number, zoom, rotation, width]);
  return (
    <section
      className="note-pdf-reader"
      aria-label={`Aperçu de ${page.attachment?.name ?? 'PDF'}`}
    >
      <div
        className="note-pdf-toolbar"
        role="toolbar"
        aria-label="Lecture du PDF"
      >
        <button
          aria-label="Page précédente"
          disabled={!pdf || number === 1}
          onClick={() => setNumber(number - 1)}
        >
          <ChevronLeft size={17} />
        </button>
        <label>
          <input
            aria-label="Page du PDF"
            type="number"
            min={1}
            max={pdf?.numPages ?? 1}
            value={number}
            onChange={(event) =>
              setNumber(
                Math.max(
                  1,
                  Math.min(pdf?.numPages ?? 1, Number(event.target.value) || 1),
                ),
              )
            }
          />{' '}
          / {pdf?.numPages ?? '…'}
        </label>
        <button
          aria-label="Page suivante"
          disabled={!pdf || number >= pdf.numPages}
          onClick={() => setNumber(number + 1)}
        >
          <ChevronRight size={17} />
        </button>
        <span className="note-pdf-toolbar-spacer" />
        <button
          aria-label="Réduire le PDF"
          disabled={zoom <= 0.5}
          onClick={() => setZoom(Math.max(0.5, zoom - 0.25))}
        >
          <Minus size={16} />
        </button>
        <span>{Math.round(zoom * 100)} %</span>
        <button
          aria-label="Agrandir le PDF"
          disabled={zoom >= 3}
          onClick={() => setZoom(Math.min(3, zoom + 0.25))}
        >
          <Plus size={16} />
        </button>
        <button
          aria-label="Tourner le PDF"
          onClick={() => setRotation((rotation + 90) % 360)}
        >
          <RotateCw size={16} />
        </button>
        <button
          aria-label="Télécharger le PDF"
          disabled={!pdf}
          onClick={() =>
            bytes.current &&
            saveBlob(
              page.attachment!.name,
              new Blob([bytes.current.slice().buffer], {
                type: 'application/pdf',
              }),
            )
          }
        >
          <Download size={16} />
        </button>
      </div>
      {needsPassword && (
        <form
          className="note-inline-form"
          onSubmit={(event) => {
            event.preventDefault();
            passwordCallback.current?.(password);
            setNeedsPassword(false);
            setLoading(true);
          }}
        >
          <input
            aria-label="Mot de passe du PDF"
            type="password"
            placeholder="Mot de passe du PDF"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
          />
          <button type="submit">Ouvrir</button>
        </form>
      )}
      {error && (
        <p className="note-file-error" role="alert">
          {error}
        </p>
      )}
      {loading && (
        <output className="note-muted note-pdf-loading">
          <Loader2 size={17} className="animate-spin" /> Chargement du PDF…
        </output>
      )}
      <div className="note-pdf-scroll" ref={holder}>
        <div className="note-pdf-sheet">
          <canvas ref={canvas} aria-label={`Page ${number}`} />
          <div ref={text} className="textLayer" />
        </div>
      </div>
    </section>
  );
}
