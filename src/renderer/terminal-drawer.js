import { state, $, getFocusedTab } from './state.js'

// ── Terminal Drawer ──────────────────────────────────────────────
// A compact workspace shell for running project commands.

let commandHistory = []
let historyIndex = -1
let terminalBusy = false
let terminalCwd = ''
let previousTerminalCwd = ''
let terminalWorkspacePath = ''
let shellInfo = { name: 'shell', path: '' }

function folderName(folderPath) {
  return String(folderPath || '').split(/[\\/]/).filter(Boolean).pop() || '~'
}

function directoryPath(filePath = '') {
  const normalized = normalizeTerminalPath(filePath)
  const index = normalized.lastIndexOf('/')
  return index >= 0 ? normalized.slice(0, index) : ''
}

function formatDuration(ms) {
  const value = Number(ms || 0)
  if (value < 1000) return `${value}ms`
  return `${(value / 1000).toFixed(1)}s`
}

function compactPath(path) {
  const normalized = normalizeTerminalPath(path || '')
  if (!normalized) return '~'
  const workspace = normalizeTerminalPath(state.folderPath || '')
  if (workspace && normalized === workspace) return folderName(workspace)
  if (workspace && normalized.startsWith(`${workspace}/`)) {
    return `${folderName(workspace)}/${normalized.slice(workspace.length + 1)}`
  }
  const parts = normalized.split('/').filter(Boolean)
  if (parts.length <= 2) return normalized
  return `…/${parts.slice(-2).join('/')}`
}

function normalizeTerminalPath(path = '') {
  const raw = String(path || '').replace(/\\/g, '/')
  const isAbsolute = raw.startsWith('/')
  const parts = raw.split('/')
  const out = []
  for (const part of parts) {
    if (!part || part === '.') continue
    if (part === '..') out.pop()
    else out.push(part)
  }
  return `${isAbsolute ? '/' : ''}${out.join('/')}`
}

function resolveTerminalCwd() {
  const workspace = normalizeTerminalPath(state.folderPath || '')
  if (workspace && terminalWorkspacePath !== workspace) {
    terminalWorkspacePath = workspace
    terminalCwd = workspace
    previousTerminalCwd = ''
  }
  if (terminalCwd) return terminalCwd
  if (workspace) return workspace
  return directoryPath(getFocusedTab()?.path || '')
}

function resolveCdTarget(rawTarget) {
  const current = resolveTerminalCwd()
  const target = unquoteShellPath(rawTarget || '')
  if (!target || target === '~') return normalizeTerminalPath(state.folderPath || current)
  if (target === '-') return previousTerminalCwd || current
  if (target.startsWith('/')) return normalizeTerminalPath(target)
  if (target.startsWith('~/')) {
    const root = normalizeTerminalPath(state.folderPath || current)
    return normalizeTerminalPath(`${root}/${target.slice(2)}`)
  }
  return normalizeTerminalPath(`${current}/${target}`)
}

function unquoteShellPath(value) {
  const text = String(value || '').trim()
  if (
    (text.startsWith('"') && text.endsWith('"')) ||
    (text.startsWith("'") && text.endsWith("'"))
  ) {
    return text.slice(1, -1)
  }
  return text.replace(/\\ /g, ' ')
}

async function syncShellInfo() {
  try {
    const nextShellInfo = await window.fjord.getShellInfo()
    shellInfo = nextShellInfo && typeof nextShellInfo.name === 'string'
      ? {
          name: nextShellInfo.name || 'shell',
          path: typeof nextShellInfo.path === 'string' ? nextShellInfo.path : '',
        }
      : { name: 'shell', path: '' }
  } catch {
    shellInfo = { name: 'shell', path: '' }
  }
  const el = $('terminal-shell')
  if (el) {
    el.textContent = shellInfo.name
    el.title = shellInfo.path || shellInfo.name
  }
}

