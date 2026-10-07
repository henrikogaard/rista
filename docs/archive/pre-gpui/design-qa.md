> **Historical, pre-GPUI document.** Preserved as an archive; paths, commands, feature descriptions, and plans below are not current product guidance. See the [active documentation index](../../README.md).

# Design QA — Nordic library shell

## Comparison setup

- Source visual: `/Users/henrik/.codex/generated_images/019fc360-160b-70e3-98a7-55bc16adcaf2/exec-92152e0e-0955-416f-8f99-890c5b9b2742.png`
- Final implementation: `/Users/henrik/.codex/visualizations/2026/08/02/019fc360-160b-70e3-98a7-55bc16adcaf2/rista-terminal-refined-rich-final.png`
- Full comparison: `/Users/henrik/.codex/visualizations/2026/08/02/019fc360-160b-70e3-98a7-55bc16adcaf2/rista-terminal-comparison-final.png`
- Focused comparison: `/Users/henrik/.codex/visualizations/2026/08/02/019fc360-160b-70e3-98a7-55bc16adcaf2/rista-terminal-comparison-focused-final.png`
- Native PTY proof: `/Users/henrik/.codex/visualizations/2026/08/02/019fc360-160b-70e3-98a7-55bc16adcaf2/rista-native-final-pty-ok.png`
- Source and implementation viewport: 1487 × 1058 at 1:1 density
- Native viewport: 1280 × 800
- State: dark Ember theme, Product direction note, Outline tab active, editor shelf hidden, Rich Text active, terminal open with a successful Git status result

## Iteration record

1. Pass 1 found P2 issues in control contrast, heading scale, default heading dividers, callout/list treatment, and terminal height. The implementation was updated with clearer toolbar controls, source-matched type scale, warm callout treatment, accent list markers, and a more compact terminal.
2. Pass 2 found P2 horizontal drift from an overly wide rich-text measure. The prose surface was constrained and centered to match the reference.
3. Pass 3 found P2 vertical drift from Toast UI reserving 46 px for its hidden internal toolbar. The internal toolbar is now removed from layout and the editor main area fills its mount.
4. Pass 4 compared the final source and implementation at the same viewport. No P0, P1, or P2 visual discrepancies remained.
5. The follow-up audit found the workspace still relied on long square seams and the terminal was a cramped one-shot command runner. Major surfaces were inset with 10–12 px radii and quiet gutters, and the command runner was replaced with a resizable xterm.js surface backed by a persistent native PTY.
6. The final browser and native comparisons verified the refined rounded geometry, hidden editor shelf, terminal hierarchy, connected shell state, and real command output. No P0, P1, or P2 visual discrepancies remain.

## Interaction checks

- Opened a project note and verified the activity rail, library, editor, context sidebar, and status metrics.
- Toggled the editor shelf off and back on from the quiet pane-header control.
- Switched through Rich Text, Markdown, and Split; each mode activated and rendered its intended surface.
- Opened the More menu and retained access to secondary formatting and view actions.
- Opened the terminal with its rail control, entered `git status --short`, and received `working tree clean` in the browser-backed QA harness.
- Enabled the terminal through Settings → Editor → Experimental in the packaged macOS app, opened it through its activity-rail control, and verified a `Connected` native PTY state.
- Entered a command in the packaged terminal and received `RISTA_PTY_OK`; verified native zsh rendering, ANSI output, cursor, prompt, and scrollback surface.
- Verified the terminal exposes Copy, Clear, Restart, collapse, close, a drag resize handle, and the consistent `⌘J` shortcut.
- Switched between the Outline and Properties context tabs.
- Checked the browser console. There were no application errors. Toast UI emitted its known selection-endpoint warning after mode switching; it did not block the tested mode changes.

## Residual P3 differences

- Toast UI's WYSIWYG surface shows the literal `[!tip]` marker inside the callout. Rísta's rendered Markdown pipeline transforms the same syntax; matching that transformation in WYSIWYG is a separate editor-engine enhancement.
- Optional Backlinks and Assistant tabs remain feature-gated to preserve Rísta's core-by-default product rule.
- Browser preview commands use the safe harness fallback; persistent shell state, streaming PTY output, and native window chrome were separately verified in the packaged Tauri app.

final result: passed
