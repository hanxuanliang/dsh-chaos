/**
 * AvatarChip — 会话/任务/预览中统一的 20px 种子字 chip原子。
 * 仿 pilot Button.tsx 体式: zero-store, props 驱动, .tsx+.module.css 成对。
 *
 * @param props.handle - 稳定 handle(种子之一)
 * @param props.displayName - 展示名(种子之二)
 * @param props.size - 'xs' 20px(默认, 现状主体) | 'md' 24px(将来需要时再开启)
 * @returns 一个圆形 chip 渲染 (首字 = avatarSeed 事实, 背景 = avatarSeed 背景)
 */
import type { JSX } from 'react'
import { avatarSeed, type AvatarSeed } from '../avatar.ts'
import css from './AvatarChip.module.css'

export function AvatarChip({ handle, displayName, avatarUrl, seed, size = 'xs', title }: {
  handle?: string
  displayName?: string
  avatarUrl?: string | undefined
  /** 调用方已有 seed(avatarSeed 已算了) 就传它; 否则由 handle/displayName 现场算。 */
  seed?: AvatarSeed
  size?: 'xs' | 'md' | 'lg' | 'xl' | 'xxsmall'
  title?: string | undefined
}): JSX.Element {
  const s: AvatarSeed = seed ?? avatarSeed(handle ?? '', displayName ?? handle ?? '')
  return (
    <span className={`${css.avatar} ${size === 'xs' ? css.xs : size === 'md' ? css.md : size === 'lg' ? css.lg : size === 'xl' ? css.xl : css.xxsmall}`} style={{ background: s.background }} title={title} data-avatar-chip aria-hidden="true">
      {avatarUrl === undefined
        ? s.initial
        : <img className={css.image} src={avatarUrl} alt="" draggable={false} />}
    </span>
  )
}
