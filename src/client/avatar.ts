/** Deterministic per-agent avatar seed: the handle hashes to a hue, the display name lends its first glyph. */
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
  const hue = djb2(handle) % 360
  const initial = (displayName.trim() || handle.trim() || '?').charAt(0)
  // Content-derived identity color, not theme chrome (the style guide's token
  // rule covers chrome colors); mirrors the imgs/05 mockup's per-agent hues.
  return { background: `hsl(${String(hue)} 45% 44%)`, initial }
}
