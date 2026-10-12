import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import {
  Extension,
  Node,
  mergeAttributes,
  type JSONContent,
} from '@tiptap/core';
import {
  EditorContent,
  NodeViewWrapper,
  ReactNodeViewRenderer,
  useEditor,
  type NodeViewProps,
} from '@tiptap/react';
import StarterKit from '@tiptap/starter-kit';
import TaskList from '@tiptap/extension-task-list';
import TaskItem from '@tiptap/extension-task-item';
import Placeholder from '@tiptap/extension-placeholder';
import { Plugin, type SelectionBookmark } from '@tiptap/pm/state';
import {
  Bold,
  Italic,
  Underline,
  Strikethrough,
  List,
  ListOrdered,
  ListTodo,
  Quote,
  Code2,
  Link,
  Undo2,
  Redo2,
  Plus,
  Table2,
  FilePlus2,
  Heading1,
  Heading2,
  Minus,
  Paperclip,
  Loader2,
} from 'lucide-react';
import {
  change,
  createDatabase,
  createPage,
  getPage,
  getWorkspace,
  id,
  noteIcon,
} from '@/lib/noosnote/model';
import type { NotePage } from '@/lib/noosnote/types';
import {
  transferFiles,
  bytesBase64,
  convertWord,
  documentChanges,
  isWord,
  MAX_ATTACHMENT_BYTES,
} from '@/lib/noosnote/files';
import { FileCard, FilePage, DocumentHeader } from './file-card';
import { useNoteContext } from './context';
import { PageContextMenu } from './page-menu';
const Database = lazy(() =>
  import('./database').then((module) => ({ default: module.Database })),
);

const BLOCKS = [
  'paragraph',
  'heading',
  'blockquote',
  'codeBlock',
  'bulletList',
  'orderedList',
  'listItem',
  'taskList',
  'taskItem',
  'horizontalRule',
  'noosDatabase',
  'noosPage',
];
const StableIds = Extension.create({
  name: 'stableIds',
  addGlobalAttributes() {
    return [
      { types: BLOCKS, attributes: { id: { default: null, rendered: false } } },
    ];
  },
  addProseMirrorPlugins() {
    return [
      new Plugin({
        appendTransaction(transactions, _old, state) {
          if (!transactions.some((transaction) => transaction.docChanged))
            return null;
          const transaction = state.tr;
          const seen = new Set<string>();
          state.doc.descendants((node, pos) => {
            if (!BLOCKS.includes(node.type.name)) return;
            const current = node.attrs.id as string | null;
            if (!current || seen.has(current))
              transaction.setNodeMarkup(pos, undefined, {
                ...node.attrs,
                id: id(),
              });
            else seen.add(current);
          });
          return transaction.docChanged ? transaction : null;
        },
      }),
    ];
  },
});

function DatabaseNode({ node }: NodeViewProps) {
  return (
    <NodeViewWrapper className="note-database-block" contentEditable={false}>
      <Suspense
        fallback={<div className="note-empty">Chargement de la table…</div>}
      >
        <Database databaseId={node.attrs.databaseId as string} />
      </Suspense>
    </NodeViewWrapper>
  );
}
function PageNode({ node }: NodeViewProps) {
  const { notes, workspaceId, openPage } = useNoteContext();
  const entry = getWorkspace(notes.files, workspaceId)?.pages.find(
    (page) => page.id === node.attrs.pageId,
  );
  if (entry?.file && !entry.trashedAt)
    return (
      <NodeViewWrapper className="note-file-node" contentEditable={false}>
        <PageContextMenu pageId={entry.id}>
          <FileCard entry={entry} />
        </PageContextMenu>
      </NodeViewWrapper>
    );
  return (
    <NodeViewWrapper contentEditable={false}>
      <PageContextMenu pageId={node.attrs.pageId as string}>
        <button
          className="note-page-link"
          disabled={!entry || !!entry.trashedAt}
          onClick={() => openPage(node.attrs.pageId as string)}
        >
          {noteIcon(entry) || '📄'} {entry?.title || 'Page indisponible'}
        </button>
      </PageContextMenu>
    </NodeViewWrapper>
  );
}
const DatabaseBlock = Node.create({
  name: 'noosDatabase',
  group: 'block',
  atom: true,
  draggable: true,
  addAttributes() {
    return { databaseId: { default: '' } };
  },
  parseHTML() {
    return [{ tag: 'div[data-noos-database]' }];
  },
  renderHTML({ HTMLAttributes }) {
    return [
      'div',
      mergeAttributes(HTMLAttributes, { 'data-noos-database': '' }),
    ];
  },
  addNodeView() {
    return ReactNodeViewRenderer(DatabaseNode);
  },
});
const PageBlock = Node.create({
  name: 'noosPage',
  group: 'block',
  atom: true,
  addAttributes() {
    return { pageId: { default: '' } };
  },
  parseHTML() {
    return [{ tag: 'div[data-noos-page]' }];
  },
  renderHTML({ HTMLAttributes }) {
    return ['div', mergeAttributes(HTMLAttributes, { 'data-noos-page': '' })];
  },
  addNodeView() {
    return ReactNodeViewRenderer(PageNode);
  },
});

