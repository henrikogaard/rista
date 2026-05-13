import { el } from './state.js'

let activeMenu = null

export function showContextMenu(x, y, items) {
  closeContextMenu()

  const menu = el('div', 'context-menu')
  menu.style.left = `${x}px`
  menu.style.top = `${y}px`

  for (const item of items) {
    if (item.separator) {
      menu.appendChild(el('div', 'context-menu-separator'))
      continue
    }
    const row = el('div', 'context-menu-item', item.label)
    row.addEventListener('click', (e) => {
      e.stopPropagation()
      closeContextMenu()
      item.action()
    })
    menu.appendChild(row)
  }

  document.body.appendChild(menu)
  activeMenu = menu

  // Reposition if menu goes off-screen
  requestAnimationFrame(() => {
    const rect = menu.getBoundingClientRect()
    if (rect.right > window.innerWidth) {
      menu.style.left = `${window.innerWidth - rect.width - 8}px`
    }
    if (rect.bottom > window.innerHeight) {
      menu.style.top = `${window.innerHeight - rect.height - 8}px`
    }
  })

  const closeOnClick = (e) => {
    if (!menu.contains(e.target)) {
      closeContextMenu()
      document.removeEventListener('click', closeOnClick)
    }
  }
  setTimeout(() => document.addEventListener('click', closeOnClick), 0)
}

export function closeContextMenu() {
  if (activeMenu) {
    activeMenu.remove()
    activeMenu = null
  }
}
