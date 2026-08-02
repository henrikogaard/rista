import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { state, $, getFocusedTab } from './state.js'
import { terminalIcon, chevronIcon, closeIcon } from './icons.js'

// ── Terminal workspace ───────────────────────────────────────────
// xterm.js renders the terminal while Tauri owns a persistent PTY session.

const DEFAULT_TERMINAL_HEIGHT = 272
const MIN_TERMINAL_HEIGHT = 184
const TERMINAL_HEIGHT_STORAGE_KEY = 'rista.terminalHeight'

let terminal = null
let fitAddon = null
let terminalSessionId = null
let terminalConnected = false
let terminalStarting = false
let terminalResizeObserver = null
let terminalOutputUnlisten = null
let terminalExitUnlisten = null
let terminalHeight = readTerminalHeight()
let resizeFrame = null
let fallbackLine = ''
let fallbackHistory = []
let fallbackHistoryIndex = -1
let terminalCwd = ''
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

function resolveTerminalCwd() {
  const workspace = normalizeTerminalPath(state.folderPath || '')
  if (workspace && terminalWorkspacePath !== workspace) {
    terminalWorkspacePath = workspace
    terminalCwd = workspace
  }
  if (terminalCwd) return terminalCwd
  if (workspace) return workspace
  return directoryPath(getFocusedTab()?.path || '')
}

function readTerminalHeight() {
  try {
    const saved = localStorage.getItem(TERMINAL_HEIGHT_STORAGE_KEY)
    if (saved === null) return DEFAULT_TERMINAL_HEIGHT
    const height = Number(saved)
    return Number.isFinite(height) ? Math.max(MIN_TERMINAL_HEIGHT, height) : DEFAULT_TERMINAL_HEIGHT
  } catch {
    return DEFAULT_TERMINAL_HEIGHT
  }
}

function persistTerminalHeight() {
  try {
    localStorage.setItem('rista.terminalHeight', String(Math.round(terminalHeight)))
  } catch {}
}

function applyTerminalHeight() {
  $('terminal-drawer')?.style.setProperty('--terminal-height', `${Math.round(terminalHeight)}px`)
}

function cssValue(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim()
}

function currentTerminalTheme() {
  return {
    background: cssValue('--bg0'),
    foreground: cssValue('--text1'),
    cursor: cssValue('--accent-hi'),
    cursorAccent: cssValue('--bg0'),
    selectionBackground: cssValue('--accent-dim'),
    black: cssValue('--bg0'),
    red: cssValue('--red'),
    green: cssValue('--green'),
    yellow: cssValue('--amber'),
    blue: cssValue('--accent'),
    magenta: cssValue('--accent-hi'),
    cyan: cssValue('--accent-hi'),
    white: cssValue('--text1'),
    brightBlack: cssValue('--text3'),
    brightRed: cssValue('--red'),
    brightGreen: cssValue('--green'),
    brightYellow: cssValue('--amber'),
    brightBlue: cssValue('--accent-hi'),
    brightMagenta: cssValue('--accent-hi'),
    brightCyan: cssValue('--accent-hi'),
    brightWhite: cssValue('--text1'),
  }
}