export function NoteEditor({ page }: { page: NotePage }) {
  if (page.attachment && !isWord(page.attachment.name))
    return (
      <>
        <FilePage page={page} />
        <TextEditor page={page} />
      </>
    );
  return (
    <>
      {page.attachment && <DocumentHeader page={page} />}
      <TextEditor page={page} />
    </>
  );
}

function TextEditor({ page }: { page: NotePage }) {
  const { notes, workspaceId, openPage, report } = useNoteContext();
  const latest = useRef(notes);
  useEffect(() => {
    latest.current = notes;
  }, [notes]);
  const [insertOpen, setInsertOpen] = useState(false);
  const [linkOpen, setLinkOpen] = useState(false);
  const [linkValue, setLinkValue] = useState('');
  const fileInput = useRef<HTMLInputElement>(null);
  const importing = useRef(false);
  const importQueue = useRef<{ file: File; bookmark: SelectionBookmark }[]>([]);
  const alive = useRef(true);
  const [importingName, setImportingName] = useState('');
  const [dragging, setDragging] = useState(false);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const editor = useEditor(
    {
      immediatelyRender: false,
      extensions: [
        StarterKit.configure({
          heading: { levels: [1, 2, 3] },
          link: {
            openOnClick: false,
            protocols: ['https', 'http', 'mailto', 'tel'],
          },
        }),
        TaskList,
        TaskItem.configure({ nested: true }),
        Placeholder.configure({
          placeholder:
            'Écris ici, ou utilise + pour ajouter un fichier, une table…',
        }),
        StableIds,
        DatabaseBlock,
        PageBlock,
      ],
      content: page.content as JSONContent,
      editorProps: {
        attributes: {
          class: 'note-prose',
          'aria-label': 'Contenu de la page',
          spellcheck: 'true',
        },
        handleKeyDown(view, event) {
          if (
            event.key === '/' &&
            view.state.selection.$from.parent.textContent.length === 0
          ) {
            event.preventDefault();
            setInsertOpen(true);
            return true;
          }
          return false;
        },
      },
      onUpdate({ editor }) {
        const current = getPage(
          latest.current.filesRef.current.files,
          workspaceId,
          page.id,
        );
        if (current)
          latest.current.edit([
            change({ ...current, content: editor.getJSON() }),
          ]);
      },
    },
    [page.id],
  );
  useEffect(() => {
    if (
      !editor ||
      JSON.stringify(editor.getJSON()) === JSON.stringify(page.content)
    )
      return;
    const latestPage = getPage(
      latest.current.filesRef.current.files,
      workspaceId,
      page.id,
    );
    if (
      latestPage &&
      JSON.stringify(latestPage.content) !== JSON.stringify(page.content)
    )
      return;
    const { from, to } = editor.state.selection;
    editor.commands.setContent(page.content as JSONContent, {
      emitUpdate: false,
    });
    const end = editor.state.doc.content.size;
    editor.commands.setTextSelection({
      from: Math.min(from, end),
      to: Math.min(to, end),
    });
  }, [editor, page.content, page.id, workspaceId]);
  useEffect(() => {
    if (!editor) return;
    const map = ({
      transaction,
    }: {
      transaction: import('@tiptap/pm/state').Transaction;
    }) => {
      for (const item of importQueue.current)
        item.bookmark = item.bookmark.map(transaction.mapping);
    };
    editor.on('transaction', map);
    return () => {
      editor.off('transaction', map);
      importQueue.current = [];
    };
  }, [editor]);
  if (!editor) return <p className="note-muted">Ouverture de l’éditeur…</p>;
  const buttons = [
    {
      title: 'Gras',
      icon: Bold,
      active: 'bold',
      run: () => editor.chain().focus().toggleBold().run(),
    },
    {
      title: 'Italique',
      icon: Italic,
      active: 'italic',
      run: () => editor.chain().focus().toggleItalic().run(),
    },
    {
      title: 'Souligné',
      icon: Underline,
      active: 'underline',
      run: () => editor.chain().focus().toggleUnderline().run(),
    },
    {
      title: 'Barré',
      icon: Strikethrough,
      active: 'strike',
      run: () => editor.chain().focus().toggleStrike().run(),
    },
    {
      title: 'Titre',
      icon: Heading1,
      active: 'heading',
      run: () => editor.chain().focus().toggleHeading({ level: 1 }).run(),
    },
    {
      title: 'Sous-titre',
      icon: Heading2,
      active: '',
      run: () => editor.chain().focus().toggleHeading({ level: 2 }).run(),
    },
    {
      title: 'Liste',
      icon: List,
      active: 'bulletList',
      run: () => editor.chain().focus().toggleBulletList().run(),
    },
    {
      title: 'Liste numérotée',
      icon: ListOrdered,
      active: 'orderedList',
      run: () => editor.chain().focus().toggleOrderedList().run(),
    },
    {
      title: 'Liste de tâches',
      icon: ListTodo,
      active: 'taskList',
      run: () => editor.chain().focus().toggleTaskList().run(),
    },
    {
      title: 'Citation',
      icon: Quote,
      active: 'blockquote',
      run: () => editor.chain().focus().toggleBlockquote().run(),
    },
    {
      title: 'Code',
      icon: Code2,
      active: 'codeBlock',
      run: () => editor.chain().focus().toggleCodeBlock().run(),
    },
    {
      title: 'Séparateur',
      icon: Minus,
      active: '',
      run: () => editor.chain().focus().setHorizontalRule().run(),
    },
    {
      title: 'Lien',
      icon: Link,
      active: 'link',
      run: () => {
        setLinkValue(String(editor.getAttributes('link').href || ''));
        setLinkOpen(true);
      },
    },
    {
      title: 'Annuler',
      icon: Undo2,
      active: '',
      run: () => editor.chain().focus().undo().run(),
    },
    {
      title: 'Rétablir',
      icon: Redo2,
      active: '',
      run: () => editor.chain().focus().redo().run(),
    },
  ];
  async function importFiles(files: File[]) {
    if (!editor || editor.isDestroyed) return;
    const bookmark = editor.state.selection.getBookmark();
    importQueue.current.push(...files.map((file) => ({ file, bookmark })));
    setInsertOpen(false);
    setDragging(false);
    if (importing.current) return;
    importing.current = true;
    try {
      while (importQueue.current.length) {
        const item = importQueue.current[0];
        const { file } = item;
        try {
          if (!alive.current || editor!.isDestroyed) return;
          if (!file.size || file.size > MAX_ATTACHMENT_BYTES)
            throw new Error('Choisis un fichier de moins de 20 Mo.');
          setImportingName(file.name);
          const converted = isWord(file.name)
            ? await convertWord(file)
            : undefined;
          if (!alive.current || editor!.isDestroyed) return;
          const content = bytesBase64(new Uint8Array(await file.arrayBuffer()));
          const attachment = await window.noosphereDesktop!.noosnote.importFile(
            workspaceId,
            file.name,
            content,
          );
          if (!alive.current || editor!.isDestroyed) return;
          const imported = documentChanges(
            latest.current.filesRef.current.files,
            workspaceId,
            page.id,
            attachment,
            converted,
          );
          latest.current.edit(imported.changes);
          editor!
            .chain()
            .focus()
            .command(({ tr }) => {
              tr.setSelection(item.bookmark.resolve(tr.doc));
              return true;
            })
            .insertContent([
              {
                type: 'noosPage',
                attrs: { id: id(), pageId: imported.page.id },
              },
              { type: 'paragraph', attrs: { id: id() } },
            ])
            .run();
          await latest.current.flush();
        } catch (error) {
          if (alive.current) report(error);
        } finally {
          importQueue.current.shift();
        }
      }
    } finally {
      importing.current = false;
      if (alive.current) setImportingName('');
    }
  }
  function insert(kind: 'table' | 'page') {
    try {
      const files = latest.current.filesRef.current.files;
      if (kind === 'table') {
        const created = createDatabase(files, workspaceId, page.id);
        notes.edit(created.changes);
        editor!
          .chain()
          .focus()
          .insertContent([
            {
              type: 'noosDatabase',
              attrs: { id: id(), databaseId: created.database.id },
            },
            { type: 'paragraph', attrs: { id: id() } },
          ])
          .run();
      } else {
        const created = createPage(files, workspaceId, page.id);
        notes.edit(created.changes);
        editor!
          .chain()
          .focus()
          .insertContent([
            { type: 'noosPage', attrs: { id: id(), pageId: created.page.id } },
            { type: 'paragraph', attrs: { id: id() } },
          ])
          .run();
        openPage(created.page.id);
      }
      setInsertOpen(false);
    } catch (error) {
      report(error);
    }
  }
  return (
    <div
      className={`note-editor${dragging ? ' note-file-drop-active' : ''}`}
      onDragOver={(event) => {
        if (event.dataTransfer.types.includes('Files')) {
          event.preventDefault();
          event.dataTransfer.dropEffect = 'copy';
          setDragging(true);
        }
      }}
      onDragLeave={(event) => {
        if (
          !event.currentTarget.contains(
            event.relatedTarget as globalThis.Node | null,
          )
        )
          setDragging(false);
      }}
      onPasteCapture={(event) => {
        if (
          !(event.target instanceof HTMLElement) ||
          !event.target.closest('.note-prose')
        )
          return;
        const files = transferFiles(event.clipboardData);
        if (!files.length) return;
        event.preventDefault();
        event.stopPropagation();
        void importFiles(files);
      }}
      onDropCapture={(event) => {
        const files = transferFiles(event.dataTransfer);
        if (files.length) {
          event.preventDefault();
          event.stopPropagation();
          const position = editor.view.posAtCoords({
            left: event.clientX,
            top: event.clientY,
          });
          if (position) editor.commands.setTextSelection(position.pos);
          void importFiles(files);
        }
      }}
    >
      <input
        ref={fileInput}
        className="note-file-input"
        aria-label="Ajouter des fichiers à la page"
        type="file"
        multiple
        onChange={(event) => {
          void importFiles([...(event.target.files ?? [])]);
          event.target.value = '';
        }}
      />
      <div className="note-toolbar" role="toolbar" aria-label="Mise en forme">
        {buttons.map((button) => (
          <button
            key={button.title}
            type="button"
            title={button.title}
            aria-label={button.title}
            aria-pressed={!!button.active && editor.isActive(button.active)}
            onMouseDown={(event) => event.preventDefault()}
            onClick={button.run}
          >
            <button.icon size={16} />
          </button>
        ))}
        <button
          type="button"
          aria-label="Insérer un bloc"
          aria-expanded={insertOpen}
          onClick={() => setInsertOpen(!insertOpen)}
        >
          <Plus size={17} />
        </button>
      </div>
      {dragging && (
        <div className="note-file-drop-hint">
          <Paperclip size={22} /> Dépose le fichier dans cette page
        </div>
      )}
      {importingName && (
        <output className="note-file-import-status">
          <Loader2 size={16} className="animate-spin" /> Importation de{' '}
          {importingName}…
        </output>
      )}
      {insertOpen && (
        <div className="note-insert-menu">
          <button
            onClick={() => fileInput.current?.click()}
            disabled={!!importingName}
          >
            <Paperclip size={18} />
            <span>
              <strong>Fichier</strong>
              <small>Image, PDF, document Word ou pièce jointe</small>
            </span>
          </button>
          <button onClick={() => insert('table')}>
            <Table2 size={18} />
            <span>
              <strong>Table</strong>
              <small>Chaque élément ouvre une page</small>
            </span>
          </button>
          <button onClick={() => insert('page')}>
            <FilePlus2 size={18} />
            <span>
              <strong>Sous-page</strong>
              <small>Une page vierge dans cette page</small>
            </span>
          </button>
          <button onClick={() => setInsertOpen(false)}>Fermer</button>
        </div>
      )}
      {linkOpen && (
        <form
          className="note-inline-form"
          onSubmit={(event) => {
            event.preventDefault();
            const value = linkValue.trim();
            if (value && !/^(https?:\/\/|mailto:|tel:)/i.test(value)) {
              report(
                new Error(
                  'Utilise un lien https://, http://, mailto: ou tel:.',
                ),
              );
              return;
            }
            if (value) editor.chain().focus().setLink({ href: value }).run();
            else editor.chain().focus().unsetLink().run();
            setLinkOpen(false);
          }}
        >
          <input
            aria-label="Adresse du lien"
            placeholder="https://…"
            value={linkValue}
            onChange={(event) => setLinkValue(event.target.value)}
          />
          <button type="submit">Appliquer</button>
          <button type="button" onClick={() => setLinkOpen(false)}>
            Annuler
          </button>
        </form>
      )}
      <EditorContent editor={editor} />
    </div>
  );
}
