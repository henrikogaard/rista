import { state, $ } from './state.js'

// ── Terminal Drawer ──────────────────────────────────────────────
// A collapsible bottom drawer for running project shell commands.

let commandHistory = []
let historyIndex = -1

export function buildTerminalDrawer() {
  return `
    <div class="terminal-drawer" id="terminal-drawer">
      <div class="terminal-drawer__header">
        <div class="terminal-drawer__title">Terminal</div>
        <div class="terminal-drawer__path" id="terminal-path"></div>
        <div class="terminal-drawer__close" id="terminal-close-btn" title="Close terminal" role="button" tabindex="0">
          <svg viewBox="0 0 16 16" width="12" height="12"><path d="M3.5 3.5l9 9M12.5 3.5l-9 9" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round"/></svg>
        </div>
      </div>
      <div class="terminal-drawer__output" id="terminal-output"></div>
      <div class="terminal-drawer__input-row">
        <span class="terminal-drawer__prompt">$</span>
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
  }
}

export function openTerminalDrawer() {
  const drawer = $('terminal-drawer')
  if (!drawer) return
  drawer.classList.add('open')
  $('terminal-input')?.focus()
  syncTerminalPath()
}

export function closeTerminalDrawer() {
  const drawer = $('terminal-drawer')
  if (!drawer) return
  drawer.classList.remove('open')
}

function syncTerminalPath() {
  const pathEl = $('terminal-path')
  if (!pathEl) return
  pathEl.textContent = state.folderPath || '~'
}

export function appendTerminalOutput(text, type = 'stdout') {
  const output = $('terminal-output')
  if (!output) return
  const line = document.createElement('div')
  line.className = `terminal-line terminal-line--${type}`
  line.textContent = text
  output.appendChild(line)
  output.scrollTop = output.scrollHeight
}

export function clearTerminalOutput() {
  const output = $('terminal-output')
  if (output) output.innerHTML = ''
}

export function handleTerminalInput(event) {
  const input = $('terminal-input')
  if (!input) return

  input.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      const command = input.value.trim()
      if (!command) return

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
        const result = await window.fjord.runTerminalCommand(command, cwd)
        if (result.stdout) appendTerminalOutput(result.stdout, 'stdout')
        if (result.stderr) appendTerminalOutput(result.stderr, 'stderr')
        if (result.code !== 0 && !result.stdout && !result.stderr) {
          appendTerminalOutput(`Exit code: ${result.code}`, 'error')
        }
      } catch (err) {
        appendTerminalOutput(err.message, 'error')
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
}
