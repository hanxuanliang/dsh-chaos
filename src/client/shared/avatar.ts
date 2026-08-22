/** Deterministic fallback identity for actors without a custom local avatar. */
export interface AvatarSeed {
  background: string
  initial: string
}

function djb2(value: string): number {
  let hash = 5381
  for (let index = 0; index < value.length; index += 1) {
    hash = ((hash << 5) + hash + value.charCodeAt(index)) | 0
  }
  return Math.abs(hash)
}

export function avatarSeed(handle: string, displayName: string): AvatarSeed {
  const stableKey = handle.trim() || displayName.trim() || '?'
  // Multiplying by the golden angle keeps sequential handles such as test-1
  // and test-2 visually separated instead of landing on adjacent hues.
  const hue = Math.round((djb2(stableKey) * 137.508) % 360)
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
  return { background: `hsl(${String(hue)} 48% 38%)`, initial }
}
