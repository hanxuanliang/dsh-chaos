/** ErrorBanner — plocal 错误 banner 原子: 红色二级描边 8px 圆角 10px 3 内边,
 * primary 错色 12px/18。 消费: 看板/activity 等顶层错误条 (乐观恢复用 retry
 * 的场景另配 retryButton, 本原子只描条本体)。 */
import type { JSX, ReactNode } from 'react'
import css from './ErrorBanner.module.css'

export function ErrorBanner({ children }: { children: ReactNode }): JSX.Element {
  return (
    <div className={css.banner} role="alert">
      {children}
    </div>
  )
}