export function buildTerminalDrawer() {
  return `
    <div class="terminal-drawer" id="terminal-drawer" data-experimental-feature="featureTerminal">
      <div class="terminal-drawer__header">
        <div class="terminal-drawer__identity">
          <div class="terminal-drawer__title">Terminal</div>
          <div class="terminal-drawer__prompt-chip">
            <span class="terminal-drawer__cwd" id="terminal-cwd">~</span>
            <span class="terminal-shell" id="terminal-shell">shell</span>
          </div>
        </div>
        <div class="terminal-drawer__path" id="terminal-path"></div>
        <div class="terminal-drawer__status" id="terminal-status">Ready</div>
        <div class="terminal-drawer__action" id="terminal-clear-btn" title="Clear terminal" role="button" tabindex="0">Clear</div>
        <div class="terminal-drawer__close" id="terminal-close-btn" title="Close terminal" role="button" tabindex="0">
          <svg viewBox="0 0 16 16" width="12" height="12"><path d="M3.5 3.5l9 9M12.5 3.5l-9 9" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round"/></svg>
        </div>
      </div>
      <div class="terminal-drawer__output" id="terminal-output">
        <div class="terminal-drawer__empty">Commands run in the current workspace folder. Use cd to move, ↑/↓ for history.</div>
      </div>
      <div class="terminal-quick-commands" aria-label="Quick terminal commands">
        <div class="terminal-quick-command" data-terminal-command="pwd" role="button" tabindex="0">pwd</div>
        <div class="terminal-quick-command" data-terminal-command="ls -la" role="button" tabindex="0">ls -la</div>
        <div class="terminal-quick-command" data-terminal-command="git status --short --branch" role="button" tabindex="0">git</div>
        <div class="terminal-quick-command" data-terminal-command="npm test" role="button" tabindex="0">test</div>
        <div class="terminal-quick-command" data-terminal-command="npm run build" role="button" tabindex="0">build</div>
      </div>
      <div class="terminal-drawer__input-row">
        <span class="terminal-drawer__prompt" id="terminal-prompt">$</span>
        <input type="text" class="terminal-drawer__input" id="terminal-input" placeholder="Run a workspace command…" spellcheck="false" autocomplete="off">
      </div>
    </div>
  `
}

export function toggleTerminalDrawer() {
  const drawer = $('terminal-drawer')
  if (!drawer) return
  drawer.classList.toggle('open')
  if (drawer.classList.contains('open')) {
    $('terminal-input')?.focus()
    syncTerminalPath()
    syncShellInfo()
  }
}

export function openTerminalDrawer() {
  const drawer = $('terminal-drawer')
  if (!drawer) return
  drawer.classList.add('open')
  $('terminal-input')?.focus()
  syncTerminalPath()
  syncShellInfo()
}

export function closeTerminalDrawer() {
  const drawer = $('terminal-drawer')
  if (!drawer) return
  drawer.classList.remove('open')
}

function syncTerminalPath() {
  const cwd = resolveTerminalCwd()
  const pathEl = $('terminal-path')
  const cwdEl = $('terminal-cwd')
  const promptEl = $('terminal-prompt')
  if (pathEl) pathEl.textContent = cwd || '~'
  if (cwdEl) {
    cwdEl.textContent = compactPath(cwd)
    cwdEl.title = cwd || '~'
  }
  if (promptEl) promptEl.textContent = `${compactPath(cwd)} $`
}

function appendTerminalCommand(command, cwd) {
  const output = $('terminal-output')
  if (!output) return
  output.querySelector('.terminal-drawer__empty')?.remove()
  const entry = document.createElement('div')
  entry.className = 'terminal-entry terminal-entry--command'
  entry.innerHTML = `
    <span class="terminal-entry__cwd">${escapeHtml(compactPath(cwd))}</span>
    <span class="terminal-entry__shell">${escapeHtml(shellInfo.name)}</span>
    <span class="terminal-entry__command">$ ${escapeHtml(command)}</span>
  `
  output.appendChild(entry)
  output.scrollTop = output.scrollHeight
}

export function appendTerminalOutput(text, type = 'stdout') {
  const output = $('terminal-output')
  if (!output) return
  output.querySelector('.terminal-drawer__empty')?.remove()
  const line = document.createElement('div')
  line.className = type === 'meta'
    ? 'terminal-entry terminal-line terminal-line__meta'
    : `terminal-entry terminal-line terminal-line--${type}`
  if (type === 'stdout' || type === 'stderr' || type === 'error') {
    line.innerHTML = ansiToHtml(text)
  } else {
    line.textContent = text
  }
  output.appendChild(line)
  output.scrollTop = output.scrollHeight
}

export function clearTerminalOutput() {
  const output = $('terminal-output')
  if (output) output.innerHTML = '<div class="terminal-drawer__empty">Commands run in the current workspace folder. Use cd to move, ↑/↓ for history.</div>'
}

function setTerminalBusy(nextBusy) {
  terminalBusy = nextBusy
  const drawer = $('terminal-drawer')
  const status = $('terminal-status')
  const input = $('terminal-input')
  drawer?.classList.toggle('is-running', nextBusy)
  if (status) status.textContent = nextBusy ? 'Running' : 'Ready'
  if (input) input.disabled = nextBusy
}