function createSessionId() {
  if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID()
  return `rista-terminal-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

function nativeTerminalAvailable() {
  return [
    'startTerminalSession',
    'writeTerminalSession',
    'resizeTerminalSession',
    'closeTerminalSession',
    'onTerminalOutput',
    'onTerminalExit',
  ].every(name => typeof window.fjord?.[name] === 'function')
}

function setTerminalStatus(status, label) {
  const drawer = $('terminal-drawer')
  const statusEl = $('terminal-status')
  drawer?.classList.toggle('is-connected', status === 'connected' || status === 'running')
  drawer?.classList.toggle('is-running', status === 'running' || status === 'connecting')
  drawer?.classList.toggle('is-disconnected', status === 'disconnected')
  if (statusEl) statusEl.textContent = label
}

function syncTerminalPath() {
  const cwd = resolveTerminalCwd()
  const pathEl = $('terminal-path')
  const cwdEl = $('terminal-cwd')
  if (pathEl) {
    pathEl.textContent = cwd || '~'
    pathEl.title = cwd || '~'
  }
  if (cwdEl) {
    cwdEl.textContent = compactPath(cwd)
    cwdEl.title = cwd || '~'
  }
}

async function syncShellInfo() {
  try {
    const next = await window.fjord?.getShellInfo?.()
    shellInfo = next && typeof next.name === 'string'
      ? { name: next.name || 'shell', path: typeof next.path === 'string' ? next.path : '' }
      : { name: 'shell', path: '' }
  } catch {
    shellInfo = { name: 'shell', path: '' }
  }
  const shell = $('terminal-shell')
  if (shell) {
    shell.textContent = shellInfo.name
    shell.title = shellInfo.path || shellInfo.name
  }
}

function fitTerminal() {
  if (!fitAddon || !$('terminal-drawer')?.classList.contains('open')) return
  if (resizeFrame) cancelAnimationFrame(resizeFrame)
  resizeFrame = requestAnimationFrame(() => {
    resizeFrame = null
    try {
      fitAddon.fit()
    } catch {}
  })
}

async function ensureTerminalListeners() {
  if (!nativeTerminalAvailable()) return
  if (!terminalOutputUnlisten) {
    terminalOutputUnlisten = await window.fjord.onTerminalOutput((payload) => {
      if (!terminal || payload?.sessionId !== terminalSessionId) return
      terminal.write(new Uint8Array(payload.data || []))
    })
  }
  if (!terminalExitUnlisten) {
    terminalExitUnlisten = await window.fjord.onTerminalExit((payload) => {
      if (!terminal || payload?.sessionId !== terminalSessionId) return
      terminalConnected = false
      terminalSessionId = null
      setTerminalStatus('disconnected', `Exited ${payload?.code ?? ''}`.trim())
      terminal.writeln(`\r\n\x1b[2mProcess exited with code ${payload?.code ?? 1}. Restart to continue.\x1b[0m`)
    })
  }
}

function writeFallbackPrompt() {
  terminal?.write(`\x1b[38;5;180m${compactPath(resolveTerminalCwd())}\x1b[0m \x1b[2m$\x1b[0m `)
}

function replaceFallbackLine(nextLine) {
  fallbackLine = nextLine
  terminal?.write(`\x1b[2K\r`)
  writeFallbackPrompt()
  terminal?.write(fallbackLine)
}

function resolveFallbackCd(rawTarget) {
  const target = String(rawTarget || '').trim().replace(/^['"]|['"]$/g, '')
  if (!target || target === '~') return normalizeTerminalPath(state.folderPath || resolveTerminalCwd())
  if (target.startsWith('/')) return normalizeTerminalPath(target)
  return normalizeTerminalPath(`${resolveTerminalCwd()}/${target}`)
}

async function runFallbackCommand(command) {
  fallbackHistory.push(command)
  fallbackHistoryIndex = fallbackHistory.length

  if (command === 'clear') {
    terminal.clear()
    writeFallbackPrompt()
    return
  }
  const cdMatch = command.match(/^cd(?:\s+(.+))?$/)
  if (cdMatch) {
    terminalCwd = resolveFallbackCd(cdMatch[1])
    syncTerminalPath()
    writeFallbackPrompt()
    return
  }

  setTerminalStatus('running', 'Running')
  try {
    const result = await window.fjord?.runTerminalCommand?.(command, resolveTerminalCwd() || undefined)
    if (result?.stdout) terminal.write(String(result.stdout).replace(/\n/g, '\r\n'))
    if (result?.stderr) terminal.write(`\x1b[33m${String(result.stderr).replace(/\n/g, '\r\n')}\x1b[0m`)
    if (result?.error) terminal.writeln(`\x1b[31m${result.error}\x1b[0m`)
  } catch (error) {
    terminal.writeln(`\x1b[31m${error.message}\x1b[0m`)
  } finally {
    setTerminalStatus('connected', 'Preview')
    writeFallbackPrompt()
  }
}

function handleFallbackData(data) {
  if (!terminal) return
  if (data === '\x1b[A' || data === '\x1b[B') {
    if (data === '\x1b[A' && fallbackHistoryIndex > 0) fallbackHistoryIndex--
    if (data === '\x1b[B' && fallbackHistoryIndex < fallbackHistory.length) fallbackHistoryIndex++
    replaceFallbackLine(fallbackHistory[fallbackHistoryIndex] || '')
    return
  }

  for (const char of data) {
    if (char === '\r') {
      const command = fallbackLine.trim()
      fallbackLine = ''
      terminal.write('\r\n')
      if (command) runFallbackCommand(command)
      else writeFallbackPrompt()
      continue
    }
    if (char === '\x7f') {
      if (fallbackLine) {
        fallbackLine = fallbackLine.slice(0, -1)
        terminal.write('\b \b')
      }
      continue
    }
    if (char === '\x03') {
      fallbackLine = ''
      terminal.write('^C\r\n')
      writeFallbackPrompt()
      continue
    }
    if (char === '\x0c') {
      terminal.clear()
      replaceFallbackLine(fallbackLine)
      continue
    }
    if (char >= ' ') {
      fallbackLine += char
      terminal.write(char)
    }
  }
}

function initializeTerminalViewport() {
  const viewport = $('terminal-viewport')
  if (!viewport || terminal) return

  terminal = new Terminal({
    allowTransparency: false,
    cursorBlink: true,
    cursorStyle: 'bar',
    drawBoldTextInBrightColors: false,
    fontFamily: cssValue('--mono'),
    fontSize: 12,
    fontWeight: '400',
    fontWeightBold: '500',
    lineHeight: 1.35,
    macOptionIsMeta: true,
    scrollback: 5000,
    smoothScrollDuration: 90,
    theme: currentTerminalTheme(),
  })
  fitAddon = new FitAddon()
  terminal.loadAddon(fitAddon)
  terminal.open(viewport)
  terminal.textarea?.setAttribute('aria-label', 'Terminal input')
  terminal.attachCustomKeyEventHandler((event) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'c' && terminal.hasSelection()) {
      navigator.clipboard?.writeText(terminal.getSelection())
      return false
    }
    return true
  })
  terminal.onData((data) => {
    if (terminalConnected && terminalSessionId) {
      window.fjord.writeTerminalSession(terminalSessionId, data).catch((error) => {
        setTerminalStatus('disconnected', 'Write failed')
        terminal.writeln(`\r\n\x1b[31m${error.message}\x1b[0m`)
      })
    } else if (!nativeTerminalAvailable()) {
      handleFallbackData(data)
    }
  })
  terminal.onResize(({ cols, rows }) => {
    if (terminalConnected && terminalSessionId) {
      window.fjord.resizeTerminalSession(terminalSessionId, cols, rows).catch(() => {})
    }
  })
  terminalResizeObserver = new ResizeObserver(fitTerminal)
  terminalResizeObserver.observe(viewport)
  fitTerminal()
}

async function startTerminalSession() {
  if (terminalStarting || terminalConnected) return
  terminalStarting = true
  initializeTerminalViewport()
  syncTerminalPath()
  await syncShellInfo()
  fitTerminal()

  if (!nativeTerminalAvailable()) {
    setTerminalStatus('connected', 'Preview')
    terminal.writeln('\x1b[2mRísta terminal preview · commands run in the workspace harness\x1b[0m')
    writeFallbackPrompt()
    terminalStarting = false
    return
  }

  setTerminalStatus('connecting', 'Connecting')
  await ensureTerminalListeners()
  terminalSessionId = createSessionId()
  try {
    const info = await window.fjord.startTerminalSession({
      sessionId: terminalSessionId,
      cwd: resolveTerminalCwd() || undefined,
      cols: terminal.cols,
      rows: terminal.rows,
    })
    if (info?.name) {
      shellInfo = info
      const shell = $('terminal-shell')
      if (shell) shell.textContent = info.name
    }
    terminalConnected = true
    setTerminalStatus('connected', 'Connected')
    terminal.focus()
  } catch (error) {
    terminalSessionId = null
    terminalConnected = false
    setTerminalStatus('disconnected', 'Unavailable')
    terminal.writeln(`\x1b[31mCould not start terminal: ${error.message}\x1b[0m`)
  } finally {
    terminalStarting = false
  }
}

async function stopTerminalSession() {
  const activeSessionId = terminalSessionId
  terminalSessionId = null
  terminalConnected = false
  if (activeSessionId && nativeTerminalAvailable()) {
    try {
      await window.fjord.closeTerminalSession(activeSessionId)
    } catch {}
  }
  setTerminalStatus('disconnected', 'Stopped')
}

async function restartTerminalSession() {
  await stopTerminalSession()
  terminal?.reset()
  fallbackLine = ''
  await startTerminalSession()
  terminal?.focus()
}

function copyTerminalSelection() {
  if (!terminal?.hasSelection()) {
    setTerminalStatus(terminalConnected ? 'connected' : 'disconnected', 'Select text first')
    return
  }
  navigator.clipboard?.writeText(terminal.getSelection())
  setTerminalStatus(terminalConnected ? 'connected' : 'disconnected', 'Copied')
}

function beginTerminalResize(event) {
  event.preventDefault()
  const startY = event.clientY
  const startHeight = $('terminal-drawer')?.getBoundingClientRect().height || terminalHeight
  const move = (moveEvent) => {
    const maxHeight = Math.max(MIN_TERMINAL_HEIGHT, window.innerHeight - 180)
    terminalHeight = Math.min(maxHeight, Math.max(MIN_TERMINAL_HEIGHT, startHeight + startY - moveEvent.clientY))
    applyTerminalHeight()
    fitTerminal()
  }
  const finish = () => {
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', finish)
    document.body.classList.remove('is-resizing-terminal')
    persistTerminalHeight()
    fitTerminal()
  }
  document.body.classList.add('is-resizing-terminal')
  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', finish, { once: true })
}

function focusTerminalSoon() {
  requestAnimationFrame(() => {
    applyTerminalHeight()
    initializeTerminalViewport()
    fitTerminal()
    startTerminalSession()
    terminal?.focus()
  })
}

export function buildTerminalDrawer() {
  return `
    <section class="terminal-drawer" id="terminal-drawer" data-experimental-feature="featureTerminal" aria-label="Workspace terminal">
      <div class="terminal-resize-handle" id="terminal-resize-handle" title="Resize terminal" aria-label="Resize terminal"><span></span></div>
      <div class="terminal-drawer__surface">
        <div class="terminal-drawer__header">
          <div class="terminal-drawer__identity">
            <div class="terminal-drawer__title">${terminalIcon()}<span>Terminal</span></div>
            <span class="terminal-drawer__status-dot" aria-hidden="true"></span>
            <span class="terminal-drawer__status" id="terminal-status" aria-live="polite">Ready</span>
          </div>
          <div class="terminal-drawer__prompt-chip">
            <span class="terminal-drawer__cwd" id="terminal-cwd">~</span>
            <span class="terminal-shell" id="terminal-shell">shell</span>
          </div>
          <div class="terminal-drawer__path" id="terminal-path"></div>
          <div class="terminal-drawer__actions">
            <div class="terminal-drawer__action" id="terminal-copy-btn" title="Copy selected text" role="button" tabindex="0">Copy</div>
            <div class="terminal-drawer__action" id="terminal-clear-btn" title="Clear scrollback" role="button" tabindex="0">Clear</div>
            <div class="terminal-drawer__action" id="terminal-restart-btn" title="Restart terminal session" role="button" tabindex="0">Restart</div>
            <div class="terminal-drawer__shortcut" title="Toggle terminal">⌘J</div>
            <div class="terminal-drawer__collapse" id="terminal-collapse-btn" title="Collapse terminal" aria-label="Collapse terminal" role="button" tabindex="0">${chevronIcon()}</div>
            <div class="terminal-drawer__close" id="terminal-close-btn" title="Close terminal session" aria-label="Close terminal session" role="button" tabindex="0">${closeIcon()}</div>
          </div>
        </div>
        <div class="terminal-drawer__viewport" id="terminal-viewport" aria-label="Terminal session"></div>
      </div>
    </section>
  `
}

export function toggleTerminalDrawer() {
  const drawer = $('terminal-drawer')
  if (!drawer) return
  drawer.classList.toggle('open')
  if (drawer.classList.contains('open')) focusTerminalSoon()
  syncTerminalToggle()
}

export function openTerminalDrawer() {
  const drawer = $('terminal-drawer')
  if (!drawer) return
  drawer.classList.add('open')
  focusTerminalSoon()
  syncTerminalToggle()
}

export function closeTerminalDrawer() {
  $('terminal-drawer')?.classList.remove('open')
  syncTerminalToggle()
}

function syncTerminalToggle() {
  $('terminal-toggle')?.classList.toggle('active', $('terminal-drawer')?.classList.contains('open'))
}

export function clearTerminalOutput() {
  terminal?.clear()
  terminal?.focus()
}

export function appendTerminalOutput(text) {
  terminal?.write(String(text || ''))
}

function wireTerminalAction(id, action) {
  const control = $(id)
  control?.addEventListener('click', action)
  control?.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter' && event.key !== ' ') return
    event.preventDefault()
    action()
  })
}

export function handleTerminalInput() {
  applyTerminalHeight()
  wireTerminalAction('terminal-copy-btn', copyTerminalSelection)
  wireTerminalAction('terminal-clear-btn', clearTerminalOutput)
  wireTerminalAction('terminal-restart-btn', restartTerminalSession)
  wireTerminalAction('terminal-collapse-btn', closeTerminalDrawer)
  wireTerminalAction('terminal-close-btn', async () => {
    await stopTerminalSession()
    closeTerminalDrawer()
  })
  $('terminal-resize-handle')?.addEventListener('pointerdown', beginTerminalResize)
  window.addEventListener('beforeunload', () => {
    if (terminalSessionId && nativeTerminalAvailable()) {
      window.fjord.closeTerminalSession(terminalSessionId).catch(() => {})
    }
    terminalOutputUnlisten?.()
    terminalExitUnlisten?.()
    terminalResizeObserver?.disconnect()
  })
}
