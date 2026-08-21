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
import type { JSX, KeyboardEvent } from 'react'
import css from './PillTabs.module.css'

export interface PillTabItem {
  id: string
  label: string
  active?: boolean
  disabled?: boolean
  title?: string | undefined
  tabId?: string | undefined
  controls?: string | undefined
  onClick?(): void
}

function moveTab(event: KeyboardEvent<HTMLButtonElement>): void {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return
  const buttons = [...event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]:not(:disabled)') ?? []]
  if (buttons.length === 0) return
  event.preventDefault()
  const current = buttons.indexOf(event.currentTarget)
  const next = event.key === 'Home'
    ? 0
    : event.key === 'End'
      ? buttons.length - 1
      : (current + (event.key === 'ArrowRight' ? 1 : -1) + buttons.length) % buttons.length
  buttons[next]?.focus()
  buttons[next]?.click()
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
          id={item.tabId}
          type="button"
          role="tab"
          className={css.tab}
          aria-selected={item.active === true}
          aria-controls={item.controls}
          data-active={item.active === true ? 'true' : undefined}
          tabIndex={item.active === true ? 0 : -1}
          disabled={item.disabled === true}
          title={item.title}
          onClick={item.onClick}
          onKeyDown={moveTab}
        >
          {item.label}
        </button>
      ))}
    </span>
  )
}
