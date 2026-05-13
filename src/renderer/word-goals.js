const STORAGE_KEY = 'fjordmark-word-goals'

let sessionGoal = null
let sessionStartWords = 0

export function getDocumentGoal(filePath) {
  if (!filePath) return null
  try {
    const goals = JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
    return goals[filePath] || null
  } catch {
    return null
  }
}

export function setDocumentGoal(filePath, target) {
  if (!filePath) return
  try {
    const goals = JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
    if (target && target > 0) {
      goals[filePath] = target
    } else {
      delete goals[filePath]
    }
    localStorage.setItem(STORAGE_KEY, JSON.stringify(goals))
  } catch {}
}

export function setSessionGoal(target, currentWords) {
  sessionGoal = target > 0 ? target : null
  sessionStartWords = currentWords || 0
}

export function getSessionGoal() {
  return sessionGoal
}

export function getSessionProgress(currentWords) {
  if (!sessionGoal) return null
  const written = Math.max(0, currentWords - sessionStartWords)
  return { written, target: sessionGoal, complete: written >= sessionGoal }
}

export function formatGoalStatus(currentWords, filePath) {
  const parts = []
  const docGoal = getDocumentGoal(filePath)
  if (docGoal) {
    parts.push(`${currentWords} / ${docGoal}`)
  }
  const session = getSessionProgress(currentWords)
  if (session) {
    parts.push(`session: ${session.written} / ${session.target}`)
  }
  return parts.join('  |  ')
}

export function isGoalReached(currentWords, filePath) {
  const docGoal = getDocumentGoal(filePath)
  return docGoal && currentWords >= docGoal
}
