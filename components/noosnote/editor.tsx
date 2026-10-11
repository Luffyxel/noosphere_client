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
import { Plugin } from '@tiptap/pm/state';
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
  const { notes, workspaceId, openPage, report } = useNoteContext();
  const latest = useRef(notes);
  useEffect(() => {
    latest.current = notes;
  }, [notes]);
  const [insertOpen, setInsertOpen] = useState(false);
  const [linkOpen, setLinkOpen] = useState(false);
  const [linkValue, setLinkValue] = useState('');
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
          placeholder: 'Écris ici, ou utilise + pour ajouter une table…',
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
    const { from, to } = editor.state.selection;
    editor.commands.setContent(page.content as JSONContent, {
      emitUpdate: false,
    });
    const end = editor.state.doc.content.size;
    editor.commands.setTextSelection({
      from: Math.min(from, end),
      to: Math.min(to, end),
    });
  }, [editor, page.content]);
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
    <div className="note-editor">
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
      {insertOpen && (
        <div className="note-insert-menu">
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
