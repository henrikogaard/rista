import { state, $ } from './state.js'

// ── Terminal Drawer ──────────────────────────────────────────────
// A collapsible bottom drawer for running project shell commands.

let commandHistory = []
let historyIndex = -1
let terminalBusy = false
let shellInfo = { name: 'shell', path: '' }

function folderName(folderPath) {
  return String(folderPath || '').split(/[\\/]/).filter(Boolean).pop() || '~'
}

function formatDuration(ms) {
  const value = Number(ms || 0)
  if (value < 1000) return `${value}ms`
  return `${(value / 1000).toFixed(1)}s`
}

async function syncShellInfo() {
  try {
    shellInfo = await window.fjord.getShellInfo()
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
    <div class="terminal-drawer" id="terminal-drawer">
      <div class="terminal-drawer__header">
        <div class="terminal-drawer__identity">
          <div class="terminal-drawer__title">Terminal</div>
          <div class="terminal-drawer__cwd" id="terminal-cwd">~</div>
        </div>
        <div class="terminal-shell" id="terminal-shell">shell</div>
        <div class="terminal-drawer__path" id="terminal-path"></div>
        <div class="terminal-drawer__status" id="terminal-status">Ready</div>
        <div class="terminal-drawer__action" id="terminal-clear-btn" title="Clear terminal" role="button" tabindex="0">Clear</div>
        <div class="terminal-drawer__close" id="terminal-close-btn" title="Close terminal" role="button" tabindex="0">
          <svg viewBox="0 0 16 16" width="12" height="12"><path d="M3.5 3.5l9 9M12.5 3.5l-9 9" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round"/></svg>
        </div>
      </div>
      <div class="terminal-drawer__output" id="terminal-output">
        <div class="terminal-drawer__empty">Commands run in the current workspace folder.</div>
      </div>
      <div class="terminal-drawer__input-row">
        <span class="terminal-drawer__prompt" id="terminal-prompt">$</span>
        <input type="text" class="terminal-drawer__input" id="terminal-input" placeholder="Type a command…" spellcheck="false" autocomplete="off">
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
  const pathEl = $('terminal-path')
  const cwdEl = $('terminal-cwd')
  const promptEl = $('terminal-prompt')
  if (pathEl) pathEl.textContent = state.folderPath || '~'
  if (cwdEl) cwdEl.textContent = folderName(state.folderPath)
  if (promptEl) promptEl.textContent = `${folderName(state.folderPath)} $`
}

export function appendTerminalOutput(text, type = 'stdout') {
  const output = $('terminal-output')
  if (!output) return
  output.querySelector('.terminal-drawer__empty')?.remove()
  const line = document.createElement('div')
  line.className = type === 'meta' ? 'terminal-line terminal-line__meta' : `terminal-line terminal-line--${type}`
  line.textContent = text
  output.appendChild(line)
  output.scrollTop = output.scrollHeight
}

export function clearTerminalOutput() {
  const output = $('terminal-output')
  if (output) output.innerHTML = '<div class="terminal-drawer__empty">Commands run in the current workspace folder.</div>'
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

export function handleTerminalInput(event) {
  const input = $('terminal-input')
  if (!input) return

  input.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      if (terminalBusy) return
      const command = input.value.trim()
      if (!command) return

      syncTerminalPath()
      appendTerminalOutput(`$ ${command}`, 'command')
      commandHistory.push(command)
      historyIndex = commandHistory.length
      input.value = ''

      if (command === 'clear') {
        clearTerminalOutput()
        return
      }

      const cwd = state.folderPath || undefined
      try {
        setTerminalBusy(true)
        const result = await window.fjord.runTerminalCommand(command, cwd)
        if (result.stdout) appendTerminalOutput(result.stdout, 'stdout')
        if (result.stderr) appendTerminalOutput(result.stderr, 'stderr')
        if (result.error) appendTerminalOutput(result.error, 'error')
        if (result.code !== 0 && !result.stdout && !result.stderr) {
          appendTerminalOutput(`Exit code: ${result.code}`, 'error')
        }
        appendTerminalOutput(`exit ${result.code ?? 0} · ${formatDuration(result.durationMs)}`, 'meta')
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
      return
    }
  })

  $('terminal-close-btn')?.addEventListener('click', closeTerminalDrawer)
  $('terminal-clear-btn')?.addEventListener('click', clearTerminalOutput)
}
