import { unified } from 'unified'
import remarkParse from 'remark-parse'
import remarkGfm from 'remark-gfm'
import remarkRehype from 'remark-rehype'
import rehypeStringify from 'rehype-stringify'

const processor = unified()
  .use(remarkParse)
  .use(remarkGfm)
  .use(remarkRehype, { allowDangerousHtml: true })
  .use(rehypeStringify, { allowDangerousHtml: true })

export async function renderMarkdown(markdown) {
  const result = await processor.process(markdown)
  return String(result)
}

// Extract headings for ToC
export function extractHeadings(markdown) {
  const headings = []
  const lines = markdown.split('\n')
  for (const line of lines) {
    const m = line.match(/^(#{1,3})\s+(.+)/)
    if (m) {
      headings.push({ level: m[1].length, text: m[2].trim() })
    }
  }
  return headings
}

// Word / char / paragraph count
export function getStats(markdown) {
  const text = markdown.replace(/```[\s\S]*?```/g, '').replace(/`[^`]+`/g, '').replace(/[#*_~\[\]]/g, '')
  const words = text.trim() ? text.trim().split(/\s+/).length : 0
  const chars = text.replace(/\s/g, '').length
  const paragraphs = markdown.split(/\n\n+/).filter(p => p.trim()).length
  const readMin = Math.ceil(words / 200) || 0
  return { words, chars, paragraphs, readMin }
}
