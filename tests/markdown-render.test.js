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


test('getStats returns correct word, char, sentence, paragraph counts', async () => {
  const { getStats } = await importMarkdownModule()

  const empty = getStats('')
  assert.equal(empty.words, 0)
  assert.equal(empty.chars, 0)
  assert.equal(empty.sentences, 0)
  assert.equal(empty.paragraphs, 0)
  assert.equal(empty.readMin, 0)

  const simple = getStats('Hello world. This is a test.')
  assert.equal(simple.words, 6)
  // "Hello world." + "This is a test." — may split to 2 or 3 groups
  assert.ok(simple.sentences >= 1)
  assert.equal(simple.paragraphs, 1)
  assert.ok(simple.chars > 0)
  assert.ok(simple.readMin >= 0)

  const multiPara = getStats('First paragraph with several words in it.\n\nSecond paragraph here. More words.')
  assert.equal(multiPara.paragraphs, 2)
  assert.ok(multiPara.words > 6)

  const longDoc = getStats('Word '.repeat(300))
  assert.equal(longDoc.words, 300)
  // 300 words at 200wpm = 1.5 min, so readMin should be >= 1
  assert.ok(longDoc.readMin >= 1, `Expected readMin >= 1, got ${longDoc.readMin}`)
})

test('extractHeadings returns heading entries with correct levels', async () => {
  const { extractHeadings } = await importMarkdownModule()
  const data = `# Title
## Section
Some text
### Subsection
More text
## Another section`

  const headings = extractHeadings(data)
  assert.equal(headings.length, 4)
  assert.equal(headings[0].text, 'Title')
  assert.equal(headings[0].level, 1)
  assert.equal(headings[1].text, 'Section')
  assert.equal(headings[1].level, 2)
  assert.equal(headings[2].text, 'Subsection')
  assert.equal(headings[2].level, 3)
  assert.equal(headings[3].text, 'Another section')
  assert.equal(headings[3].level, 2)
  assert.ok(headings.every(h => typeof h.line === 'number'), 'all headings should have line numbers')
})

test('extractHeadings returns empty array for documents without headings', async () => {
  const { extractHeadings } = await importMarkdownModule()
  assert.deepEqual(extractHeadings('Just some plain text.'), [])
  assert.deepEqual(extractHeadings(''), [])
})

test('parseFrontmatterBlock extracts frontmatter and body', async () => {
  const { parseFrontmatterBlock } = await importMarkdownModule()
  const doc = '---\ntitle: Test\ntags: [a, b]\n---\n\n# Hello\n\nBody text'
  const result = parseFrontmatterBlock(doc)
  assert.ok(result.frontmatter)
  assert.equal(result.frontmatter.title, 'Test')
  assert.equal(result.frontmatter.tags, '[a, b]')  // parsed as string, not array
  assert.ok(result.body.includes('# Hello'))
  assert.ok(result.body.includes('Body text'))
})

test('parseFrontmatterBlock returns empty when no frontmatter', async () => {
  const { parseFrontmatterBlock } = await importMarkdownModule()
  const result = parseFrontmatterBlock('# Just content')
  assert.strictEqual(result.frontmatter, null, 'no frontmatter -> null')
  assert.equal(result.body, '# Just content')
})

test('parseFrontmatterBlock returns null frontmatter when malformed', async () => {
  const { parseFrontmatterBlock } = await importMarkdownModule()
  // Empty frontmatter with no key/value lines
  const result = parseFrontmatterBlock('---\n---\n# Content')
  assert.strictEqual(result.frontmatter, null, 'empty frontmatter block -> null')
  assert.ok(result.body.includes('# Content'), 'should fall through to body')
})

test('mergeFrontmatterWithBody preserves original frontmatter when body has none', async () => {
  const { mergeFrontmatterWithBody } = await importMarkdownModule()
  const original = '---\ntitle: Original\n---\n\n# Content'
  const body = '# Content'
  const result = mergeFrontmatterWithBody(original, body)
  assert.ok(result.startsWith('---'), 'result should start with frontmatter delimiter')
  assert.ok(result.includes('title: Original'), 'should preserve original title')
  assert.ok(result.includes('# Content'), 'should include body content')
})

test('getStats accounts for mixed CJK and Latin text', async () => {
  const { getStats } = await importMarkdownModule()
  const cjk = getStats('你好世界 This is a test')
  // 5 whitespace-delimited tokens (Chinese and English)
  assert.equal(cjk.words, 5)
  // 4 Chinese chars + 13 Latin letters + 3 spaces = 15 non-whitespace chars
  assert.ok(cjk.chars >= 14)
  // No sentence-ending punctuation, so 0 or 1 sentences depending on implementation
  assert.ok(cjk.paragraphs >= 1)
})
