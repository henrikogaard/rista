# Command-line interface

The `rista` executable supports local vault operations and can open paths in
the desktop app. Headless commands run before the app, updater, or HTTP client
starts. They do not use the network.

## Commands

```sh
rista open ~/Notes [folder/Note.md]
rista new ~/Notes "Río plan" [--template meeting]
rista daily ~/Notes
rista append ~/Notes "Río plan.md" "Follow up tomorrow"
printf '%s' "Exact text" | rista append ~/Notes "Río plan.md" -
cat change.md | rista append ~/Notes "Río plan.md"
rista search ~/Notes "project status"
rista links "Río plan.md" [--vault ~/Notes]
rista backlinks "Río plan.md" [--vault ~/Notes]
```

`append` adds exactly the supplied UTF-8 bytes: it does not add a bullet or
newline. If its text argument is omitted or is `-`, it reads standard input.
Input larger than 2 MiB is rejected rather than truncated. `new`, `daily`, and
`append` only target `.md` or `.markdown` notes; `.md` is added when the name
has no extension. Creating a note never overwrites an existing path. `daily`
uses the vault's configured daily folder and date format, with an optional
`daily.md` template in the configured templates folder. `new --template`
resolves the template inside that configured folder. Templates requiring
interactive prompts, clipboard, or selection context are not supported by the
CLI.

`search` prints matching filesystem paths using the vault's existing search
behavior, which may include non-Markdown files. `links` prints resolved local
Markdown targets, and `backlinks` prints source paths that link to the requested note.
These are deterministic, disk-only results; unsaved editor contents are not
included. `links` and `backlinks` use the current directory as the vault when
`--vault` is omitted. Remote URLs are never fetched. Hidden folders and
symlinks are excluded from note scans. Link indexing is bounded to 16 MiB of
note content.

`--help` lists the commands and `--version` prints the application version.
Bare paths and multiple paths retain the legacy behavior of opening them in
the app. Use `rista -- open` to open a path whose name is also a command.

## URL actions

The app accepts `rista:` URLs using percent-encoded UTF-8 query values:

```text
rista://open?vault=%2FUsers%2Fyou%2FNotes&file=R%C3%ADo%20plan.md
rista://new?vault=%2FUsers%2Fyou%2FNotes&name=Draft%20notes
rista://daily?vault=%2FUsers%2Fyou%2FNotes
rista://append?vault=%2FUsers%2Fyou%2FNotes&file=Draft.md&text=More%20text
rista://search?vault=%2FUsers%2Fyou%2FNotes&query=project%20status
rista://links?vault=%2FUsers%2Fyou%2FNotes&file=Draft.md
rista://backlinks?vault=%2FUsers%2Fyou%2FNotes&file=Draft.md
```

URL fields must be unique and known for the selected action; malformed
percent-encoding, credentials, ports, fragments, and non-local vault paths are
rejected. URL writes (`new`, `daily`, and `append`) require native confirmation
before any file or directory is changed. Cancel is the keyboard default.
Dirty, conflicted, or externally changed open notes are not silently
overwritten.

## Limits and errors

Notes and appended text are limited to 2 MiB. Local IPC requests are limited
to 4 MiB. Link scans are limited to 16 MiB of note content. Invalid paths,
ambiguous wiki links, unreadable notes, unsafe symlink paths, and oversized
input return an error; results are never silently truncated. Read-only
commands use the bytes currently on disk.

The Unix app instance and CLI writers coordinate through a private local
socket and process lock. If a running app cannot confirm a forwarded write,
the CLI reports an error and does not retry the disk write. File writes refuse
dirty or conflicted editor buffers rather than saving or discarding them.
Non-Unix builds report that coordinated writes are unsupported.

## Install the command

On macOS, use **Rísta → Install Command-Line Tool** in the packaged app. After
native confirmation, Rísta installs a symlink at `/usr/local/bin/rista`
pointing to the executable inside that app. An existing unrelated file or
symlink is never replaced. If system permissions are required, macOS asks for
administrator authorization only after confirmation. Reinstalling is safe when
the destination already points to this app. A stale symlink left after moving
the app is refused; remove it manually before installing again. Development
executables cannot be installed this way.

CLI and URL behavior is currently tested on macOS only. URL scheme registration
in a packaged installation has not yet been verified.
