#!/usr/bin/env node

const fs = require('node:fs')
const path = require('node:path')
const { spawnSync } = require('node:child_process')

const root = path.resolve(__dirname, '..')
const defaultApp = path.join(root, 'src-tauri/target/release/bundle/macos/Rísta.app')
const appPath = process.argv[2] ? path.resolve(process.argv[2]) : defaultApp
const asciiIconName = 'Rista.icns'
const generatedIconName = 'Rísta.icns'

function run(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8' })
  if (result.status !== 0) {
    const message = result.stderr || result.stdout || `${command} failed`
    throw new Error(message.trim())
  }
  return result.stdout.trim()
}

function patchBundleIcon(bundlePath) {
  if (!fs.existsSync(bundlePath)) {
    return false
  }

  const resourcesPath = path.join(bundlePath, 'Contents', 'Resources')
  const plistPath = path.join(bundlePath, 'Contents', 'Info.plist')
  const generatedIconPath = path.join(resourcesPath, generatedIconName)
  const asciiIconPath = path.join(resourcesPath, asciiIconName)

  if (!fs.existsSync(generatedIconPath) && !fs.existsSync(asciiIconPath)) {
    throw new Error(`Missing macOS app icon in ${resourcesPath}`)
  }
  if (!fs.existsSync(asciiIconPath)) {
    fs.copyFileSync(generatedIconPath, asciiIconPath)
  }

  run('/usr/libexec/PlistBuddy', ['-c', `Set :CFBundleIconFile ${asciiIconName}`, plistPath])
  return true
}

module.exports = { patchBundleIcon }

if (require.main === module) {
  try {
    const patched = patchBundleIcon(appPath)
    if (patched) {
      console.log(`Verified macOS bundle icon: ${path.join(appPath, 'Contents', 'Resources', asciiIconName)}`)
    }
  } catch (error) {
    console.error(error.message)
    process.exit(1)
  }
}
