<p align="center">
  <img src="public/icon.svg" width="128" height="128" alt="Rísta app icon" />
</p>

<h1 align="center">Rísta</h1>
<p align="center"><strong>A quiet place for your notes.</strong></p>
<p align="center">Native Markdown. Beautiful pages. Databases made from your own files.</p>
<p align="center">
  <a href="https://github.com/henrikogaard/rista/releases">Releases</a> ·
  <a href="docs/README.md">Documentation</a> ·
  <a href="DESIGN.md">Design system</a> ·
  <a href="LICENSE">MIT license</a>
</p>

---

Rísta is a local-first Markdown editor built in Rust with
[GPUI](https://gpui.rs) and [gpui-kit](https://gpui-kit.com).
Open a single note or a folder of notes. Write in source, read in preview,
or keep both side by side. Your work stays in ordinary Markdown files,
with images alongside them and properties in YAML frontmatter.

No account is required. There is no built-in cloud sync or telemetry.
Remote images and packaged update checks can still make network requests.

## Write, connect, organise

- **A native writing workspace.** Multiple tabs, a file tree, search,
  command palette, autosave, session restoration, and a distraction-free zen mode.
- **Pages with presence.** Headings, cover images, image embeds, callouts,
  editable properties, and tables in a live Markdown preview.
- **Connected notes.** Wikilinks and aliases, note transclusion, backlinks,
  outgoing links, tags, an outline, and local/global graph views. Dock the local
  graph above the Agent panel to keep note context visible while working.
- **Databases without a server.** `.base` YAML files query your notes and
  frontmatter. View the same notes as a table, cards, a board, or a calendar;
  add filters, formulas, relations, and rollups.
- **Everyday organisation.** Daily notes, templates, tasks, starred notes,
  local page history, and a recoverable vault trash.
- **Quiet by design.** Rísta Night and Rísta Day, compact controls, adjustable
  typography, and resizable panes.

### Exploring and writing

Filter the explorer by filename/path or file type; folders stay first. Expansion
is remembered per vault. **Files** offers a compact Name/Type/Modified/Size table
alongside the curated **Dashboard**. Drag files or folders to move them, then use
**Undo last move** if needed; existing destinations are never overwritten.
**⌘P** switches files/folders; breadcrumb menus browse siblings. Single-click
previews a file, while double-click or editing keeps its tab open.

The outline supports ATX and Setext headings and moves both source and preview
to the selected section. In Split mode, the link control in the status bar lets
the preview follow source scrolling at Markdown-block boundaries (not pixel-exact
bidirectional scrolling). A missing-links indicator opens the outgoing panel;
local note and image targets are checked, excluding code samples and external URLs.
Wikilink and image completion remain available with `[[` and `![[`.

Right-click a file/folder → **Tools here…** passes that selection to a confirmed
JSON command. Configure ACP agents in an extension manifest to use the native
Agent panel, which streams responses and reviews protocol-mediated file changes
before applying them. Agent processes are not sandboxed, and their own tools can
change files without that review. The terminal uses your existing shell
configuration, supports independent tabs, and has font/size/shell preferences.
See [extensions, agents, and terminal tools](docs/extensions.md) for capabilities
and limits.

## Get Rísta

The [latest release](https://github.com/henrikogaard/rista/releases/latest)
is an early **macOS 13+ / Apple Silicon** build. The v0.1.0 and v0.1.1 ZIP
packages are ad-hoc signed and not notarized.

Releases from v0.1.2 use Developer ID signing, notarization, and stapled ZIP/DMG
packages. The release workflow requires these checks before publishing.

**Updating older installs:** The accented bundle filename in v0.1.0–v0.1.2
can prevent Sparkle from recognizing the running app. An update may install
without restarting and report the old version. Quit and reopen if that happens.
Before your next update, quit the app and rename only its outer bundle to
`Rista.app` in Finder; leave its contents untouched. The published v0.1.1 →
v0.1.2 update successfully terminated and relaunched with that filename.
Packages built from this source use `Rista.app` while retaining **Rísta** as
the display name. Installing the current DMG instead also requires removing the
old accented-name copy after quitting it, so you do not keep launching it.

macOS is the primary development and tested platform. Linux and Windows are
targets, not verified distributions. See the
[roadmap and limitations](docs/PRODUCT-ROADMAP.md).

## Your first page

Choose **Open File** (`⌘O`) for a standalone document, or
**Open Folder** (`⌘⇧O`) for a vault: a normal folder of notes and attachments.

Folder names open live dashboards; the disclosure chevrons expand or collapse
the sidebar independently. Each dashboard lists its immediate folders, notes,
and databases. Enable **Show non-Markdown files** in Settings to include images
and other files, with file-type sorting, read-only text/image previews, and
default-app opening. macOS also offers Quick Look for supported formats such
as PDF. See [other-file previews](docs/folder-pages.md#other-files).
Use Home, breadcrumbs, or Back/Forward to navigate.
An optional `<folder>/<folder>.md` introduction (or `README.md` fallback) supports
Markdown and local images, including frontmatter covers. **Edit introduction**
opens the ordinary Markdown file; no separate dashboard format is created.

The titlebar inspector button shows or hides properties, outline, links, tasks,
calendar, and tags on the right. Settings preserve the inspector and active folder.
New navigation labels support English and Norwegian through **Navigation language**
in Settings; translation of older app-wide controls is not yet complete.

Single-click a file in the tree to browse in a reusable italic preview tab.
Double-click the file or tab to keep it; editing keeps it automatically.
Cmd-click opens a separate tab. Unsaved, conflicted, and pinned notes are never
replaced by browsing.

### Personalise notes and colours

- Use `:LiInbox:` or `:lucide-inbox:` in headings and prose for bundled note
  icons. Unknown icons stay readable as text; Markdown source stays unchanged.
- Settings → **Properties** controls whether frontmatter starts expanded,
  collapsed, or hidden. The inspector still provides editing when hidden.
- Choose independent light and dark palettes: Rísta, Fjord, or Rose. Each palette
  changes surfaces and accent colours together.
- **Create theme…** copies the active palette to an editable JSON file.
  Edit the colours, choose **Reload themes**, then select your palette.
  See [custom palette documentation](DESIGN.md#custom-palettes).
Finder’s **Open With → Rísta** also opens `.md` and `.markdown` files.

Use **Split** (`⌘2`) to see how a note renders. With `cover.jpg` next to
your note, try:

```markdown
---
title: Field notes
cover: cover.jpg
banner_y: 50
status: Draft
---

# A little room to think

Ideas become clearer when you write them down.

> [!note] Keep it simple
> Start with one page, then connect it to [[Tomorrow]].

![A detail worth remembering](cover.jpg)
```

For a small database, create `Notes.base` inside your vault:

```yaml
filters:
  and:
    - 'file.ext == "md"'
views:
  - type: table
    name: Notes
    order:
      - file.name
      - status
```

Open it from the file tree. Rows come from your notes; properties remain in
their frontmatter. `.base` implements a supported expression/view subset,
not a promise of complete compatibility with other tools.

## Keyboard essentials

| Action | macOS |
|---|---|
| New note / Open file / Open folder | `⌘N` / `⌘O` / `⌘⇧O` |
| Save / Save as | `⌘S` / `⌘⇧S` |
| Source / Split / Preview | `⌘1` / `⌘2` / `⌘3` |
| Find / Search vault | `⌘F` / `⌘⇧F` |
| Command palette | `⌘K` |
| Quick open file/folder | `⌘P` |
| Toggle sidebar / Zen mode | `⌘B` / `⌘⇧Enter` |
| Daily note / Graph | `⌘⇧D` / `⌘G` |
| Settings | `⌘,` |

## Build from source

On macOS, install a stable [Rust toolchain](https://rustup.rs) and Xcode
Command Line Tools (`xcode-select --install`), then:

```bash
git clone https://github.com/henrikogaard/rista.git
cd rista
cargo run
# Or open a file/folder at launch:
cargo run -- /absolute/path/to/notes
```

`cargo build --release` produces a binary, not an installable app bundle.
See the [release checklist](docs/RELEASE-CHECKLIST.md) for macOS packaging
and Sparkle signing. A normal `cargo run` does not need Sparkle.

Before submitting a change:

```bash
cargo fmt --check
cargo clippy
cargo test
```

Read [AGENTS.md](AGENTS.md) for code conventions and
[the architecture guide](docs/ARCHITECTURE.md) for the module map.
Use the [manual QA checklist](docs/TEST-CASES.md) for user-facing changes.

## Storage and safety

Notes, attachments, and `.base` definitions live in your own folder.
Vault history and trash live under `.rista/`. Settings and session state
live in `~/Library/Application Support/no.ogard.rista/settings.json` on macOS.

Rísta checks for external changes before saving and stops on conflicts rather
than silently overwriting them. History is not a backup: keep independent
backups, especially while using an early release.

## License

[MIT](LICENSE) · Copyright (c) 2026 Henrik Øgård.
Dependencies retain their respective licenses.
