const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')

async function importTableEditor() {
  const source = fs.readFileSync(path.join(root, 'src/renderer/table-editor.js'), 'utf8')
  const modPath = path.join(os.tmpdir(), `rista-table-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  fs.writeFileSync(modPath, source)
  const mod = await import(`file://${modPath}`)
  fs.rmSync(modPath, { force: true })
  return mod
}

test('isInsideTable detects pipe rows', async () => {
  const { isInsideTable } = await importTableEditor()
  const mockState = {
    doc: {
      lineAt(pos) {
        if (pos === 0) return { number: 1, text: '| a | b |' }
        if (pos === 10) return { number: 2, text: '| 1 | 2 |' }
        if (pos === 3) return { number: 1, text: '# Not a table' }
        return { number: 1, text: '' }
      },
    },
  }
  assert.ok(isInsideTable(mockState, 0), 'pipe row detected')
  assert.ok(isInsideTable(mockState, 10), 'second pipe row detected')
  assert.ok(!isInsideTable(mockState, 3), 'non-table row rejected')
})

test('getTableRange detects full table boundaries', async () => {
  const { getTableRange } = await importTableEditor()
  const lines = [
    '| h1 | h2 |',
    '| --- | --- |',
    '| a | b |',
    '',
    '# Not table',
  ]
  const mockState = {
    doc: {
      lineAt(pos) {
        let acc = 0
        for (let i = 0; i < lines.length; i++) {
          const lineLen = lines[i].length + 1
          if (pos < acc + lineLen || i === lines.length - 1) {
            return { number: i + 1, from: acc, to: acc + lines[i].length, text: lines[i] }
          }
          acc += lineLen
        }
        return { number: lines.length, from: acc, to: acc, text: '' }
      },
      line(num) {
        let acc = 0
        for (let i = 0; i < lines.length; i++) {
          if (i + 1 === num) return { number: num, from: acc, to: acc + lines[i].length, text: lines[i] }
          acc += lines[i].length + 1
        }
        return { number: num, from: acc, to: acc, text: '' }
      },
      get lines() { return lines.length },
    },
  }
  const range = getTableRange(mockState, 2)
  assert.ok(range, 'table range found')
  assert.ok(range.fromLine >= 1, 'starts at line 1 or earlier')
  assert.ok(range.toLine >= 3, 'ends at line 3 or later')
})

test('parseMarkdownTable parses valid table', async () => {
  const { parseMarkdownTable } = await importTableEditor()
  const text = '| Name | Age |\n| --- | --- |\n| Alice | 30 |\n| Bob | 25 |'
  const table = parseMarkdownTable(text)
  assert.ok(table, 'table parsed')
  assert.deepEqual(table.headers, ['Name', 'Age'])
  assert.deepEqual(table.alignments, ['left', 'left'])
  assert.equal(table.rows.length, 2)
  assert.deepEqual(table.rows[0], ['Alice', '30'])
  assert.deepEqual(table.rows[1], ['Bob', '25'])
})

test('parseMarkdownTable returns null for non-table', async () => {
  const { parseMarkdownTable } = await importTableEditor()
  assert.equal(parseMarkdownTable('Just text'), null)
  assert.equal(parseMarkdownTable(''), null)
})

test('serializeTable produces valid markdown', async () => {
  const { parseMarkdownTable, serializeTable } = await importTableEditor()
  const original = '| A | B |\n| --- | --- |\n| 1 | 2 |'
  const table = parseMarkdownTable(original)
  const serialized = serializeTable(table)
  // Serialized pads cells to header width: | A   | B   |
  assert.ok(serialized.includes('A'), 'contains header A')
  assert.ok(serialized.includes('B'), 'contains header B')
  assert.ok(serialized.includes('1'), 'contains data 1')
  assert.ok(serialized.includes('2'), 'contains data 2')
  // Should include the divider row
  assert.ok(serialized.includes('---'), 'contains divider')
  assert.ok(serialized.length > 10, 'produced reasonable output')
})

test('parseMarkdownTable handles alignment', async () => {
  const { parseMarkdownTable } = await importTableEditor()
  const text = '| Left | Center | Right |\n| :--- | :---: | ---: |\n| a | b | c |'
  const table = parseMarkdownTable(text)
  assert.deepEqual(table.alignments, ['left', 'center', 'right'])
})

test('serializeTable with alignment preserves alignments', async () => {
  const { parseMarkdownTable, serializeTable } = await importTableEditor()
  const text = '| Left | Center |\n| :--- | :---: |\n| a | b |'
  const table = parseMarkdownTable(text)
  const serialized = serializeTable(table)
  const reparsed = parseMarkdownTable(serialized)
  assert.deepEqual(reparsed.alignments, ['left', 'center'])
})
