# Local extensions and terminal tools (API v1)

Rísta reads declarative JSON extensions without recompiling. Extensions can add
commands to **Tools** and the command palette, and configure external agents for
the native Agent panel. Commands launch an installed executable in the native
PTY terminal. Agents communicate through ACP over newline-delimited JSON-RPC.
Five built-in presets launch ACP adapters; custom manifest agents are also
supported. Adapters and providers are not bundled: Rísta does not install tools
or authenticate, and credentials remain with the configured tools. There is no
JavaScript/WASM runtime.

Create and inspect a manifest, then choose **Tools → Reload**. No process runs
on discovery, reload, startup, or vault opening. Launching an agent requires
explicit confirmation showing its executable, arguments, vault working
directory, and unsandboxed permissions. Processes launch directly with an
argument vector, not through a shell.

## Locations

- User scope: `~/Library/Application Support/no.ogard.rista/plugins/*.json` on macOS.
  **Open extensions folder** locates the equivalent config directory on other OSes.
- Vault scope: `<vault>/.rista/plugins/*.json`.

Only direct JSON files are loaded; symlinks, invalid manifests, unknown fields,
unsupported API versions, and duplicate plugin IDs are rejected. User extensions
load first; a vault cannot replace one by reusing its ID. Errors appear in Tools
without preventing valid extensions from loading. Manifests are bounded to 64 KiB,
64 commands and 64 agents each, and 128 files per scope.

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
  ],
  "agents": [
    {
      "id": "my-agent",
      "name": { "en": "My agent", "nb": "Min agent" },
      "program": "/usr/local/bin/my-agent",
      "args": ["--stdio"]
    }
  ]
}
```

The `agents` array is optional. Each entry requires a unique `id`, localized
`name`, and `program`; `args` defaults to an empty array. The Agent picker lists
the built-in presets followed by configured agents. The IDs `vibe`,
`rista-codex`, `rista-claude`, `rista-opencode`, and `rista-grok` are reserved
for those presets and cannot be used by a manifest agent. If no manifest agents
are configured, **Open extensions folder** opens the vault extension directory
(or the user extension directory without a vault). Configured executables
receive the vault root as their working directory.

## Built-in agent presets

Rísta launches these programs and argument vectors directly; each preset uses
an ACP adapter rather than a regular chat CLI:

| Preset | Program | Arguments |
|---|---|---|
| Vibe | `vibe-acp` | none |
| Codex | `codex-acp` | none |
| Claude | `claude-agent-acp` | none |
| OpenCode | `opencode` | `acp` |
| Grok (via OpenCode) | `opencode` | `acp` |

Install and configure the tools yourself:

- **Vibe:** `uv tool install mistral-vibe`, then run `vibe` to complete setup.
- **Codex:** `npm install -g @agentclientprotocol/codex-acp`; sign in with
  `codex login` or use the adapter's documented credential flow.
- **Claude:** `npm install -g @agentclientprotocol/claude-agent-acp` (Node.js
  22+), then authenticate using Claude's own CLI or setup.
- **OpenCode:** `npm install -g opencode-ai`, then run `opencode auth login`.
- **Grok:** install and configure OpenCode as above, enable xAI API access, and
  choose an available xAI model in the Agent panel. Grok is not a separate
  adapter; Rísta only offers model IDs advertised by the ACP session that begin
  with `xai/`. If none are available, configure xAI in OpenCode and restart the
  conversation.

Rísta never installs or authenticates these tools automatically. No provider
credentials are stored in Rísta settings or manifests.

`program` is an installed executable name or absolute path. `args` defaults to
`[]`. On macOS, agent launches use the app's inherited absolute PATH entries
plus existing common directories such as Homebrew, `/usr/local`, and selected
user tool directories. Rísta does not evaluate shell startup files or recreate
a login-shell environment; NVM and other custom shell paths may require an
absolute executable path in the manifest. Literal argument boundaries are
preserved and shell expansion is not performed. `working_directory` is `vault`
(default) or `folder` (current dashboard or active note's parent, falling back
to the vault). It must resolve within the current vault. Arguments support
literal `{{vault}}`, `{{folder}}`, and `{{file}}` substitution; a missing current
file prevents a `{{file}}` command from launching. Paths containing spaces
remain one argument.
Names must include both `en` and `nb`. Use unique plugin and command IDs.

Right-click a file or folder and choose **Tools here…** to use that selection
rather than the open tab. A selected file supplies `{{file}}` and its parent
supplies `{{folder}}`; a selected folder/dashboard has no current file, so a
command requiring `{{file}}` will not launch. Reload preserves this context.

For a command extension that launches another CLI in the terminal, set
`program` and `args` to its documented executable and options. Terminal
launchers cover `claude`, `codex`, `vibe`, `pi`, `omp`, and `opencode`; they do
not install or authenticate tools or call a hosted provider by themselves. An
Agent manifest is different: its executable must speak ACP. Tool versions,
account access, and provider charges belong to the CLI. Use an absolute path if
the command is not on the app's inherited PATH.

## Agent protocol and security boundary

Agent support currently implements ACP initialization and new sessions,
streaming prompts and updates, tool calls and plans, permission requests,
advertised model selection, and the `fs/read_text_file` / `fs/write_text_file`
methods. Context files are sent as resource links. Authentication is managed
by the configured agent; if setup reports an authentication error, complete
authentication using that agent's own setup and start a new conversation.
Rísta does not request or store agent credentials.

Protocol-mediated file reads and writes are restricted to the open vault;
symlink escapes are rejected. Reads prefer unsaved contents of open buffers and
support line-range requests. ACP-mediated write requests are shown as proposed
changes and must be accepted before Rísta applies them. Accept updates an open
buffer or writes a non-open file; Reject returns an error to the agent. This
review covers only writes requested through ACP: the agent or its tools may
also modify files directly and bypass protocol-mediated review.

The agent process runs with your normal user permissions and is **not sandboxed**.
It inherits the app's environment and may access files and the network. A vault
working directory is not a sandbox and cannot constrain those rights. Only
launch trusted programs; do not put secrets in manifests. No persistent “trust
this vault” grant or background lifecycle hook exists.

## Terminal

**⌘J** or the status-bar **Terminal** button shows/hides the panel. The default
shell starts in the current folder as an interactive login shell on Unix/macOS.
Settings → Terminal can select an executable (blank uses `$SHELL`), an installed
monospace/Nerd Font, and font size. Shell changes affect new sessions; typography
changes affect existing ones. zsh loads its normal startup files and Oh My Zsh
configuration; Rísta never writes those files. A prompt that needs Nerd Font
glyphs requires that font installed and selected. This is not a promise of full
compatibility with every terminal protocol or prompt plugin.
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
The folder button returns to the tab's **launch folder**; it does not track later
`cd` commands. Screen snapshots are cached until output changes, and inactive tabs
poll less often without requesting redraws; their processes still run normally.
Rísta does not persist terminal output or commands; the shell may save its normal
history. Mouse reporting, text-range selection, terminal images, and advanced
keyboard protocols are not implemented in this first version.

External note edits go through the existing vault watcher and conflict detection;
the Agent panel reads unsaved open-buffer contents and reviews its own
protocol-mediated writes. The terminal and Agent panel are separate: launching a
terminal command does not give it ACP streaming, permission prompts, or review
cards. macOS is the initial target; Linux/Windows terminal behavior and keyboard
mappings are not verified.
