// ── Built-in note templates ──────────────────────────────────────
import { state } from './state.js'

const BUILT_IN_TEMPLATES = [
  { name: 'Blog Post', content: '# {{title}}\n\n*{{date}}*\n\n' },
  { name: 'Meeting Notes', content: '# Meeting Notes — {{date}}\n\n## Attendees\n\n- \n\n## Agenda\n\n1. \n\n## Notes\n\n\n\n## Action Items\n\n- [ ] ' },
  { name: 'Daily Note', content: '# {{date}}\n\n## Tasks\n\n- [ ] \n\n## Notes\n\n' },
  { name: 'README', content: '# Project Name\n\n## Description\n\n\n\n## Installation\n\n```bash\nnpm install\n```\n\n## Usage\n\n## License\n\nMIT' },
  { name: 'Changelog', content: '# Changelog\n\n## [Unreleased]\n\n### Added\n\n- \n\n### Changed\n\n### Fixed\n\n' },
]

export async function getAvailableTemplates() {
  const builtIn = [...BUILT_IN_TEMPLATES]
  if (state.folderPath && window.fjord.readTemplates) {
    try {
      const userTemplates = await window.fjord.readTemplates(state.folderPath)
      return [...builtIn, ...userTemplates]
    } catch { /* ignore */ }
  }
  return builtIn
}

export function resolveTemplateVars(content) {
  const now = new Date()
  const date = now.toISOString().split('T')[0]
  let resolved = content.replace(/\{\{date\}\}/g, date)
  if (resolved.includes('{{title}}')) {
    const title = prompt('Title:') || 'Untitled'
    resolved = resolved.replace(/\{\{title\}\}/g, title)
  }
  return resolved
}
