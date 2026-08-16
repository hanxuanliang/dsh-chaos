/**
 * Deterministic geometric identicon for one actor (GitHub identicon idea):
 * the actor id hashes to a fixed palette entry and pattern, so every actor
 * keeps a stable identity color without any design asset.
 */

const PALETTE: ReadonlyArray<readonly [string, string, string]> = [
  ['#1a7f37', '#d3f8df', '#0e5223'],
  ['#4c6fff', '#dbe4ff', '#2440b8'],
  ['#7a5af8', '#e9e2ff', '#4b2fb0'],
  ['#d9730d', '#ffe4c2', '#8f4a05'],
  ['#0e7490', '#cffafe', '#164e63'],
  ['#c026d3', '#fae8ff', '#86198f'],
  ['#dc2626', '#fee2e2', '#7f1d1d'],
  ['#65a30d', '#ecfccb', '#3f6212'],
]

function hashId(text: string): number {
  let hash = 2166136261
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index)
    hash = Math.imul(hash, 16777619)
  }
  return hash >>> 0
}

/** Four geometric placements: circle/square occupy different quadrants. */
const PATTERNS: ReadonlyArray<readonly [number, number, number, number]> = [
  [8, 8, 12, 12],
  [14, 8, 2, 12],
  [7, 14, 12, 2],
  [15, 15, 1, 1],
]

export function Avatar({ seed, size = 22 }: { seed: string; size?: number }) {
  const hash = hashId(seed)
  const [base, light, dark] = PALETTE[hash % PALETTE.length] ?? PALETTE[0]!
  const [circleX, circleY, rectX, rectY] = PATTERNS[(hash >> 3) % PATTERNS.length] ?? PATTERNS[0]!
  return (
    <svg
      viewBox="0 0 22 22"
      width={size}
      height={size}
      aria-hidden
      style={{ display: 'block', borderRadius: Math.max(4, Math.round(size * 0.32)) }}
    >
      <rect width="22" height="22" fill={base} />
      <circle cx={circleX} cy={circleY} r="4.6" fill={light} />
      <rect x={rectX} y={rectY} width="9" height="9" rx="2.5" fill={dark} />
    </svg>
  )
}
