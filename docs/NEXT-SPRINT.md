# Rísta Next Sprint

## Sprint Goal

Strengthen Rísta's foundation so new features can be added without destabilizing the editor, layout, or file workflows.

This sprint focuses on:
- renderer cleanup
- path-safe behavior
- regression safety
- a few infrastructure fixes with high leverage

## Priority Tasks

### 1. Split renderer responsibilities out of `index.js`

Target:
- begin extracting app shell, pane layout, tabs, and tree logic into modules

Definition of done:
- [src/renderer/index.js](../src/renderer/index.js) is meaningfully smaller
- at least 2-3 modules are created
- build passes

### 2. Document pane and workspace transitions in code

Target:
- centralize pane/workspace transition helpers

Definition of done:
- mode changes no longer depend on scattered direct mutations
- transition rules are easier to inspect

### 3. Add and validate `TEST-CASES.md`

Target:
- create the manual regression checklist and use it after layout/editor changes

Definition of done:
- [TEST-CASES.md](./TEST-CASES.md) exists
- core pane transitions are covered

### 4. Fix path-based explorer highlighting

Target:
- replace filename-based matching with full path identity

Definition of done:
- duplicate filenames in different folders do not confuse highlighting

### 5. Fix preload unsubscribe behavior

Target:
- make event unsubscribe remove only the intended listener

Definition of done:
- `onFileChange()` and `onCommand()` do not use broad `removeAllListeners`

### 6. Rebuild PDF export as document-only export

Target:
- export should print document content, not the full app shell

Definition of done:
- exported PDF no longer includes sidebars/toolbars/empty panes

### 7. Separate debounce scheduling for save and preview

Target:
- avoid tying heavy updates to the same idle path

Definition of done:
- autosave and preview rendering are scheduled independently

### 8. Reduce tree refresh scope after file changes

Target:
- stop full tree refreshes for simple watcher updates where possible

Definition of done:
- file-change handling updates only what is necessary

### 9. Add `ARCHITECTURE.md`

Target:
- provide a short implementation-facing architecture guide

Definition of done:
- the current architectural direction is documented concisely

### 10. Update worklog with sprint progress

Target:
- keep handoff quality high while refactors are underway

Definition of done:
- [WORKLOG.md](./WORKLOG.md) reflects the latest architecture and sprint status

## Suggested Execution Order

1. Fix path-based explorer highlighting
2. Fix preload unsubscribe behavior
3. Rebuild PDF export
4. Add `TEST-CASES.md`
5. Add `ARCHITECTURE.md`
6. Start renderer module extraction
7. Centralize pane/workspace transition helpers
8. Separate debounce pipelines
9. Reduce tree refresh scope
10. Update worklog

## Success Criteria For The Sprint

- core editor and pane flows remain stable
- build passes throughout
- at least three infrastructure problems are removed
- the renderer becomes easier to evolve
- tomorrow's work starts from a cleaner base than today's

