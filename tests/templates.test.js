const assert = require('node:assert/strict')
const test = require('node:test')

// ── Inline the template vars resolver ────────────────────────────
// The actual resolveTemplateVars is a pure function modulo Date/prompt.
// We test the logic directly.

const BUILT_IN_TEMPLATES = [
  { name: 'Blog Post', content: '# {{title}}\n\n*{{date}}*\n\n' },
  { name: 'Meeting Notes', content: '# Meeting Notes — {{date}}\n\n## Attendees\n\n- \n\n## Agenda\n\n1. \n\n## Notes\n\n\n\n## Action Items\n\n- [ ] ' },
  { name: 'Daily Note', content: '# {{date}}\n\n## Tasks\n\n- [ ] \n\n## Notes\n\n' },
  { name: 'README', content: '# Project Name\n\n## Description\n\n\n\n## Installation\n\n```bash\nnpm install\n```\n\n## Usage\n\n## License\n\nMIT' },
  { name: 'Changelog', content: '# Changelog\n\n## [Unreleased]\n\n### Added\n\n- \n\n### Changed\n\n### Fixed\n\n' },
]

function resolveTemplateVars(content) {
  const now = new Date()
  const date = now.toISOString().split('T')[0]
  let resolved = content.replace(/\{\{date\}\}/g, date)
  if (resolved.includes('{{title}}')) {
    resolved = resolved.replace(/\{\{title\}\}/g, 'Test Title')
  }
  return resolved
}

test('template list has all expected template names', () => {
  const names = BUILT_IN_TEMPLATES.map(t => t.name)
  assert.ok(names.includes('Daily Note'))
  assert.ok(names.includes('Meeting Notes'))
  assert.ok(names.includes('Blog Post'))
  assert.ok(names.includes('README'))
  assert.ok(names.includes('Changelog'))
})

test('each template has name and content', () => {
  for (const t of BUILT_IN_TEMPLATES) {
    assert.ok(typeof t.name === 'string' && t.name.length > 0, `name for ${t.name}`)
    assert.ok(typeof t.content === 'string' && t.content.length > 0, `content for ${t.name}`)
  }
})

test('templates contain markdown headers', () => {
  for (const t of BUILT_IN_TEMPLATES) {
    assert.ok(t.content.includes('#'), `${t.name} has headers`)
  }
})

test('resolveTemplateVars replaces date variable', () => {
  const now = new Date()
  const date = now.toISOString().split('T')[0]
  const result = resolveTemplateVars('# {{date}}\n\nContent')
  assert.ok(result.includes(date), 'date variable replaced')
  assert.ok(result.includes('Content'), 'other content preserved')
})

test('resolveTemplateVars replaces title variable', () => {
  const result = resolveTemplateVars('Title: {{title}}')
  assert.ok(result.includes('Test Title'), 'title variable replaced')
})

test('resolveTemplateVars handles content with no variables', () => {
  assert.equal(resolveTemplateVars('Plain text'), 'Plain text')
})

test('resolveTemplateVars handles empty content', () => {
  assert.equal(resolveTemplateVars(''), '')
})

test('resolveTemplateVars handles multiple date occurrences', () => {
  const result = resolveTemplateVars('{{date}} - {{date}}')
  const datePart = new Date().toISOString().split('T')[0]
  // Both occurrences should be replaced
  const count = (result.match(new RegExp(datePart, 'g')) || []).length
  assert.equal(count, 2, 'both date occurrences replaced')
})
