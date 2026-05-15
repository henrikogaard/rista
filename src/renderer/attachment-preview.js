import { state, $ } from './state.js'

// ── Attachment Preview ───────────────────────────────────────────
// Handles preview of images and PDFs in the workspace.

const IMAGE_EXTS = ['.png', '.jpg', '.jpeg', '.gif', '.svg', '.webp', '.bmp']
const PDF_EXTS = ['.pdf']

export function isImageFile(path) {
  return IMAGE_EXTS.some(ext => path.toLowerCase().endsWith(ext))
}

export function isPdfFile(path) {
  return PDF_EXTS.some(ext => path.toLowerCase().endsWith(ext))
}

export function isAttachmentFile(path) {
  return isImageFile(path) || isPdfFile(path)
}

export async function renderAttachmentPreview(pane, tab) {
  const singleSurface = $(`single-surface-${pane}`)
  const splitLeft = $(`view-slot-left-${pane}`)
  const splitRight = $(`view-slot-right-${pane}`)

  const container = singleSurface || splitLeft || splitRight
  if (!container) return

  // Remove any existing attachment preview
  container.querySelectorAll('.attachment-preview').forEach(el => el.remove())

  const previewHost = document.createElement('div')
  previewHost.className = 'attachment-preview'

  if (isImageFile(tab.path)) {
    try {
      const result = await window.fjord.readFileBase64(tab.path)
      if (result.error) throw new Error(result.error)
      const base64 = result.data
      const ext = tab.path.split('.').pop().toLowerCase()
      const mime = ext === 'svg' ? 'image/svg+xml' : ext === 'webp' ? 'image/webp' : ext === 'gif' ? 'image/gif' : ext === 'png' ? 'image/png' : 'image/jpeg'
      previewHost.innerHTML = `<img src="data:${mime};base64,${base64}" alt="${tab.name}" style="max-width:100%;max-height:100%;object-fit:contain;">`
    } catch {
      previewHost.innerHTML = `<div class="attachment-error">Failed to load image</div>`
    }
  } else if (isPdfFile(tab.path)) {
    previewHost.innerHTML = `<embed src="file://${tab.path}" type="application/pdf" width="100%" height="100%" style="border:none;">`
  }

  container.appendChild(previewHost)
}

export function clearAttachmentPreview(pane) {
  const singleSurface = $(`single-surface-${pane}`)
  const splitLeft = $(`view-slot-left-${pane}`)
  const splitRight = $(`view-slot-right-${pane}`)

  const container = singleSurface || splitLeft || splitRight
  container?.querySelectorAll('.attachment-preview').forEach(el => el.remove())
}
