# Folder pages

Click a folder name to open its dashboard; use the disclosure arrow separately.
The page reads ordinary child folders, Markdown notes, `.base` databases, and
images. It refreshes when vault files change. Single-click previews a note;
double-click keeps its tab, and Cmd-click opens a separate tab.

**Customize page** opens `<folder-name>.md`, or `README.md` when that already
serves as the introduction. Edit its Markdown freely, including existing `.base`
embeds such as `![[Projects.base]]`. A leading H1 matching the page title is hidden
on the dashboard so the title is not repeated; the source remains unchanged.

```yaml
---
title: Studio
icon: LiPalette
description: Projects, references, and work in progress.
cover: attachments/studio.jpg
dashboard:
  layout: gallery
  sort: modified
  pinned:
    - Weekly plan.md
    - Ideas.md
  tag: ""
  status: ""
---
```

`layout` is `list`, `cards` (default), or `gallery`; `sort` is `name` (default) or
`modified`. The layout, sort, tag/status filters, and pin buttons save these
preferences into the introduction's frontmatter, creating the file if needed.
Search is temporary. Pinned notes keep their configured order regardless of
sorting. Pins refer to direct child Markdown notes by filename; they do not
modify sidebar stars or tab pins. Filters apply to all object groups, including
pinned notes, and are reset with **Clear filters**.

Cards use frontmatter `title`, `icon`, `description`, `cover`/`banner`, `tags`, and
`status`. A heading/filename and a short text excerpt supply useful defaults.
Tags may be a YAML list or a comma-separated string. Folder cards show a recursive
page count from the vault index. The live open-tasks section shows up to five
unchecked tasks below the folder and opens their source note. Database cards open
the existing `.base` renderer; there is no second database format.

An open introduction is edited through its existing buffer (undoable and subject
to normal autosave/conflict detection). Invalid frontmatter and known conflicts
block view saving rather than silently replacing the header. Disk writes use a
read/check/write guard, not atomic compare-and-swap; normal concurrent-writer
limitations still apply.
