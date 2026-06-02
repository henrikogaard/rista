const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')

async function importMarkdownModule() {
  const source = fs.readFileSync(path.join(root, 'src/renderer/markdown.js'), 'utf8')
  const modulePath = path.join(root, `.tmp-markdown-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  fs.writeFileSync(modulePath, source)
  try {
    return await import(`file://${modulePath}`)
  } finally {
    fs.rmSync(modulePath, { force: true })
  }
}

test('Obsidian callout fold markers render without leaking syntax', async () => {
  const { renderMarkdown } = await importMarkdownModule()

  const html = await renderMarkdown('> [!warning]- Check this\n> Keep **calm**.')

  assert.match(html, /class="callout callout--warning"/)
  assert.match(html, /data-callout-fold="closed"/)
  assert.match(html, /<div class="callout__title">Check this<\/div>/)
  assert.doesNotMatch(html, /\[!warning\]/)
  assert.doesNotMatch(html, />- Check this</)
})

test('Obsidian attachment image embeds render as local images', async () => {
  const { renderMarkdown } = await importMarkdownModule()

  const html = await renderMarkdown('![[images/Cover.png|Recipe cover]]', {
    currentFilePath: '/vault/Recipes/Smores.md',
  })

  assert.match(html, /class="obsidian-embed obsidian-embed--image"/)
  assert.match(html, /src="file:\/\/\/vault\/Recipes\/images\/Cover.png"/)
  assert.match(html, /alt="Recipe cover"/)
  assert.doesNotMatch(html, /!\[\[/)
})

test('Obsidian block reference ids become hidden anchors', async () => {
  const { renderMarkdown } = await importMarkdownModule()

  const html = await renderMarkdown('Remember this paragraph ^abc-123\n\n[[Other#^abc-123]]')

  assert.match(html, /class="block-ref-anchor"/)
  assert.match(html, /data-block-ref="abc-123"/)
  assert.match(html, /data-wikilink="Other\#\^abc-123"/)
  assert.doesNotMatch(html, /paragraph \^abc-123/)
})

test('document banners resolve Obsidian attachments from the whole vault', async () => {
  const { renderMarkdown } = await importMarkdownModule()

  const html = await renderMarkdown('---\nbanner: "[[Smores.jpeg]]"\nbanner_x: 0.25\nbanner_y: 0.75\n---\n\n# Family Favorites', {
    currentFilePath: '/vault/Family/Objectives/Family Recipes.md',
    attachmentPaths: ['/vault/images/Smores.jpeg'],
  })

  assert.match(html, /class="document-banner"/)
  assert.match(html, /src="file:\/\/\/vault\/images\/Smores.jpeg"/)
  assert.match(html, /object-position:25% 75%/)
})
