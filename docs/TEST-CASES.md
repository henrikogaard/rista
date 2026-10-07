# Test cases

This is a manual regression checklist for native Rísta. Items are deliberately unchecked; this document does not claim that any case has passed. Use disposable copies of fixtures and preserve originals.

## Fixtures and settings

- [ ] Back up the active settings file before changing preferences: `~/Library/Application Support/no.ogard.rista/settings.json`.
- [ ] Prepare a disposable vault containing ordinary Markdown, frontmatter, Unicode filenames, duplicate note stems in different folders, wiki links, local images, `.base` files, and notes with relations/rollups.
- [ ] Record fixture file bytes before each save test so comparisons are exact, not visual-only.
- [ ] Change theme and source/split/preview preferences; restart and confirm they persist without modifying unrelated note files.

## Editing, saving, and document lifecycle

- [ ] Edit a standalone document and a vault document, save, and compare exact bytes with the expected output.
- [ ] Type and immediately close a tab, switch folders/vaults, or quit before the 800 ms autosave interval. Confirm dirty content is flushed before the lifecycle completes.
- [ ] Force a save failure during a dirty-document lifecycle; confirm closing or switching aborts rather than discarding dirty content.
- [ ] Externally edit a document after it is opened, including replacing bytes with different content of the same length. Confirm a later save rejects the conflict and preserves the external bytes.
- [ ] Delete an opened document externally. Confirm save rejects the missing file and does not recreate or overwrite it silently.
- [ ] Reopen duplicate stems with Unicode names in separate folders and confirm selection, tabs, and links resolve to the intended files.
- [ ] Change a standalone file externally. Confirm the 500 ms poll only invalidates after metadata changes and the changed content is noticed without an unconditional idle read/render loop.

## Native file opening and windows

- [ ] Open `.md` and `.markdown` files from Finder and with **⌘O**; confirm they open as standalone documents.
- [ ] Open a vault with **⌘⇧O** and confirm its Markdown files are indexed.
- [ ] Close the final window, then reopen from Finder and from the Dock. Confirm a workspace returns without an extra recovery click.
- [ ] Immediately use **⌘K** after each reopening; confirm keyboard focus works without clicking the editor first.
- [ ] With no windows left open, use **⌘Q** and confirm the process exits.

## Preview and vault regression

- [ ] Check headings, local images, cover/banner content, callouts, wiki links, note transclusion, and base embeds in preview.
- [ ] Check representative supported math notation; do not treat this as full TeX coverage.
- [ ] Check vault indexing and wikilink aliases after file create, rename, delete, and recreate operations.
- [ ] Check `.base` query/formula results in table, cards, gallery, kanban, board, and calendar views.
- [ ] Mutate a `.base` source row that is not selected in the current view; confirm derived views, relations, and rollups refresh correctly.
- [ ] Re-run preview and `.base` regression after changing a note's frontmatter properties.

## Themes, input, and accessibility

- [ ] Repeat representative editor, preview, vault, and `.base` cases in both dark and light themes.
- [ ] Check keyboard focus, common editing input, and assistive-technology labels on macOS.
- [ ] Verify platform-specific keyboard mappings separately on any Linux or Windows build; those mappings are not currently verified.

## Cleanup

- [ ] Restore settings and preferences exactly as found, including their absence on a clean machine.
- [ ] Remove only disposable fixtures and test app installations; restore any Launch Services registration changed for the test.
- [ ] Confirm no test process remains and no real note or default file-handler preference was changed.
