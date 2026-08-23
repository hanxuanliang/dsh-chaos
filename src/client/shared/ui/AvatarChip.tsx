import type { JSX } from 'react'
import { avatarSeed, type AvatarSeed } from '../avatar.ts'
import css from './AvatarChip.module.css'

function pixelPath(pattern: readonly string[], value: '1' | '2'): string {
  const commands: string[] = []
  pattern.forEach((row, rowIndex) => {
    Array.from(row).forEach((cell, columnIndex) => {
      if (cell === value) commands.push(`M${String(columnIndex)} ${String(rowIndex)}h1v1h-1z`)
    })
  })
  return commands.join('')
}

export function AvatarChip({ handle, displayName, avatarUrl, seed, kind = 'user', size = 'xs', title }: {
  handle?: string
  displayName?: string
  avatarUrl?: string | undefined
  seed?: AvatarSeed
  kind?: 'user' | 'agent' | undefined
  size?: 'xs' | 'md' | 'lg' | 'xl' | 'xxsmall'
  title?: string | undefined
}): JSX.Element {
  const s: AvatarSeed = seed ?? avatarSeed(handle ?? '', displayName ?? handle ?? '')
  const generated = (
    <svg className={css.generated} viewBox="0 0 8 8" preserveAspectRatio="none" focusable="false" aria-hidden="true">
      <path d={pixelPath(s.pattern, '1')} fill="var(--dsw-alias-label-primary)" />
      <path d={pixelPath(s.pattern, '2')} fill={s.accent} />
    </svg>
  )
  return (
    <span className={`${css.avatar} ${size === 'xs' ? css.xs : size === 'md' ? css.md : size === 'lg' ? css.lg : size === 'xl' ? css.xl : css.xxsmall}`} style={{ background: s.background }} title={title} data-avatar-chip aria-hidden="true">
      {avatarUrl === undefined
        ? kind === 'agent' ? generated : s.initial
        : <img className={css.image} src={avatarUrl} alt="" draggable={false} />}
    </span>
  )
}
