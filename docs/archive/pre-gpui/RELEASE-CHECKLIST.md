> **Historical, pre-GPUI document.** Preserved as an archive; paths, commands, feature descriptions, and plans below are not current product guidance. See the [active documentation index](../../README.md).

# Rista Release Checklist

## Purpose

Pre-flight checks before cutting a release. Run these steps in order before
tagging and packaging.

## Pre-Release

- [ ] All PRs targeting this release are merged
- [ ] No open todos, fixmes, or debugger statements
- [ ] `npm run build` passes (zero warnings)
- [ ] `cargo build` passes (zero warnings, `cargo clippy` clean)
- [ ] `npm test` passes (all tests green)
- [ ] WORKLOG.md is updated through the release date

## Regression QA (manual)

Run the full TEST-CASES.md pass:
- [ ] App boot
- [ ] Folder open / close / switch
- [ ] File open, tab switch, tab close
- [ ] Duplicate filename safety (same name, different folders)
- [ ] Standalone pane modes: Markdown, Preview, WYSIWYG
- [ ] Split pane: left/right editable, preview side toggle
- [ ] Dual workspace mode
- [ ] Explorer navigation
- [ ] File operations: create, rename, delete, move
- [ ] Watcher: create/rename/delete file externally, verify UI updates
- [ ] Session restore: relaunch with previous folder
- [ ] External conflict detection: edit file externally while tab is dirty
- [ ] Auto-save: verify debounce fires, content persists on reload
- [ ] Keyboard shortcuts: all registered shortcuts work
- [ ] Command palette: file search, command search, dismiss
- [ ] Find & replace: find, replace, replace all
- [ ] Theme toggle: dark ↔ light, editor + preview surfaces update
- [ ] Focus mode / zen mode
- [ ] Print / PDF export
- [ ] Settings panel: every toggle and control
- [ ] Settings persistence: close, reopen, verify state restored

## Platform Checks

- [ ] macOS: ⌘ shortcuts, native menu, window controls
- [ ] Windows/Linux: Ctrl shortcuts (verify keybindings module)
- [ ] Dark theme: all surfaces readable
- [ ] Light theme: all surfaces readable
- [ ] High-contrast / accessibility: contrast ratios meet WCAG AA

## Performance Check

- [ ] Cold launch ≤ 400 ms (check console for perf budget warnings)
- [ ] File-switch ≤ 80 ms p95

## Release

- [ ] Version bumped in `package.json` and `tauri.conf.json`
- [ ] CHANGELOG.md entry added for this release
- [ ] Git tag created (`git tag vX.Y.Z`)
- [ ] Tag pushed (`git push --tags`)
- [ ] `npm run package` completes without error
- [ ] Packaged app launches and passes a minimal QA smoke test
- [ ] Release notes published on GitHub