function handleInternalTerminalCommand(command) {
  if (command === 'clear') {
    clearTerminalOutput()
    return true
  }
  const cdMatch = command.match(/^cd(?:\s+(.+))?$/)
  if (!cdMatch) return false
  const current = resolveTerminalCwd()
  const next = resolveCdTarget(cdMatch[1] || '')
  previousTerminalCwd = current
  terminalCwd = next
  syncTerminalPath()
  appendTerminalOutput(`cwd ${next || '~'}`, 'meta')
  return true
}

export function handleTerminalInput() {
  const input = $('terminal-input')
  if (!input) return

  input.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      if (terminalBusy) return
      const command = input.value.trim()
      if (!command) return

      syncTerminalPath()
      const cwd = resolveTerminalCwd() || undefined
      appendTerminalCommand(command, cwd)
      commandHistory.push(command)
      historyIndex = commandHistory.length
      input.value = ''

      if (handleInternalTerminalCommand(command)) return

      try {
        setTerminalBusy(true)
        const result = await window.fjord.runTerminalCommand(command, cwd)
        const code = Number.isFinite(Number(result?.code)) ? Number(result.code) : -1
        if (result?.stdout) appendTerminalOutput(result.stdout, 'stdout')
        if (result?.stderr) appendTerminalOutput(result.stderr, 'stderr')
        if (result?.error) appendTerminalOutput(result.error, 'error')
        if (code !== 0 && !result?.stdout && !result?.stderr) {
          appendTerminalOutput(`Exit code: ${code}`, 'error')
        }
        appendTerminalOutput(`exit ${code} · ${formatDuration(result.durationMs)}`, code === 0 ? 'meta' : 'error')
      } catch (err) {
        appendTerminalOutput(err.message, 'error')
      } finally {
        setTerminalBusy(false)
        input.focus()
      }
      return
    }

    if (e.key === 'ArrowUp') {
      e.preventDefault()
      if (historyIndex > 0) {
        historyIndex--
        input.value = commandHistory[historyIndex]
      }
      return
    }

    if (e.key === 'ArrowDown') {
      e.preventDefault()
      if (historyIndex < commandHistory.length - 1) {
        historyIndex++
        input.value = commandHistory[historyIndex]
      } else {
        historyIndex = commandHistory.length
        input.value = ''
      }
      return
    }

    if (e.key === 'Escape') {
      closeTerminalDrawer()
    }
  })

  document.querySelectorAll('[data-terminal-command]').forEach((chip) => {
    chip.addEventListener('click', () => {
      input.value = chip.dataset.terminalCommand || ''
      input.focus()
    })
    chip.addEventListener('keydown', (e) => {
      if (e.key !== 'Enter' && e.key !== ' ') return
      e.preventDefault()
      input.value = chip.dataset.terminalCommand || ''
      input.focus()
    })
  })

  $('terminal-close-btn')?.addEventListener('click', closeTerminalDrawer)
  $('terminal-clear-btn')?.addEventListener('click', clearTerminalOutput)
}

function ansiToHtml(text) {
  const input = String(text || '')
  const regex = /\x1b\[([0-9;]*)m/g
  let html = ''
  let lastIndex = 0
  let classes = []
  let match

  while ((match = regex.exec(input)) !== null) {
    html += wrapAnsiText(input.slice(lastIndex, match.index), classes)
    classes = updateAnsiClasses(classes, match[1])
    lastIndex = match.index + match[0].length
  }
  html += wrapAnsiText(input.slice(lastIndex), classes)
  return html
}

function updateAnsiClasses(current, sequence) {
  const codes = String(sequence || '0').split(';').filter(Boolean)
  if (!codes.length || codes.includes('0')) return []
  const next = current.filter(cls => !cls.startsWith('ansi-fg-') && !cls.startsWith('ansi-bg-'))
  for (const code of codes) {
    if (code === '1' && !next.includes('ansi-bold')) next.push('ansi-bold')
    if (code === '2' && !next.includes('ansi-dim')) next.push('ansi-dim')
    if (ANSI_CLASS_MAP[code]) next.push(ANSI_CLASS_MAP[code])
  }
  return next
}

function wrapAnsiText(text, classes) {
  if (!text) return ''
  const escaped = escapeHtml(text)
  return classes.length ? `<span class="${classes.join(' ')}">${escaped}</span>` : escaped
}

const ANSI_CLASS_MAP = {
  30: 'ansi-fg-black',
  31: 'ansi-fg-red',
  32: 'ansi-fg-green',
  33: 'ansi-fg-yellow',
  34: 'ansi-fg-blue',
  35: 'ansi-fg-magenta',
  36: 'ansi-fg-cyan',
  37: 'ansi-fg-white',
  90: 'ansi-fg-gray',
}

function escapeHtml(text) {
  return String(text)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}
