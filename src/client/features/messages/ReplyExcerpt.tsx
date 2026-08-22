/**
 * ReplyExcerpt — reply 预览行原子: `{senderName}: {excerpt}`。
 * 两风格(rx 用户取 2026-08-20):
 *  - avatar: 前置 AvatarChip, 用于 stream 内独立 reply 卡
 *  - plain: 无头像, 用于 Activity 行卡内嵌(外层已包的行 button)
 * 调用方可包 button; 本原子坐标只一个字串 span 序列。
 */
import { AvatarChip } from '../../shared/ui/AvatarChip.tsx'
import css from './ReplyExcerpt.module.css'

export function ReplyExcerpt({ senderName, excerpt, withAvatar = false }: {
  senderName: string
  excerpt: string
  withAvatar?: boolean | undefined
}) {
  return (
    <span className={css.root} data-with-avatar={withAvatar ? 'true' : undefined}>
      {withAvatar && <AvatarChip handle={senderName} displayName={senderName} />}
      <span className={css.text}>{senderName}: {excerpt}</span>
    </span>
  )
}
