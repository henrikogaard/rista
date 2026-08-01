// ── First-run sample document ────────────────────────────────────
// Creates a welcome file on first launch so new users aren't dropped
// into a blank screen. Guards itself with a localStorage flag.

export const FIRST_RUN_SAMPLE = `# Welcome to Rista ✦

A local-first Markdown editor. Your files, your folder — no cloud, no accounts.

## Getting started

Open a folder with **⌘O** to see all your notes in the sidebar.
Press **⌘K** to jump to any file or run a command.

## Writing shortcuts

| Action | Shortcut |
|---|---|
| Bold | ⌘B |
| Italic | ⌘I |
| Inline code | ⌘\` |
| Find & replace | ⌘F |
| Project search | ⇧⌘F |
| Zen mode | ⇧⌘↵ |

## Views

Toggle between **Edit**, **Split**, and **Preview** using the buttons in the toolbar.

## Tips

- Type \`---\` on its own line for a horizontal rule
- Type \`"quotes"\` and they become "smart quotes" automatically
- Use \`#tag\` anywhere in a note to build a tag index

---

_This file lives at \`~/Documents/Rista/welcome.md\`. Feel free to edit or delete it._
`

export async function ensureFirstRunSample(openSingleFilePath) {
  if (!window.fjord || localStorage.getItem('rista-onboarded')) return
  localStorage.setItem('rista-onboarded', '1')
  try {
    const home = await window.fjord.getHomeDir?.()
    if (!home) return
    const dir = `${home}/Documents/Rista`
    const samplePath = `${dir}/welcome.md`
    await window.fjord.createDir(dir).catch(() => {})
    const existing = await window.fjord.stat(samplePath).catch(() => null)
    if (!existing) {
      await window.fjord.writeFile(samplePath, FIRST_RUN_SAMPLE)
    }
    await openSingleFilePath(samplePath)
  } catch (_) { /* non-fatal — empty state is fine */ }
}
