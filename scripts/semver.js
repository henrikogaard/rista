#!/usr/bin/env node

const fs = require('fs')
const path = require('path')

const rootDir = path.resolve(__dirname, '..')
const packageJsonPath = path.join(rootDir, 'package.json')
const packageLockPath = path.join(rootDir, 'package-lock.json')

const SEMVER_RE = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'))
}

function writeJson(filePath, data) {
  fs.writeFileSync(filePath, `${JSON.stringify(data, null, 2)}\n`)
}

function parse(version) {
  const match = String(version).trim().match(SEMVER_RE)
  if (!match) return null
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    prerelease: match[4] || '',
    build: match[5] || '',
  }
}

function format({ major, minor, patch, prerelease = '', build = '' }) {
  return `${major}.${minor}.${patch}${prerelease ? `-${prerelease}` : ''}${build ? `+${build}` : ''}`
}

function validate(version) {
  const parsed = parse(version)
  if (!parsed) {
    console.error(`Invalid semantic version: ${version}`)
    process.exit(1)
  }
  return parsed
}

function bump(version, release, identifier = 'alpha') {
  const parsed = validate(version)
  const next = { ...parsed, build: '' }

  if (release === 'major') {
    next.major += 1
    next.minor = 0
    next.patch = 0
    next.prerelease = ''
  } else if (release === 'minor') {
    next.minor += 1
    next.patch = 0
    next.prerelease = ''
  } else if (release === 'patch') {
    next.patch += 1
    next.prerelease = ''
  } else if (release === 'premajor') {
    next.major += 1
    next.minor = 0
    next.patch = 0
    next.prerelease = `${identifier}.0`
  } else if (release === 'preminor') {
    next.minor += 1
    next.patch = 0
    next.prerelease = `${identifier}.0`
  } else if (release === 'prepatch') {
    next.patch += 1
    next.prerelease = `${identifier}.0`
  } else if (release === 'prerelease') {
    if (!next.prerelease) {
      next.patch += 1
      next.prerelease = `${identifier}.0`
    } else {
      const parts = next.prerelease.split('.')
      const last = parts[parts.length - 1]
      const currentIdentifier = parts.slice(0, -1).join('.') || identifier
      if (/^\d+$/.test(last)) {
        parts[parts.length - 1] = String(Number(last) + 1)
        next.prerelease = parts.join('.')
      } else {
        next.prerelease = `${currentIdentifier}.0`
      }
    }
  } else {
    console.error(`Unsupported bump type: ${release}`)
    process.exit(1)
  }

  return format(next)
}

function syncVersion(nextVersion) {
  const pkg = readJson(packageJsonPath)
  const lock = readJson(packageLockPath)

  pkg.version = nextVersion
  lock.version = nextVersion
  if (lock.packages && lock.packages['']) {
    lock.packages[''].version = nextVersion
  }

  writeJson(packageJsonPath, pkg)
  writeJson(packageLockPath, lock)
}

function printUsage() {
  console.log(`Usage:
  node scripts/semver.js check
  node scripts/semver.js current
  node scripts/semver.js set <version>
  node scripts/semver.js bump <patch|minor|major|prepatch|preminor|premajor|prerelease> [identifier]
`)
}

function main() {
  const [, , command, ...args] = process.argv
  const pkg = readJson(packageJsonPath)

  if (!command) {
    printUsage()
    process.exit(1)
  }

  if (command === 'check') {
    validate(pkg.version)
    console.log(`Semver OK: ${pkg.version}`)
    return
  }

  if (command === 'current') {
    validate(pkg.version)
    console.log(pkg.version)
    return
  }

  if (command === 'set') {
    const nextVersion = args[0]
    if (!nextVersion) {
      printUsage()
      process.exit(1)
    }
    validate(nextVersion)
    syncVersion(nextVersion)
    console.log(`Version set to ${nextVersion}`)
    return
  }

  if (command === 'bump') {
    const release = args[0]
    const identifier = args[1] || 'alpha'
    if (!release) {
      printUsage()
      process.exit(1)
    }
    const nextVersion = bump(pkg.version, release, identifier)
    syncVersion(nextVersion)
    console.log(`Version bumped: ${pkg.version} -> ${nextVersion}`)
    return
  }

  printUsage()
  process.exit(1)
}

main()
