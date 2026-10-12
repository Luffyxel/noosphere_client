import {
  Document,
  ExternalHyperlink,
  HeadingLevel,
  Packer,
  Paragraph,
  Table,
  TableCell,
  TableRow,
  TextRun,
  WidthType,
  type ParagraphChild,
} from 'docx';
import type { NoteFiles, NotePage, RichNode } from './types';
import { activeRows, getDatabase, getWorkspace } from './model';
import { createCellReader } from './data';

export async function wordBlob(
  page: NotePage,
  files: NoteFiles,
): Promise<Blob> {
  const reader = createCellReader(files);
  const runs = (node: RichNode): ParagraphChild[] => {
    if (node.type === 'hardBreak') return [new TextRun({ break: 1 })];
    if (node.type !== 'text') return (node.content ?? []).flatMap(runs);
    const marks = node.marks ?? [];
    const run = new TextRun({
      text: node.text ?? '',
      bold: marks.some((mark) => mark.type === 'bold'),
      italics: marks.some((mark) => mark.type === 'italic'),
      strike: marks.some((mark) => mark.type === 'strike'),
      ...(marks.some((mark) => mark.type === 'underline')
        ? { underline: {} }
        : {}),
      ...(marks.some((mark) => mark.type === 'code')
        ? { font: 'Consolas' }
        : {}),
    });
    const href = marks.find((mark) => mark.type === 'link')?.attrs?.href;
    return typeof href === 'string' && /^(https?:\/\/|mailto:|tel:)/i.test(href)
      ? [new ExternalHyperlink({ link: href, children: [run] })]
      : [run];
  };
  const blocks = (
    node: RichNode,
    level = 0,
    list?: 'bullet' | 'number',
  ): (Paragraph | Table)[] => {
    if (node.type === 'noosDatabase') {
      const database = getDatabase(
        files,
        page.workspaceId,
        String(node.attrs?.databaseId),
      );
      if (!database) return [new Paragraph({ text: 'Tableau indisponible' })];
      const pages = getWorkspace(files, page.workspaceId)?.pages ?? [];
      const values = [
        database.columns.map((column) => column.name),
        ...activeRows(files, database).map((row) =>
          database.columns.map((column) => {
            if (column.type === 'title')
              return (
                pages.find((entry) => entry.id === row.pageId)?.title ?? ''
              );
            const cell = reader.read(database, row, column);
            if (cell.error) return cell.error;
            const value = reader.label(column, cell.value);
            return Array.isArray(value)
              ? value.join(', ')
              : typeof value === 'boolean'
                ? value
                  ? 'Oui'
                  : 'Non'
                : String(value ?? '');
          }),
        ),
      ];
      return [
        new Paragraph({ text: database.title }),
        new Table({
          width: { size: 100, type: WidthType.PERCENTAGE },
          rows: values.map(
            (row, index) =>
              new TableRow({
                ...(index === 0 ? { tableHeader: true } : {}),
                children: row.map(
                  (text) =>
                    new TableCell({
                      children: [
                        new Paragraph({
                          children: [new TextRun({ text, bold: index === 0 })],
                        }),
                      ],
                    }),
                ),
              }),
          ),
        }),
      ];
    }
    if (node.type === 'noosPage') {
      const child = getWorkspace(files, page.workspaceId)?.pages.find(
        (entry) => entry.id === node.attrs?.pageId,
      );
      return [new Paragraph({ text: child?.title ?? 'Page liée' })];
    }
    if (['doc', 'blockquote', 'listItem', 'taskItem'].includes(node.type))
      return (node.content ?? []).flatMap((child) =>
        blocks(child, level, list),
      );
    if (['bulletList', 'orderedList', 'taskList'].includes(node.type))
      return (node.content ?? []).flatMap((child) =>
        blocks(
          child,
          level + 1,
          node.type === 'orderedList' ? 'number' : 'bullet',
        ),
      );
    if (node.type === 'horizontalRule')
      return [new Paragraph({ text: '────────────────' })];
    const headings = [
      HeadingLevel.HEADING_1,
      HeadingLevel.HEADING_2,
      HeadingLevel.HEADING_3,
    ];
    return [
      new Paragraph({
        children: runs(node),
        ...(node.type === 'heading'
          ? {
              heading:
                headings[
                  Math.min(2, Math.max(0, Number(node.attrs?.level ?? 1) - 1))
                ],
            }
          : {}),
        ...(list === 'bullet'
          ? { bullet: { level: Math.min(8, Math.max(0, level - 1)) } }
          : {}),
        ...(list === 'number'
          ? {
              numbering: {
                reference: 'list',
                level: Math.min(8, Math.max(0, level - 1)),
              },
            }
          : {}),
        spacing: { after: 120 },
      }),
    ];
  };
  const document = new Document({
    title: page.title,
    creator: 'Noosphere',
    numbering: {
      config: [
        {
          reference: 'list',
          levels: Array.from({ length: 9 }, (_, level) => ({
            level,
            format: 'decimal',
            text: `%${level + 1}.`,
            alignment: 'start',
          })),
        },
      ],
    },
    sections: [{ children: blocks(page.content) }],
  });
  return Packer.toBlob(document);
}
