export const LAUNCH_BUDGET_MS = 400
export const FILE_SWITCH_BUDGET_MS = 80

let _launchStart = null
let _fileSwitchStart = null

export function markLaunchStart() {
  _launchStart = performance.now()
}

export function markLaunchDone() {
  if (_launchStart === null) return
  const elapsed = performance.now() - _launchStart
  if (elapsed > LAUNCH_BUDGET_MS) {
    console.warn(`[perf] Launch exceeded budget: ${Math.round(elapsed)}ms > ${LAUNCH_BUDGET_MS}ms`)
  }
  _launchStart = null
}

export function markFileSwitchStart() {
  _fileSwitchStart = performance.now()
}

export function markFileSwitchDone() {
  if (_fileSwitchStart === null) return
  const start = _fileSwitchStart
  _fileSwitchStart = null
  requestAnimationFrame(() => {
    const elapsed = performance.now() - start
    if (elapsed > FILE_SWITCH_BUDGET_MS) {
      console.warn(`[perf] File switch exceeded budget: ${Math.round(elapsed)}ms > ${FILE_SWITCH_BUDGET_MS}ms`)
    }
  })
}

export function measureFileSwitch(fn) {
  const start = performance.now()
  const result = fn()
  const elapsed = performance.now() - start
  if (elapsed > FILE_SWITCH_BUDGET_MS) {
    console.warn(`[perf] File switch exceeded budget: ${Math.round(elapsed)}ms > ${FILE_SWITCH_BUDGET_MS}ms`)
  }
  return result
}
