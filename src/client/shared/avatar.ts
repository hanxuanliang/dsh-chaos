/** Deterministic fallback identity for actors without a custom local avatar. */
export interface AvatarSeed {
  background: string
  accent: string
  pattern: readonly string[]
  initial: string
}

const AGENT_PATTERNS = [
  ['00011000', '00122100', '01222210', '01222210', '01222210', '12222221', '11111111', '00011000'],
  ['00111100', '01222210', '12122121', '12222221', '11222211', '01222210', '01111110', '00100100'],
  ['00011000', '01122110', '12222221', '12122121', '12222221', '01122110', '00111100', '01100110'],
  ['01100110', '12211221', '12222221', '11222211', '01222210', '01211210', '01111110', '00100100'],
] as const

function djb2(value: string): number {
  let hash = 5381
  for (let index = 0; index < value.length; index += 1) {
    hash = ((hash << 5) + hash + value.charCodeAt(index)) | 0
  }
  return Math.abs(hash)
}

export function avatarSeed(handle: string, displayName: string): AvatarSeed {
  const stableKey = handle.trim() || displayName.trim() || '?'
  const hash = djb2(stableKey)
  // Multiplying by the golden angle keeps sequential handles such as test-1
  // and test-2 visually separated instead of landing on adjacent hues.
  const hue = Math.round((hash * 137.508) % 360)
  // Saturation and lightness vary per identity too, from independent hash
  // slices: one fixed pastel recipe made consecutive Agents read as the same
  // pale wash. Vivid range: strong chroma, mid lightness.
  const saturation = 68 + (hash >>> 8) % 24
  const lightness = 46 + (hash >>> 16) % 18
  const label = displayName.trim() || handle.trim() || '?'
  const words = label.split(/[\s_-]+/u).filter(Boolean)
  const glyphs = Array.from(label)
  const initial = words.length > 1
    ? `${Array.from(words[0] ?? '?')[0] ?? '?'}${Array.from(words.at(-1) ?? '?')[0] ?? '?'}`
    : glyphs.length <= 2
      ? glyphs.join('')
      : `${glyphs[0] ?? '?'}${glyphs.at(-1) ?? '?'}`
  // Content-derived identity color, not theme chrome. Stable identities keep
  // their hue while all surrounding UI colors continue to use host tokens.
  return {
    background: `hsl(${String(hue)} ${String(saturation)}% ${String(lightness)}%)`,
    accent: `hsl(${String((hue + 137) % 360)} 82% 70%)`,
    pattern: AGENT_PATTERNS[hash % AGENT_PATTERNS.length] ?? AGENT_PATTERNS[0],
    initial,
  }
}
