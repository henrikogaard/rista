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
    if (item.submenu && item.submenu.length) {
      row.classList.add('context-menu-item--has-submenu')
      const caret = el('span', 'context-menu-item__caret', '›')
      row.appendChild(caret)
      row.addEventListener('mouseenter', () => {
        const rect = row.getBoundingClientRect()
        showSubmenu(rect.right - 4, rect.top, item.submenu, menu)
      })
      row.addEventListener('click', (e) => {
        e.stopPropagation()
        const rect = row.getBoundingClientRect()
        showSubmenu(rect.right - 4, rect.top, item.submenu, menu)
      })
    } else if (item.action) {
      row.addEventListener('click', (e) => {
        e.stopPropagation()
        closeContextMenu()
        item.action()
      })
    }
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

let activeSubmenu = null
function closeSubmenu() {
  if (activeSubmenu) { activeSubmenu.remove(); activeSubmenu = null }
}

function showSubmenu(x, y, items, parentMenu) {
  closeSubmenu()
  const sub = el('div', 'context-menu context-menu--submenu')
  sub.style.left = `${x}px`
  sub.style.top = `${y}px`
  for (const item of items) {
    if (item.separator) { sub.appendChild(el('div', 'context-menu-separator')); continue }
    const row = el('div', 'context-menu-item', item.label)
    if (item.action) {
      row.addEventListener('click', (e) => {
        e.stopPropagation()
        closeContextMenu()
        item.action()
      })
    }
    sub.appendChild(row)
  }
  document.body.appendChild(sub)
  activeSubmenu = sub
  requestAnimationFrame(() => {
    const rect = sub.getBoundingClientRect()
    if (rect.right > window.innerWidth) {
      sub.style.left = `${window.innerWidth - rect.width - 8}px`
    }
    if (rect.bottom > window.innerHeight) {
      sub.style.top = `${window.innerHeight - rect.height - 8}px`
    }
  })
  // Close submenu when user moves to a different parent row
  parentMenu?.addEventListener('mouseleave', () => {
    // Defer so a quick movement into the submenu doesn't kill it
    setTimeout(() => {
      if (activeSubmenu && !activeSubmenu.matches(':hover')) closeSubmenu()
    }, 100)
  }, { once: true })
}

export function closeContextMenu() {
  closeSubmenu()
  if (activeMenu) {
    activeMenu.remove()
    activeMenu = null
  }
}
