import { getSettings } from './settings.js'

import { $ } from './state.js'
const DAY_NAMES = ['Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa', 'Su']
const MONTH_NAMES = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December',
]

let _currentYear = new Date().getFullYear()
let _currentMonth = new Date().getMonth()
let _dailyNotesMap = new Map()
let _onDateClick = null

function pad(n) { return String(n).padStart(2, '0') }

function toDateKey(y, m, d) {
  return `${y}-${pad(m + 1)}-${pad(d)}`
}

function todayKey() {
  const now = new Date()
  return toDateKey(now.getFullYear(), now.getMonth(), now.getDate())
}

/**
 * Scan the daily notes folder and return a Map of YYYY-MM-DD => filepath.
 */
export async function getDailyNotesMap(folderPath) {
  const map = new Map()
  if (!folderPath || !window.fjord) return map
  const settings = getSettings()
  const dailyFolder = settings.dailyNotesFolder || 'daily'
  const dirPath = `${folderPath}/${dailyFolder}`

  try {
    const tree = await window.fjord.readFolder(dirPath)
    for (const item of tree) {
      if (item.type === 'file' && /^\d{4}-\d{2}-\d{2}\.md$/.test(item.name)) {
        const dateKey = item.name.replace(/\.md$/, '')
        map.set(dateKey, item.path)
      }
    }
  } catch {
    // Folder may not exist yet
  }
  return map
}

/**
 * Render the calendar grid into an existing container element.
 */
export function renderCalendar(year, month, dailyNotes, container) {
  _currentYear = year
  _currentMonth = month
  _dailyNotesMap = dailyNotes || new Map()

  const today = todayKey()

  // First day of month (0=Sun, adjust to Mon-based)
  const firstDay = new Date(year, month, 1).getDay()
  const startOffset = (firstDay + 6) % 7 // Monday = 0
  const daysInMonth = new Date(year, month + 1, 0).getDate()

  // Build header
  const headerHtml = `
    <div class="calendar-header">
      <div class="calendar-nav calendar-nav--prev" data-calendar-nav="prev" role="button" tabindex="0" aria-label="Previous month">&lsaquo;</div>
      <span class="calendar-header__title">${MONTH_NAMES[month]} ${year}</span>
      <div class="calendar-nav calendar-nav--next" data-calendar-nav="next" role="button" tabindex="0" aria-label="Next month">&rsaquo;</div>
    </div>
  `

  // Weekday headers
  const weekdaysHtml = `
    <div class="calendar-weekdays">
      ${DAY_NAMES.map(d => `<span class="calendar-weekday">${d}</span>`).join('')}
    </div>
  `

  // Day cells
  const totalCells = Math.ceil((startOffset + daysInMonth) / 7) * 7
  let cellsHtml = ''
  for (let i = 0; i < totalCells; i++) {
    const dayNum = i - startOffset + 1
    if (dayNum < 1 || dayNum > daysInMonth) {
      cellsHtml += '<div class="calendar-cell calendar-cell--empty"></div>'
    } else {
      const key = toDateKey(year, month, dayNum)
      const isToday = key === today
      const hasNote = _dailyNotesMap.has(key)
      const classes = [
        'calendar-cell',
        isToday ? 'calendar-cell--today' : '',
        hasNote ? 'calendar-cell--has-note' : '',
      ].filter(Boolean).join(' ')
      cellsHtml += `<div class="${classes}" data-calendar-date="${key}" role="button" tabindex="0">${dayNum}</div>`
    }
  }

  const gridHtml = `<div class="calendar-grid">${cellsHtml}</div>`

  container.innerHTML = headerHtml + weekdaysHtml + gridHtml
}

/**
 * Build the calendar panel element. Returns the DOM node.
 * @param {object} opts
 * @param {Function} opts.onDateClick - Called with (dateKey: string) when a date is clicked
 * @param {string} opts.folderPath - Current project folder path
 */
export function buildCalendarPanel({ onDateClick, folderPath } = {}) {
  _onDateClick = onDateClick || null
  const panel = document.createElement('div')
  panel.className = 'calendar-panel'
  panel.id = 'calendar-panel'

  // Initial render with today's month
  const now = new Date()
  _currentYear = now.getFullYear()
  _currentMonth = now.getMonth()

  // Render empty first, then populate async
  renderCalendar(_currentYear, _currentMonth, new Map(), panel)

  // Load daily notes and re-render
  if (folderPath) {
    getDailyNotesMap(folderPath).then(map => {
      _dailyNotesMap = map
      renderCalendar(_currentYear, _currentMonth, map, panel)
    })
  }

  // Event delegation
  panel.addEventListener('click', (e) => {
    const nav = e.target.closest('[data-calendar-nav]')
    if (nav) {
      const dir = nav.dataset.calendarNav
      if (dir === 'prev') {
        _currentMonth--
        if (_currentMonth < 0) { _currentMonth = 11; _currentYear-- }
      } else {
        _currentMonth++
        if (_currentMonth > 11) { _currentMonth = 0; _currentYear++ }
      }
      renderCalendar(_currentYear, _currentMonth, _dailyNotesMap, panel)
      return
    }

    const cell = e.target.closest('[data-calendar-date]')
    if (cell && _onDateClick) {
      _onDateClick(cell.dataset.calendarDate)
    }
  })

  panel.addEventListener('keydown', (e) => {
    if (e.key !== 'Enter' && e.key !== ' ') return
    const target = e.target.closest('[data-calendar-nav], [data-calendar-date]')
    if (target) {
      e.preventDefault()
      target.click()
    }
  })

  return panel
}

/**
 * Refresh the daily notes map and re-render an existing calendar panel.
 */
export async function refreshCalendarPanel(folderPath) {
  const panel = $('calendar-panel')
  if (!panel) return
  _dailyNotesMap = await getDailyNotesMap(folderPath)
  renderCalendar(_currentYear, _currentMonth, _dailyNotesMap, panel)
}
