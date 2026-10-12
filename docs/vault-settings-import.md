# Vault settings import

The importer reads compatible preferences from the selected vault's `.obsidian` directory and shows a preview before applying them. It never writes to source configuration, notes, or templates, and does not run plugins, scripts, or network requests.

Choose **Import vault settings…** in the command palette to retry at any time. On first opening a vault with supported configuration, the app offers the same preview once for that canonical vault. Cancel dismisses that offer; it does not import anything. Confirming **Import** applies the plan and completes the first offer; the command remains available for later retries.

## Scope

Imported preferences are stored in the app's global Settings and apply across vaults. Shortcuts are global as well. The preview calls this out before confirmation. Bookmarks are merged into the app's bookmark list as absolute paths within the selected vault. Repeating an import does not duplicate existing bookmarks.

The importer reads these files when present:

- `.obsidian/daily-notes.json`
- `.obsidian/templates.json`
- `.obsidian/plugins/templater-obsidian/data.json`
- `.obsidian/plugins/periodic-notes/data.json`
- `.obsidian/app.json`
- `.obsidian/bookmarks.json`
- `.obsidian/hotkeys.json`
- `.obsidian/community-plugins.json`

Importable values include daily and periodic note folders/formats, selected period templates, the templates folder, folder templates, attachment folder, link format, literal and regular-expression ignore filters, compatible bookmarks, and supported shortcuts. Unsupported enabled plugin IDs are reported, not installed.

## Safety and limits

Source files are read-only. Each JSON file is limited to 1 MiB, total input to 4 MiB, and parsed collections and paths are bounded. Symlinked or escaping configuration paths are rejected. Missing optional settings are left unchanged; malformed, unsafe, or unsupported values are reported and do not replace existing values.

Template references must resolve to Markdown files within the selected vault. An enabled, valid template folder takes precedence over the ordinary templates folder. Current folder rules are imported only when the trigger mode is `folder`; `none` and regex-trigger rules are not imported. When the current mode is absent, the legacy enable boolean controls import. Script and executable template features are not evaluated. Expanded template output is limited to 4 MiB, including recursive note includes. Unsupported shortcut commands and alternatives are reported rather than executed or silently substituted.

Attachment folders are vault-relative by default. `.` selects the vault root; `./` and `./subfolder` are relative to the active note's folder and must remain inside the vault. Ignore filters support vault-relative files/folders and bounded regular expressions; expressions use Rust's regex syntax. If any filter is malformed or unsupported, the entire imported filter list is preserved unchanged and a warning is shown. An empty valid list clears the imported filters.

Link format supports shortest, relative, and vault-relative targets for generated wikilinks. Changing the format does not rewrite existing explicit links. Note rename rewrites links in the renamed note and links pointing to it; the existing tree-move path does not rewrite links. Relative links resolve from their originating note and are included in backlink analysis; copied links use vault-relative targets because their destination is unknown. Ordinary names continue to use the vault index.

Settings are saved before shortcuts are applied. These are separate stores, so a shortcut-save failure can leave successfully imported settings in place; the final report distinguishes that partial result. Changes to imported fields made after preview cause the import to stop rather than overwrite newer values.
