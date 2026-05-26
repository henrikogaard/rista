#!/usr/bin/env node

const fs = require('node:fs')
const path = require('node:path')
const { execFileSync, spawnSync } = require('node:child_process')
const { patchBundleIcon } = require('./fix-macos-bundle-icon.js')

const root = path.resolve(__dirname, '..')
const sourceApp = path.join(root, 'src-tauri/target/release/bundle/macos/Rísta.app')
const targetApp = '/Applications/Rísta.app'
const decomposedTargetApp = '/Applications/Rísta.app'
const iconRelativePath = 'Contents/Resources/Rista.icns'
const plistRelativePath = 'Contents/Info.plist'

function assertExists(filePath, label) {
  if (!fs.existsSync(filePath)) {
    console.error(`Missing ${label}: ${filePath}`)
    process.exit(1)
  }
}

function plistValue(plistPath, key) {
  const result = spawnSync('/usr/libexec/PlistBuddy', ['-c', `Print :${key}`, plistPath], {
    encoding: 'utf8',
  })
  return result.status === 0 ? result.stdout.trim() : ''
}

function assertBundleIcon(appPath) {
  const iconPath = path.join(appPath, iconRelativePath)
  const plistPath = path.join(appPath, plistRelativePath)
  assertExists(iconPath, 'app icon')
  assertExists(plistPath, 'Info.plist')

  const iconFile = plistValue(plistPath, 'CFBundleIconFile')
  if (iconFile !== 'Rista.icns') {
    console.error(`Unexpected CFBundleIconFile in ${plistPath}: ${iconFile || '(empty)'}`)
    process.exit(1)
  }
}

assertExists(sourceApp, 'built app bundle')
patchBundleIcon(sourceApp)
assertBundleIcon(sourceApp)

try {
  execFileSync('osascript', ['-e', 'tell application "Rísta" to quit'], { stdio: 'ignore' })
} catch {}

spawnSync('pkill', ['-f', '/Applications/Ri.*sta.app/Contents/MacOS/rista'], { stdio: 'ignore' })
spawnSync('pkill', ['-f', '/Applications/Rísta.app/Contents/MacOS/rista'], { stdio: 'ignore' })

fs.rmSync(targetApp, { recursive: true, force: true })
fs.rmSync(decomposedTargetApp, { recursive: true, force: true })
fs.cpSync(sourceApp, targetApp, { recursive: true, preserveTimestamps: true })
assertBundleIcon(targetApp)

console.log(`Installed ${targetApp}`)
console.log(`Verified ${path.join(targetApp, iconRelativePath)}`)
