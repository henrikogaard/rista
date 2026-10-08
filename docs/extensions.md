# Local extensions and terminal tools (API v1)

Rísta reads declarative JSON extensions without recompiling. This first API adds
commands to **Tools** and the command palette. Each command launches an installed
executable in the native PTY terminal. It is not a JavaScript/WASM runtime or a
sandbox, and does not yet inject arbitrary native UI, intercept editor events,
or provide a unified AI chat protocol.

Use an AI coding agent to create a manifest, inspect it, then choose **Tools →
Reload**. No process runs on discovery, reload, startup, or vault opening.
The confirmation displays the actual executable, argument array, and working
directory. Commands launch directly with an argument vector, not through `sh -c`.

## Locations

- User scope: `~/Library/Application Support/no.ogard.rista/plugins/*.json` on macOS.
  **Open extensions folder** locates the equivalent config directory on other OSes.
- Vault scope: `<vault>/.rista/plugins/*.json`.

Only direct JSON files are loaded; symlinks, invalid manifests, unknown fields,
unsupported API versions, and duplicate plugin IDs are rejected. User extensions
load first; a vault cannot replace one by reusing its ID. Errors appear in Tools
without preventing valid extensions from loading. Manifests are bounded to 64 KiB,
64 commands each, and 128 files per scope.

## Manifest

```json
{
  "api_version": 1,
  "id": "my-note-tools",
  "name": { "en": "My note tools", "nb": "Mine notatverktøy" },
  "commands": [
    {
      "id": "inspect-note",
      "name": { "en": "Inspect current note", "nb": "Vis gjeldende notat" },
      "program": "/usr/bin/stat",
      "args": ["{{file}}"],
      "working_directory": "folder"
    }
  ]
}
```

`program` is an installed executable name or absolute path. `args` defaults to
`[]`. `working_directory` is `vault` (default) or `folder` (current dashboard or
active note's parent, falling back to the vault). It must resolve within the
current vault. Arguments support literal `{{vault}}`, `{{folder}}`, and `{{file}}`
substitution; a missing current file prevents a `{{file}}` command from launching.
Paths containing spaces remain one argument. Shell expansion is not performed.
Names must include both `en` and `nb`. Use unique plugin and command IDs.

To launch an AI CLI, change `program` and `args` to its documented executable and
options. Built-in launchers cover `claude`, `codex`, `vibe`, `pi`, `omp`, and
`opencode`; they do not install, authenticate, or call a hosted provider by
themselves. Tool versions, account access, and provider charges belong to the CLI.
Use an absolute path if the command is not on the app's inherited PATH.

## Security boundary

The only executable capability in v1 is **launch a local process**, granted for
one launch by an explicit confirmation. It includes all the process's normal
filesystem, environment, and network access. A vault working directory is NOT
a sandbox: a process may read outside it, change files, access credentials from
the environment, or contact a paid service. JSON cannot constrain these rights.
Only run trusted extensions and programs; do not place secrets in manifests.
No persistent “trust this vault” grant or background lifecycle hook exists.

## Terminal

**⌘J** or the status-bar **Terminal** button shows/hides the panel. The default
shell starts in the current folder with the normal login-shell configuration.
The native renderer supports ANSI colors, a cursor, alternate-screen apps,
interactive input, Unicode composition, resize, keyboard controls, bracketed
paste, and bounded scrollback. **Ctrl+C** interrupts; **Stop** terminates the
active session. **Copy**/⌘C copies visible output. Hidden and inactive tabs keep
their processes alive. Use **+** to start a new shell, or right-click a folder in
the tree or dashboard and choose **Open terminal here**. The launch confirmation
shows that folder, even when another page is selected. Every tool opens in a new
tab without replacing existing sessions. Each tab has a close button that stops
only its process. Closing/switching the vault or closing the app stops all
sessions; terminal tabs are not restored.
Rísta does not persist terminal output or commands; the shell may save its normal
history. Mouse reporting, text-range selection, terminal images, and advanced
keyboard protocols are not implemented in this first version.

External note edits go through the existing vault watcher and conflict detection;
an agent must not assume it can overwrite an unsaved editor buffer. Rich graphical
agent integrations need a documented structured protocol; terminal launchers do
not pretend to be such integrations. macOS is the initial target; Linux/Windows
terminal behavior and keyboard mappings are not verified.
