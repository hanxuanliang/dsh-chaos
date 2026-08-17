/** Official trigger pipeline only sees `/` and `@`. We detect `#` ourselves. */

export interface HashHit {
  /** Text after `#` up to the caret. */
  readonly query: string
  /** Inclusive index of `#`. */
  readonly start: number
  /** Exclusive caret index. */
  readonly end: number
}

export interface HashChannel {
  readonly id: string
  readonly name: string
}

const NAME_CHAR = /[0-9A-Za-z_-]/
const WHITESPACE = /\s/
const MAX_CANDIDATES = 12

function isBoundary(draft: string, index: number): boolean {
  return index === 0 || WHITESPACE.test(draft.charAt(index - 1))
}

/** Live `#query` at the caret, or null when `/` `@` or a mid-word hash owns the token. */
export function detectHashTrigger(draft: string, caret: number): HashHit | null {
  const end = Math.max(0, Math.min(caret, draft.length))
  for (let index = end - 1; index >= 0; index -= 1) {
    const char = draft.charAt(index)
    if (WHITESPACE.test(char)) return null
    if (char === '/' || char === '@') return null
    if (char === '#') {
      if (!isBoundary(draft, index)) return null
      return { query: draft.slice(index + 1, end), start: index, end }
    }
    if (!NAME_CHAR.test(char)) return null
  }
  return null
}

/** Prefix first, then contains. Empty query lists every Channel. */
export function rankChannels(
  channels: readonly HashChannel[],
  query: string,
  limit = MAX_CANDIDATES,
): readonly HashChannel[] {
  const needle = query.toLowerCase()
  return channels
    .map(channel => {
      const name = channel.name.toLowerCase()
      if (needle === '') return { channel, score: 1 }
      if (name === needle) return { channel, score: 3 }
      if (name.startsWith(needle)) return { channel, score: 2 }
      if (name.includes(needle)) return { channel, score: 1 }
      return undefined
    })
    .filter((row): row is { channel: HashChannel; score: number } => row !== undefined)
    .sort((left, right) => right.score - left.score || left.channel.name.localeCompare(right.channel.name))
    .slice(0, limit)
    .map(row => row.channel)
}

/** Drop the `#query` token. Picking a Channel enters the room; it does not send. */
export function applyHashPick(draft: string, hit: HashHit): { draft: string; caret: number } {
  const next = `${draft.slice(0, hit.start)}${draft.slice(hit.end)}`
  return { draft: next, caret: hit.start }
}
