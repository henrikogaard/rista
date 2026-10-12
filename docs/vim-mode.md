# Vim mode

Vim mode is an optional, keyboard-focused editing mode. Enable it in Settings;
it is off by default. It applies only while a writable note's source editor is
focused. Disable it at any time without restarting the app. The status bar shows
the current mode and command/search entry.

This is a focused command subset, not full Vim compatibility.

## Modes and movement

- `i`, `a`, `I`, `A` enter Insert mode before/after the cursor or at the first
  nonblank character/end of the line.
- `o` and `O` open a line below or above and enter Insert mode.
- `Escape` or `Ctrl-[` returns to Normal mode.
- `v` and `V` start characterwise and linewise Visual selection.
- `h`, `j`, `k`, `l`, `w`, `b`, `e`, `0`, `$`, `^`, `gg`, `G`, `f{char}`,
  `t{char}`, and `%` provide horizontal/vertical, word, line, file, find/till,
  and matching-delimiter motions.
- Decimal counts can prefix motions and operators; an operator count and a
  motion count multiply.

## Operators and edits

- `d`, `c`, and `y` work with supported motions, counts, Visual selections, and
  the text objects `iw`, `aw`, `i"`, `a"`, `i(`, `a(`, and `ip`.
- `x`, `X`, `dd`, `cc`, `yy`, `D`, and `C` delete, change, or yank characters,
  lines, or the rest of a line.
- `p` and `P` paste after or before the cursor; linewise registers paste whole
  lines.
- `.` repeats supported Normal-mode edits and changes relative to the current
  cursor. It does not repeat yanks, navigation, or Visual edits; Visual changes
  are intentionally non-repeatable in this focused subset.
- `u` invokes native Undo and `Ctrl-r` invokes native Redo.

Counts are bounded to keep command processing linear. Counted dot repeats are
simulated atomically and then applied as one native editor replacement. The
repeat work budget is at least 16 MiB, or twice the initial note size when that
is larger; an over-budget repeat is rejected with a localized status and no
partial edit. A repeat that makes no change emits no edit.

The editor's native undo system does not expose a Vim transaction API. A change
operator's deletion and the text subsequently typed in Insert mode may therefore
require separate Undo steps.

## Registers

The unnamed register and lower-case named registers (`"a` … `"z`) are shared
between open notes in the current process. Upper-case names append to their
lower-case register. `"_` discards a yank or delete. Ordinary registers never
write to the system clipboard. Use `"+` or `"*` explicitly to read or write the
system clipboard; the platform's normal copy/paste shortcuts remain unchanged.

## Search and commands

- `/pattern` starts a forward search; `Enter` runs it, and `Escape` cancels.
  `n` and `N` repeat in the same or opposite direction, wrapping at the buffer
  boundary.
- Search patterns use Rust's regular-expression syntax, with bounded pattern
  length and compilation size. Invalid patterns show a localized status
  message. Look-around and backreferences are not supported by that syntax.
- `:w` invokes the app's normal Save action. Other Ex commands are unsupported
  and show a localized status message; command input never runs a shell.

Command and search entry support `Escape`, `Backspace`, and `Enter`. Search
patterns are interpreted as regular expressions rather than Vim's full search
language.
