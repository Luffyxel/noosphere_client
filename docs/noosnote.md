# Noosnote

Use **+ → Noosnote** in the left sidebar to create a workspace. Workspaces
contain rich text pages and databases whose rows open their own blank pages.
Pages can contain further databases and subpages. The School template starts
with a Courses category and English and French pages.

Database properties include text, numbers, categories, tags, dates, checkboxes,
URLs, e-mail, phone numbers, relations, formulas and rollups. Saved views support
tables, boards, lists, galleries and calendars, with grouping, filtering,
sorting and property visibility. Tables export to CSV and pages to JSON.

Right-click pages, search results or database items to open, rename, change
the icon, add a subpage, duplicate, move, export or send them to the trash.
**Shift+F10** also opens the menu. Database rows stay in their database;
their categories can be changed through properties or board columns.

Icons use a French searchable emoji catalog with categories and skin tones,
rendered with the system font. Emojis from the desktop picker can also be
pasted. Colors include an extended palette, a native picker and custom hex codes.

Storage format version 1 is independent of the application release. Notes
created with 0.2.0 remain readable and editable without recreating the
repository. Extended emoji sequences keep a fallback icon compatible with 0.2.0.

## Private repository

All workspaces share `noosnote_<GitHub account ID>`, a private repository in the
signed-in account. Account IDs and document UUIDs keep repository and file names
stable when titles or usernames change. Other computers discover the same
repository at sign-in. Chat identity keys are not involved in note sync.

First use requests consent to create and authorize the private repository.
If the GitHub App token cannot create it, the setup opens the prefilled GitHub
creation page and the app installation settings. Keep **Only select repositories**
and add the notes repository alongside the public messaging repository, then
return to Noosphere and verify. Automatic creation requires the GitHub App
`repository_creation: write` permission; repository administration is not required.

Notes are readable JSON in the private repository. GitHub and anyone granted
repository access can read them. Messaging end-to-end encryption is separate.
Local drafts and recovery copies use the application's protected storage.

```text
.noosnote.json
notes/<workspace UUID>/workspace.json
notes/<workspace UUID>/pages/<page UUID>.json
notes/<workspace UUID>/databases/<database UUID>.json
```

The owner marker binds the repository to its GitHub account. Workspace files
index page titles, icons and parents. Pages hold structured editor documents;
database rows reference page UUIDs. Renaming and moving pages preserve paths.

## Sync and recovery

Local checkpoints follow a 400 ms typing pause, capped at 1.8 seconds. Automatic
publishes are grouped and spaced by at least a minute. **Ctrl+S**, **Cmd+S** or
the sync button publishes immediately. Cached pages and drafts remain available
offline; unopened pages are loaded on demand. Search covers all page titles and
the text of loaded pages.

Git object IDs identify changed files. Metadata and opened pages are fetched in
bounded batches. Pending files are published through one Git tree and commit;
the branch update never forces a competing commit. Independent edits merge by
stable IDs and fields. Overlapping edits retain both versions for explicit
resolution, with an export and a protected recovery copy. Local checkpoints
also carry their editing base so typing during a pull preserves received edits.

Trash is reversible. The current format supports 4,096 pages per workspace,
5,000 rows, 64 properties and 24 views per database, with a 1 MiB document limit.
Views display rows in batches of 100. Memory retains 24 recently opened pages
and pending drafts; downloaded pages remain in the protected disk cache.

For the full workflow and formula examples, see the
[French guide](noosnote.fr.md).
