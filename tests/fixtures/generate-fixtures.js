// Benchmark fixture generator for Rísta markdown tests (Task 2.3)
const fs = require('node:fs')
const path = require('node:path')

const FIXTURES_DIR = path.resolve(__dirname)

function headingDoc(headingCount) {
  const lines = []
  lines.push('# Benchmark Document')
  lines.push('')
  lines.push('This document is auto-generated for preview performance benchmarks.')
  lines.push('')
  for (let i = 0; i < headingCount; i++) {
    const depth = (i % 6) + 1
    const h = '#'.repeat(depth)
    lines.push(`${h} Heading ${i + 1}`)
    lines.push('')
    lines.push(`Content paragraph ${i + 1}. This is some body text with **bold** and *italic* and a [link](https://example.com). Lorem ipsum dolor sit amet.`)
    lines.push('')
  }
  return lines.join('\n')
}

function tableDoc(rowCount) {
  const lines = []
  lines.push('# Tables Benchmark')
  lines.push('')
  lines.push('| Name | Value | Count | Status | Notes |')
  lines.push('|------|-------|-------|--------|-------|')
  for (let i = 0; i < rowCount; i++) {
    lines.push(`| Item ${i} | ${Math.random().toFixed(2)} | ${Math.floor(Math.random() * 100)} | ${i % 2 === 0 ? 'active' : 'inactive'} | Row number ${i} |`)
  }
  return lines.join('\n')
}

function calloutDoc(calloutCount) {
  const lines = []
  lines.push('# Callouts Benchmark')
  lines.push('')
  const types = ['note', 'info', 'tip', 'warning', 'danger']
  for (let i = 0; i < calloutCount; i++) {
    const type = types[i % types.length]
    lines.push(`> [!${type}] Callout ${i + 1}`)
    lines.push(`> This is callout number ${i + 1} with some content inside it.`)
    lines.push('')
  }
  return lines.join('\n')
}

fs.writeFileSync(path.join(FIXTURES_DIR, '500-headings.md'), headingDoc(500))
fs.writeFileSync(path.join(FIXTURES_DIR, '100-tables.md'), tableDoc(100))
fs.writeFileSync(path.join(FIXTURES_DIR, '200-callouts.md'), calloutDoc(200))
fs.writeFileSync(path.join(FIXTURES_DIR, '50-headings.md'), headingDoc(50))
fs.writeFileSync(path.join(FIXTURES_DIR, '10-tables.md'), tableDoc(10))

console.log('Fixtures generated in', FIXTURES_DIR)
