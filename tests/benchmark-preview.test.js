// Preview performance benchmarks (Task 2.3)
// Measures renderMarkdown() time for large documents with headings, tables, and callouts.
// These are not exhaustive benchmarks — they detect severe regressions.

const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')
const FIXTURES_DIR = path.join(root, 'tests/fixtures')

const BENCHMARK_BUDGETS = {
  '50-headings.md': 100,      // 50 headings + content: < 100ms
  '500-headings.md': 500,     // 500 headings: < 500ms
  '10-tables.md': 100,        // 10 tables: < 100ms
  '100-tables.md': 500,       // 100 tables: < 500ms
  '200-callouts.md': 500,     // 200 callouts: < 500ms
}

async function importMarkdownModule() {
  const source = fs.readFileSync(path.join(root, 'src/renderer/markdown.js'), 'utf8')
  // Write inside src/renderer/ so relative imports (./state.js) resolve.
  const modulePath = path.join(root, 'src/renderer', `.tmp-bench-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  fs.writeFileSync(modulePath, source)
  try {
    return await import(`file://${modulePath}`)
  } finally {
    fs.rmSync(modulePath, { force: true })
  }
}

test('benchmark: small heading document renders within budget', async () => {
  const { renderMarkdown } = await importMarkdownModule()
  const content = fs.readFileSync(path.join(FIXTURES_DIR, '50-headings.md'), 'utf8')

  const start = performance.now()
  await renderMarkdown(content)
  const elapsed = performance.now() - start

  assert.ok(elapsed < BENCHMARK_BUDGETS['50-headings.md'],
    `50-headings.md took ${elapsed.toFixed(1)}ms (budget: ${BENCHMARK_BUDGETS['50-headings.md']}ms)`)
})

test('benchmark: large heading document renders within budget', async () => {
  const { renderMarkdown } = await importMarkdownModule()
  const content = fs.readFileSync(path.join(FIXTURES_DIR, '500-headings.md'), 'utf8')

  const start = performance.now()
  await renderMarkdown(content)
  const elapsed = performance.now() - start

  assert.ok(elapsed < BENCHMARK_BUDGETS['500-headings.md'],
    `500-headings.md took ${elapsed.toFixed(1)}ms (budget: ${BENCHMARK_BUDGETS['500-headings.md']}ms)`)
})

test('benchmark: table-heavy document renders within budget', async () => {
  const { renderMarkdown } = await importMarkdownModule()
  const content = fs.readFileSync(path.join(FIXTURES_DIR, '10-tables.md'), 'utf8')

  const start = performance.now()
  await renderMarkdown(content)
  const elapsed = performance.now() - start

  assert.ok(elapsed < BENCHMARK_BUDGETS['10-tables.md'],
    `10-tables.md took ${elapsed.toFixed(1)}ms (budget: ${BENCHMARK_BUDGETS['10-tables.md']}ms)`)
})

test('benchmark: large table document renders within budget', async () => {
  const { renderMarkdown } = await importMarkdownModule()
  const content = fs.readFileSync(path.join(FIXTURES_DIR, '100-tables.md'), 'utf8')

  const start = performance.now()
  await renderMarkdown(content)
  const elapsed = performance.now() - start

  assert.ok(elapsed < BENCHMARK_BUDGETS['100-tables.md'],
    `100-tables.md took ${elapsed.toFixed(1)}ms (budget: ${BENCHMARK_BUDGETS['100-tables.md']}ms)`)
})

test('benchmark: callout-heavy document renders within budget', async () => {
  const { renderMarkdown } = await importMarkdownModule()
  const content = fs.readFileSync(path.join(FIXTURES_DIR, '200-callouts.md'), 'utf8')

  const start = performance.now()
  await renderMarkdown(content)
  const elapsed = performance.now() - start

  assert.ok(elapsed < BENCHMARK_BUDGETS['200-callouts.md'],
    `200-callouts.md took ${elapsed.toFixed(1)}ms (budget: ${BENCHMARK_BUDGETS['200-callouts.md']}ms)`)
})

test('benchmark fixtures exist and are non-empty', async () => {
  for (const name of Object.keys(BENCHMARK_BUDGETS)) {
    const content = fs.readFileSync(path.join(FIXTURES_DIR, name), 'utf8')
    assert.ok(content.length > 100, `${name} should be non-trivial`)
  }
})
