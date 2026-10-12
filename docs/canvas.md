# JSON Canvas

Files ending in `.canvas` open as a native board. The board keeps the original
JSON document as its source: opening and saving without an edit leaves the
original bytes intact, and fields the current app does not understand remain
in the document. A malformed file is shown as a non-destructive error rather
than replaced with an empty board.

## Board controls

- Use **Text**, **Link**, or **Group** to create cards. Dropping a file adds a
  file card only when the file is inside the open vault.
- Click a card to select it; Shift-click adds to the selection. Drag selected
  cards to move them and use the lower-right handle to resize. Escape cancels a
  staged gesture.
- Drag the small handle on a selected card to another card to create an edge.
  Click an edge to select it. Delete removes the current selection.
- Double-click a text card to edit its Markdown. Escape, Command-Enter, or
  leaving the editor finishes the edit without discarding it.
- Select one text card and choose **To note** to create a unique sibling
  Markdown file and replace the card with a file reference. Canvas Undo restores
  the text card; the created note is intentionally kept.
- Use Command-C/Command-V to copy and paste cards. Internal edges are copied
  with their endpoints remapped. File cards can only be pasted between boards
  in the same vault.
- Use Command-F to search text and labels on the current board. Enter advances
  to the next result; Shift-Enter moves to the previous result; Escape closes
  search.
- Space-drag or middle-drag pans. Pinch and Command-plus/minus zoom. **Fit all**
  frames the board and **Fit selection** frames the selected cards.
- Board Undo and Redo use the toolbar or keyboard shortcuts and the document's
  per-file Canvas history. Panning and zooming do not change the file.

Links opened from link cards are restricted to HTTP and HTTPS. File-card paths
are resolved relative to the vault and checked for containment before opening.
Unsupported node types remain in the JSON and display a neutral placeholder.

## Current limitations

The board currently renders and creates edges, but does not yet expose endpoint,
label, arrow, or color editing. Card color editing, alignment/distribution and
z-order commands are not available. File cards open the referenced file, but do
not yet render image/PDF previews or navigate to heading/block subpaths.
Automatic Canvas-reference retargeting during file moves, a New File type
chooser, and group background-image editing are not implemented.

File and directory move operations do not yet retarget Canvas references;
review those references after moving files.
