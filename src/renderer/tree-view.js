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
  depth = 0,
}) {
  container.innerHTML = ''

  items.forEach(item => {
    if (item.type === 'folder') {
      const isOpen = expandedPaths.has(item.path)
      const folder = document.createElement('div')
      folder.className = `tree-folder${isOpen ? ' open' : ''}`
      folder.style.paddingLeft = `${10 + depth * 14}px`
      folder.title = item.name
      folder.innerHTML = `<svg viewBox="0 0 6 10"><path d="M1 1l4 4-4 4" stroke-width="1.5" stroke="currentColor" fill="none" stroke-linecap="round"/></svg>${item.name}`

      const children = document.createElement('div')
      children.style.display = isOpen ? 'block' : 'none'
      if (item.children) {
        renderFileTree({
          items: item.children,
          container: children,
          expandedPaths,
          activePaths,
          onToggleFolder,
          onOpenFile,
          depth: depth + 1,
        })
      }

      folder.addEventListener('click', () => {
        const nextOpen = !folder.classList.contains('open')
        folder.classList.toggle('open', nextOpen)
        children.style.display = nextOpen ? 'block' : 'none'
        onToggleFolder(item.path, nextOpen)
      })

      container.appendChild(folder)
      container.appendChild(children)
      return
    }

    const file = document.createElement('div')
    file.className = 'tree-file'
    file.dataset.path = item.path
    file.style.paddingLeft = `${24 + depth * 14}px`
    file.title = item.path
    file.innerHTML = `<div class="tree-file__dot"></div>${item.name}`
    file.classList.toggle('active', activePaths.has(item.path))
    file.addEventListener('click', () => onOpenFile(item))
    container.appendChild(file)
  })
}

export function highlightTreeFiles(activePaths) {
  document.querySelectorAll('.tree-file').forEach(node => {
    node.classList.toggle('active', activePaths.has(node.dataset.path))
  })
}
