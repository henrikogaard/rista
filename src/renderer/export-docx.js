import { Document, Packer, Paragraph, TextRun, HeadingLevel, AlignmentType } from 'docx'
import { getFocusedTab, stripMarkdownExtension } from './state.js'

export async function exportToDocx() {
  const tab = getFocusedTab()
  if (!tab) return

  const lines = (tab.content || '').split('\n')
  const children = []

  for (const line of lines) {
    const h1 = line.match(/^#\s+(.+)/)
    const h2 = line.match(/^##\s+(.+)/)
    const h3 = line.match(/^###\s+(.+)/)
    const bullet = line.match(/^[-*]\s+(.+)/)
    const numbered = line.match(/^\d+\.\s+(.+)/)

    if (h1) {
      children.push(new Paragraph({ text: h1[1], heading: HeadingLevel.HEADING_1 }))
    } else if (h2) {
      children.push(new Paragraph({ text: h2[1], heading: HeadingLevel.HEADING_2 }))
    } else if (h3) {
      children.push(new Paragraph({ text: h3[1], heading: HeadingLevel.HEADING_3 }))
    } else if (bullet) {
      children.push(new Paragraph({ text: bullet[1], bullet: { level: 0 } }))
    } else if (numbered) {
      children.push(new Paragraph({ text: numbered[1], numbering: { reference: 'default-numbering', level: 0 } }))
    } else if (line.trim() === '') {
      children.push(new Paragraph({ text: '' }))
    } else {
      const runs = parseInlineRuns(line)
      children.push(new Paragraph({ children: runs }))
    }
  }

  const doc = new Document({
    numbering: {
      config: [{
        reference: 'default-numbering',
        levels: [{ level: 0, format: 'decimal', text: '%1.', alignment: AlignmentType.LEFT }],
      }],
    },
    sections: [{ children }],
  })

  const blob = await Packer.toBlob(doc)
  const arrayBuffer = await blob.arrayBuffer()
  const bytes = new Uint8Array(arrayBuffer)
  let binary = ''
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i])
  }
  const base64 = btoa(binary)

  const fileName = stripMarkdownExtension(tab.name)
  await window.fjord.saveDocx(base64, fileName)
}

function parseInlineRuns(text) {
  const runs = []
  const regex = /(\*\*(.+?)\*\*|\*(.+?)\*|`(.+?)`|([^*`]+))/g
  let match
  while ((match = regex.exec(text)) !== null) {
    if (match[2]) {
      runs.push(new TextRun({ text: match[2], bold: true }))
    } else if (match[3]) {
      runs.push(new TextRun({ text: match[3], italics: true }))
    } else if (match[4]) {
      runs.push(new TextRun({ text: match[4], font: 'Courier New', size: 20 }))
    } else if (match[5]) {
      runs.push(new TextRun({ text: match[5] }))
    }
  }
  return runs.length ? runs : [new TextRun({ text })]
}
