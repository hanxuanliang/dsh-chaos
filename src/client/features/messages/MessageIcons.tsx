/**
 * Message-only glyphs not available from the DSH primitive package. Keep this
 * set scoped to the feature; every host-provided icon is imported directly.
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

/** MessageStream 空膽 bubble。 */
export function IconBubble({ size = 24 }: { size?: number }): JSX.Element {
  return base({ size, strokeWidth: 1.3 }, <path d="M2.5 3.5h11v8h-7l-4 3v-11Z" />)
}

/** 回复枝 back-arrow (ThreadPreview/replyButton)。 */
export function IconReply(props: IconProps): JSX.Element {
  return base({ size: 11, strokeWidth: 1.4, ...props }, <path d="M6 11 2.5 7.5 6 4M2.5 7.5h6a3.5 3.5 0 0 1 3.5 3.5v2" />)
}
