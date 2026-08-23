const HANDLE_MAX_LENGTH = 40
const AGENT_HANDLE = /^[a-z0-9](?:[a-z0-9-]{0,38}[a-z0-9])?$/

/** Stable ASCII suffix for display names that contain no Latin letters or digits. */
function handleFingerprint(value: string): string {
  let hash = 0x811c9dc5
  for (const character of value.normalize('NFKC')) {
    hash ^= character.codePointAt(0) ?? 0
    hash = Math.imul(hash, 0x01000193)
  }
  return (hash >>> 0).toString(36)
}

function availableHandle(base: string, unavailable: ReadonlySet<string>): string {
  if (!unavailable.has(base)) return base
  for (let index = 2; index < 100_000; index += 1) {
    const suffix = `-${index}`
    const stem = base.slice(0, HANDLE_MAX_LENGTH - suffix.length).replace(/-+$/g, '') || 'agent'
    const candidate = `${stem}${suffix}`
    if (!unavailable.has(candidate)) return candidate
  }
  throw new Error('could not generate an available Agent handle')
}

/** Generate a valid, collision-free default while keeping the handle user-editable. */
export function generatedAgentHandle(name: string, existingHandles: readonly string[]): string {
  const normalized = name.trim().toLowerCase().normalize('NFKC')
  const ascii = normalized.replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')
  const fallback = normalized === '' ? 'agent' : `agent-${handleFingerprint(normalized)}`
  const base = (ascii || fallback).slice(0, HANDLE_MAX_LENGTH).replace(/-+$/g, '') || 'agent'
  return availableHandle(base, new Set(existingHandles.map(handle => handle.trim().toLowerCase())))
}

/** Match the same public handle contract enforced by the host boundary. */
export function isValidAgentHandle(handle: string): boolean {
  return AGENT_HANDLE.test(handle)
}
