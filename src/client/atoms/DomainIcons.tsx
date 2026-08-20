/**
 * DomainIcons — collab panel 自造内联图标集 (host Icon*Outline16 未提供的
 * 域件): chevron/members/join-arrow/bubble/reply/copy。 规则: 14px stroke
 * currentColor, 颜色随容器 token; 禁 lucide (style-guide §I)。
 */
import type { JSX } from 'react'

export interface IconProps {
  /** px 边长; 默认 14。 */
  size?: number
  /** strokeWidth; 默认 1.4。 */
  strokeWidth?: number
}

function base({ size = 14, strokeWidth = 1.4 }: IconProps, children: JSX.Element | JSX.Element[]): JSX.Element {
  return (
    <svg viewBox="0 0 16 16" width={size} height={size} fill="none" stroke="currentColor" strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {children}
    </svg>
  )
}

/** Channel rail 全局 collapse 指示 (旋转开合)。 */
export function IconChevron({ open, size = 10 }: { open: boolean; size?: number }): JSX.Element {
  return base({ size, strokeWidth: 2 }, <path d={open ? 'M4 6l4 4 4-4' : 'M6 4l4 4-4 4'} />)
}

/** ChannelHeader 右 member chip 人形标。 */
export function IconMembers(props: IconProps): JSX.Element {
  return base({ size: 14, strokeWidth: 1.3, ...props }, (
    <>
      <circle cx="6" cy="5" r="2.6" />
      <path d="M1.8 13.2c.6-2.4 2.2-3.6 4.2-3.6s3.6 1.2 4.2 3.6" />
      <path d="M10.3 7.6c1.4 0 2.6-1.1 2.6-2.6 0-.4-.1-.8-.2-1.2" />
      <path d="M11.6 13.2c.5-2 1.8-3 3.2-3" />
    </>
  ))
}

/** CollabPanel 空膽 join-arrow (select a channel)。 */
export function IconJoin({ size = 24 }: { size?: number }): JSX.Element {
  return base({ size, strokeWidth: 1.3 }, <path d="M9.5 2.5 14 8l-4.5 5.5M13.5 8H6M6 2.5 1.5 8 6 13.5" />)
}

/** MessageStream 空膽 bubble。 */
export function IconBubble({ size = 24 }: { size?: number }): JSX.Element {
  return base({ size, strokeWidth: 1.3 }, <path d="M2.5 3.5h11v8h-7l-4 3v-11Z" />)
}

/** 回复枝 back-arrow (ThreadPreview/replyButton)。 */
export function IconReply(props: IconProps): JSX.Element {
  return base({ size: 11, strokeWidth: 1.4, ...props }, <path d="M6 11 2.5 7.5 6 4M2.5 7.5h6a3.5 3.5 0 0 1 3.5 3.5v2" />)
}

/** MessageBody code-shell 复制标。 */
export function IconCopy(props: IconProps): JSX.Element {
  return base({ size: 14, strokeWidth: 1.6, ...props }, (
    <>
      <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
      <path d="M10.5 5.5V3.5A1.5 1.5 0 0 0 9 2H4a1.5 1.5 0 0 0-1.5 1.5V9A1.5 1.5 0 0 0 4 10.5h1.5" />
    </>
  ))
}
