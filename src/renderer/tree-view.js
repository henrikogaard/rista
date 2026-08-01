import { markdownFileIcon, folderIcon } from './icons.js'

const TREE_ROOT_INDENT = 8
const TREE_DEPTH_INDENT = 10
const TREE_FILE_INDENT = 20
const LAZY_RENDER_DEPTH = 3  // Folders deeper than this defer child rendering until first expansion (Task 2.5)

export function collectFolderPaths(items, result = []) {
  items.forEach(item => {
    if (item.type === 'folder') {
      result.push(item.path)
      if (item.children) collectFolderPaths(item.children, result)
    }
  })
  return result
}

export function renderFileTree({
  items,
  container,
  expandedPaths,
  activePaths,
  onToggleFolder,
  onOpenFile,
  onOpenFilePreview,
  onContextMenu,
  depth = 0,
}) {
  container.innerHTML = ''

  items.forEach(item => {
    if (item.type === 'folder') {
      const isOpen = expandedPaths.has(item.path)
      const folder = document.createElement('div')
      folder.className = `tree-folder${isOpen ? ' open' : ''}`
      folder.style.paddingLeft = `${TREE_ROOT_INDENT + depth * TREE_DEPTH_INDENT}px`
      folder.title = item.name
      folder.dataset.path = item.path
      folder.dataset.type = 'folder'
      folder.innerHTML = `<span class="tree-folder__chevron"><svg viewBox="0 0 6 10"><path d="M1 1l4 4-4 4" stroke-width="1.5" stroke="currentColor" fill="none" stroke-linecap="round"/></svg></span><span class="tree-folder__icon">${folderIcon()}</span><span class="tree-folder__name">${item.name}</span>${item.children?.length ? `<span class="tree-folder__count">${item.children.length}</span>` : ''}`

      const children = document.createElement('div')
      children.className = 'tree-children'
      children.style.display = isOpen ? 'block' : 'none'
      if (isOpen || depth < LAZY_RENDER_DEPTH) {
        if (item.children) {
          renderFileTree({
            items: item.children,
            container: children,
            expandedPaths,
            activePaths,
            onToggleFolder,
            onOpenFile,
            onOpenFilePreview,
            onContextMenu,
            depth: depth + 1,
          })
        }
      }

      folder.addEventListener('click', () => {
        const nextOpen = !folder.classList.contains('open')
        folder.classList.toggle('open', nextOpen)
        children.style.display = nextOpen ? 'block' : 'none'
        // Lazy render deep branches on first expansion
        if (nextOpen && depth >= LAZY_RENDER_DEPTH && item.children && !children.children.length) {
          renderFileTree({
            items: item.children,
            container: children,
            expandedPaths,
            activePaths,
            onToggleFolder,
            onOpenFile,
            onOpenFilePreview,
            onContextMenu,
            depth: depth + 1,
          })
        }
        onToggleFolder(item.path, nextOpen)
      })

      if (onContextMenu) {
        folder.addEventListener('contextmenu', (event) => {
          event.preventDefault()
          event.stopPropagation()
          onContextMenu(item, event)
        })
      }

      container.appendChild(folder)
      container.appendChild(children)
      return
    }

    const file = document.createElement('div')
    file.className = 'tree-file'
    file.dataset.path = item.path
    file.dataset.type = 'file'
    file.style.paddingLeft = `${TREE_FILE_INDENT + depth * TREE_DEPTH_INDENT}px`
    file.title = item.path
    file.innerHTML = `<span class="tree-file__icon">${markdownFileIcon()}</span>${item.name}`
    file.classList.toggle('active', activePaths.has(item.path))
    let clickTimer = null
    file.addEventListener('click', () => {
      if (clickTimer) return
      clickTimer = setTimeout(() => {
        clickTimer = null
        if (onOpenFilePreview) onOpenFilePreview(item)
        else onOpenFile(item)
      }, 200)
    })
    file.addEventListener('dblclick', () => {
      if (clickTimer) { clearTimeout(clickTimer); clickTimer = null }
      onOpenFile(item)
    })
    if (onContextMenu) {
      file.addEventListener('contextmenu', (event) => {
        event.preventDefault()
        event.stopPropagation()
        onContextMenu(item, event)
      })
    }
    container.appendChild(file)
  })
}

export function highlightTreeFiles(activePaths) {
  document.querySelectorAll('.tree-file').forEach(node => {
    node.classList.toggle('active', activePaths.has(node.dataset.path))
  })
}
