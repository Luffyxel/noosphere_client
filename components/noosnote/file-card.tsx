import { lazy, Suspense, useState } from 'react';
import { Download, FileText, File, Loader2, X } from 'lucide-react';
import type { NotePage, PageEntry } from '@/lib/noosnote/types';
import {
  base64Bytes,
  fileSize,
  isImage,
  isWord,
  saveBlob,
} from '@/lib/noosnote/files';
import { getPage } from '@/lib/noosnote/model';
import { ImagePreview } from './image-preview';
import { useNoteContext } from './context';
const PdfReader = lazy(() =>
  import('./pdf-reader').then((module) => ({ default: module.PdfReader })),
);

export function FileCard({ entry }: { entry: PageEntry }) {
  const { notes, workspaceId, openPage, report } = useNoteContext();
  const [preview, setPreview] = useState<NotePage | null>(null);
  const [loading, setLoading] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const file = entry.file!;
  const word = isWord(file.name);
  const pdf = file.mediaType === 'application/pdf';
  async function open() {
    if (!pdf) {
      openPage(entry.id);
      return;
    }
    if (preview) {
      setPreview(null);
      return;
    }
    setLoading(true);
    try {
      const page = await notes.loadPage(workspaceId, entry.id);
      if (page) setPreview(page);
    } catch (error) {
      report(error);
    } finally {
      setLoading(false);
    }
  }
  async function download() {
    setDownloading(true);
    try {
      const content = await window.noosphereDesktop!.noosnote.readFile(
        workspaceId,
        entry.id,
      );
      saveBlob(
        file.name,
        new Blob([base64Bytes(content).buffer as ArrayBuffer], {
          type: file.mediaType,
        }),
      );
    } catch (error) {
      report(error);
    } finally {
      setDownloading(false);
    }
  }
  return (
    <div className="note-file-block">
      <div className="note-file-card">
        <button
          className={`note-file-main ${word ? 'note-file-word' : pdf ? 'note-file-pdf' : ''}`}
          onClick={() => void open()}
          aria-label={`${word ? 'Modifier' : 'Ouvrir'} ${file.name}`}
        >
          <span className="note-file-icon">
            {word ? <FileText size={25} /> : <File size={25} />}
          </span>
          <span>
            <strong>{entry.title || file.name}</strong>
            <small>
              {word
                ? 'Document Word · Modifiable'
                : pdf
                  ? 'PDF'
                  : file.name.split('.').pop()?.toUpperCase()}{' '}
              · {fileSize(file.size)}
            </small>
          </span>
        </button>
        <button
          className="note-file-action"
          onClick={() => void open()}
          disabled={loading}
        >
          {loading ? (
            <Loader2 size={15} className="animate-spin" />
          ) : preview ? (
            <X size={15} />
          ) : null}
          {word ? 'Modifier' : preview ? 'Fermer' : pdf ? 'Aperçu' : 'Ouvrir'}
        </button>
        <button
          className="note-file-action"
          aria-label={`Télécharger ${file.name}`}
          onClick={() => void download()}
          disabled={downloading}
        >
          {downloading ? (
            <Loader2 size={16} className="animate-spin" />
          ) : (
            <Download size={16} />
          )}
        </button>
      </div>
      {isImage(file) && (
        <ImagePreview
          key={entry.id}
          pageId={entry.id}
          name={file.name}
          onOpen={() => openPage(entry.id)}
        />
      )}
      {preview && (
        <Suspense fallback={<p className="note-muted">Ouverture du PDF…</p>}>
          <PdfReader page={preview} />
        </Suspense>
      )}
    </div>
  );
}

export function DocumentHeader({ page }: { page: NotePage }) {
  const { notes, workspaceId, report } = useNoteContext();
  const [busy, setBusy] = useState(false);
  const word = isWord(page.attachment!.name);
  async function download(edited: boolean) {
    setBusy(true);
    try {
      await notes.flush();
      if (edited) {
        const { wordBlob } = await import('@/lib/noosnote/export-word');
        const blob = await wordBlob(
          getPage(notes.filesRef.current.files, workspaceId, page.id) ?? page,
          notes.filesRef.current.files,
        );
        saveBlob(`${page.title || 'Document'}.docx`, blob);
      } else {
        const content = await window.noosphereDesktop!.noosnote.readFile(
          workspaceId,
          page.id,
        );
        saveBlob(
          page.attachment!.name,
          new Blob([base64Bytes(content).buffer as ArrayBuffer], {
            type: page.attachment!.mediaType,
          }),
        );
      }
    } catch (error) {
      report(error);
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="note-document-header">
      <div>
        <FileText size={18} />
        <span>
          {word
            ? 'Document Word · Enregistrement automatique'
            : page.attachment!.name}
        </span>
      </div>
      <div>
        {word && (
          <button onClick={() => void download(true)} disabled={busy}>
            <Download size={15} /> Exporter en Word
          </button>
        )}
        <button onClick={() => void download(false)} disabled={busy}>
          Original
        </button>
      </div>
      {page.importNotice && <p>{page.importNotice}</p>}
    </div>
  );
}

export function FilePage({ page }: { page: NotePage }) {
  return (
    <>
      <DocumentHeader page={page} />
      {page.attachment && isImage(page.attachment) && (
        <ImagePreview
          key={page.id}
          pageId={page.id}
          name={page.attachment.name}
        />
      )}
      {page.attachment?.mediaType === 'application/pdf' && (
        <Suspense fallback={<p className="note-muted">Ouverture du PDF…</p>}>
          <PdfReader page={page} />
        </Suspense>
      )}
    </>
  );
}
