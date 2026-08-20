/**
 * PillTabs — panel 顶/频道头/ Activity filter 共用的深色 pill 段选条。
 * (consumer-owned strip: pilot Pill 无此项 —— pilot README 自己把此类
 *  mkdir=消费方自定义 strip 记录)
 * 仿 pilot Button.tsx 体式: zero-store, props 驱动, .tsx+.module.css 成对。
 *
 * @param props.items - 段项(id/label/active/disabled?/title?/onClick?)
 * @param props.align - 'inline'(默认, margin-left:auto, 频道头居右语义)
 *                      | 'lead'(贴左, Activity filter row)
 * @param props.ariaLabel - tablist 的 aria-label
 */
import type { JSX } from 'react'
import css from './PillTabs.module.css'

export interface PillTabItem {
  id: string
  label: string
  active?: boolean
  disabled?: boolean
  title?: string | undefined
  onClick?(): void
}

export function PillTabs({ items, align = 'inline', ariaLabel }: {
  items: PillTabItem[]
  align?: 'inline' | 'lead'
  ariaLabel?: string | undefined
}): JSX.Element {
  return (
    <span className={css.tabs} data-align={align === 'lead' ? 'lead' : undefined} role="tablist" aria-label={ariaLabel}>
      {items.map((item) => (
        <button
          key={item.id}
          type="button"
          role="tab"
          className={css.tab}
          aria-selected={item.active === true}
          data-active={item.active === true ? 'true' : undefined}
          disabled={item.disabled === true}
          title={item.title}
          onClick={item.onClick}
        >
          {item.label}
        </button>
      ))}
    </span>
  )
}
