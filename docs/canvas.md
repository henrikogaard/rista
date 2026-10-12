# JSON Canvas

Files ending in `.canvas` open as a native, editable board. The document keeps
the original JSON as its source: opening and saving without a content change
preserves the original bytes. Unknown root, node, and edge fields, future node
types, numeric values, and array order survive edits. Malformed or unsupported
data is reported without replacing the file; unknown node types display as
read-only placeholders when their geometry is usable.

Canvas content participates in the ordinary document save, dirty, conflict,
autosave, and tab lifecycle. Board Undo and Redo use bounded per-document
Canvas snapshots. Viewport and selection changes do not alter the file.
Save As and file renames cannot convert between Canvas and text documents;
choose a destination with the same document kind. Directory names ending in
`.canvas` do not change directory rename behavior.
Note-formatting, template, and property commands do not modify Canvas JSON.

## Board controls

- Use **Add** to create text, file, link, or group cards. The background menu
  offers the same creation actions. Use the file picker or drop a file from the
  open vault to create a file card; files outside the vault are rejected.
- Click to select a card or edge; Shift-click extends the selection. Drag on
  empty board space to marquee-select. Command-A selects all cards. Drag cards
  to move, use the lower-right handle to resize, and hold Option while dragging to
  bypass snapping. Escape cancels a staged gesture.
- Double-click a text card to edit its Markdown in place. Changes are saved
  through the document; Escape, Command-Enter, or leaving the editor ends the
  coalesced board edit without discarding typed text.
- Drag the small handle on a selected card to another card to create an edge.
  Select a card or edge and choose **Edit selection** to change available
  fields, including link URLs, group labels and backgrounds, colors, edge
  endpoints and sides, arrows, labels, and curve style. Colors accept palette
  values `1`–`6` or custom hexadecimal values.
- Use the selection menu to duplicate, align or distribute cards, change
  z-order, or convert a text card to a note. Group selection includes contained
  cards for move, copy, and duplicate; deleting only a group leaves its cards.
  Conversion creates a unique sibling note. Undo restores the text card but
  intentionally leaves the created note in place.
- Command-C/Command-X/Command-V copy, cut, and paste selected cards. Internal
  edges are copied with remapped endpoints and fresh IDs. Pasted fragments are
  validated before mutation; file and background references must stay inside
  the destination vault.
- Command-F searches parsed card text, labels, URLs, and file names on the
  current board. Enter advances; Shift-Enter moves backward; Escape closes
  search. Hidden JSON geometry and unknown metadata are not indexed.
- Space-drag or middle-drag pans. Pinch and Command-plus/minus zoom. **Fit**
  frames the board; **More → Fit selection** frames selected cards.

Text cards and Markdown file-card excerpts use the existing Markdown preview
pipeline. File cards show supported image and PDF previews, note excerpts, and
heading or block subpaths; unavailable and unsupported files retain an
actionable title/open fallback. Opening a file card honors Markdown
heading/block subpaths and PDF page subpaths. Link cards open only validated HTTP(S) URLs.
Group backgrounds support cover, aspect-preserving fit, and repetition. Extremely
dense repeated backgrounds fall back to cover when more than 4,096 visible tiles
would be needed.

Parsed Canvas file/background references and text-node Markdown links
participate in outgoing links, backlinks, and the graph. File references are
vault-root-relative; text-node links follow the Canvas document's Markdown
directory. File and directory rename/move, including move Undo, structurally
retarget references. Conflicts are preflighted; if a concurrent writer changes
a closed Canvas during the subsequent guarded write, its data is preserved and
the operation reports an incomplete reference update rather than overwriting
the external change.

**New File** defaults to creating a note; choose **Canvas** for a board. The
dedicated **New Canvas** command remains available from the File menu, palette,
and vault tree.

## Deferred scope

Read-only `![[Board.canvas]]` embedding remains deferred. Multi-file reference
updates are guarded per file, not an atomic filesystem transaction; a later
race may leave some references updated and others unchanged, and is reported
as such.
