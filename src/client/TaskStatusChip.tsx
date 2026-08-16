import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { NativeTask } from '../native.ts'
import css from './ChaosPanel.module.css'

const MENU_ROW_HEIGHT_PX = 30
const ALL_STATUSES: readonly NativeTask['status'][] = ['todo', 'in_progress', 'in_review', 'done']

/**
 * Task status chip with a dropdown of the full status set: the current
 * status is checkmarked, the server's legal transitions are clickable, and
 * illegal ones render disabled (no extra prose). The menu portals to
 * document.body and positions itself `fixed` from the chip rect: ancestors
 * may create stacking contexts (transform/overflow in the workspace grid),
 * so an in-card absolute menu can be painted over or clipped. Keyboard:
 * Arrow/Home/End move across enabled items, Enter/Space activates, Escape
 * closes and focus returns to the chip. When the viewer has no update
 * permission the chip renders as a plain label without a menu.
 */
export function TaskStatusChip({
  task,
  transitions,
  canUpdate,
  pending,
  onUpdate,
}: {
  task: NativeTask
  transitions: readonly NativeTask['status'][]
  canUpdate: boolean
  pending: boolean
  onUpdate: (status: NativeTask['status']) => void
}) {
  const [open, setOpen] = useState(false)
  const [placeUp, setPlaceUp] = useState(false)
  const [menuLeft, setMenuLeft] = useState(0)
  const [menuOffset, setMenuOffset] = useState(0)
  const rootRef = useRef<HTMLSpanElement>(null)
  const chipRef = useRef<HTMLButtonElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)

  const closeMenu = (restoreFocus: boolean): void => {
    setOpen(false)
    if (restoreFocus) chipRef.current?.focus()
  }

  useEffect(() => {
    if (!open) return
    // Focus lands on the first enabled item when the menu opens.
    const first = menuRef.current?.querySelector<HTMLButtonElement>('[role="menuitem"]:not([disabled])')
    first?.focus()
    const onDocMouseDown = (event: MouseEvent): void => {
      const target = event.target as Node
      if (rootRef.current?.contains(target) === true) return
      if (menuRef.current?.contains(target) === true) return
      setOpen(false)
    }
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') {
        event.stopPropagation()
        closeMenu(true)
      }
    }
    // The portal does not follow scroll; closing beats floating detached.
    const onScrollOrResize = (): void => { setOpen(false) }
    document.addEventListener('mousedown', onDocMouseDown)
    document.addEventListener('keydown', onKeyDown, true)
    window.addEventListener('scroll', onScrollOrResize, true)
    window.addEventListener('resize', onScrollOrResize)
    return () => {
      document.removeEventListener('mousedown', onDocMouseDown)
      document.removeEventListener('keydown', onKeyDown, true)
      window.removeEventListener('scroll', onScrollOrResize, true)
      window.removeEventListener('resize', onScrollOrResize)
    }
  }, [open])

  const toggleOpen = (event: React.MouseEvent): void => {
    event.preventDefault()
    event.stopPropagation()
    if (!open) {
      const rect = rootRef.current?.getBoundingClientRect()
      if (rect !== undefined) {
        const menuHeight = ALL_STATUSES.length * MENU_ROW_HEIGHT_PX + 2
        const spaceBelow = window.innerHeight - rect.bottom
        const up = spaceBelow < menuHeight + 8 && rect.top > spaceBelow
        setPlaceUp(up)
        setMenuLeft(Math.max(8, Math.min(rect.left, window.innerWidth - 148)))
        setMenuOffset(up ? window.innerHeight - rect.top + 4 : rect.bottom + 4)
      }
    }
    setOpen(value => !value)
  }

  const onMenuKeyDown = (event: React.KeyboardEvent): void => {
    const items = Array.from(
      menuRef.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not([disabled])') ?? [],
    )
    if (items.length === 0) return
    const active = document.activeElement as HTMLButtonElement | null
    const index = items.indexOf(active as HTMLButtonElement)
    let next: number | undefined
    if (event.key === 'ArrowDown') next = index < 0 ? 0 : (index + 1) % items.length
    else if (event.key === 'ArrowUp') next = index < 0 ? items.length - 1 : (index - 1 + items.length) % items.length
    else if (event.key === 'Home') next = 0
    else if (event.key === 'End') next = items.length - 1
    else if (event.key === 'Tab') { setOpen(false); return }
    else return
    event.preventDefault()
    items[next]?.focus()
  }

  const legal = new Set(transitions)

  return (
    <span ref={rootRef} className={css.statusChipRoot}>
      <button
        ref={chipRef}
        type="button"
        className={css.statusChip}
        data-status={task.status}
        aria-haspopup={canUpdate ? 'menu' : undefined}
        aria-expanded={canUpdate ? open : undefined}
        aria-label={`Task #${task.number} 状态 ${task.status}${canUpdate ? '，点击修改' : ''}`}
        disabled={pending || !canUpdate}
        onClick={toggleOpen}
      >
        {task.status}
        {canUpdate && <span aria-hidden className={css.statusChipCaret}>▾</span>}
      </button>
      {open && canUpdate
        ? createPortal(
          <div
            ref={menuRef}
            role="menu"
            className={css.statusMenu}
            style={{
              left: menuLeft,
              ...(placeUp ? { bottom: menuOffset } : { top: menuOffset }),
            }}
            onKeyDown={onMenuKeyDown}
          >
            {ALL_STATUSES.map(status => {
              if (status === task.status) {
                return (
                  <button
                    key={status}
                    type="button"
                    role="menuitem"
                    disabled
                    aria-current
                    className={css.statusMenuOption}
                    data-status={status}
                    data-current
                  >
                    {status}
                    <span aria-hidden>✓</span>
                  </button>
                )
              }
              if (!legal.has(status)) {
                return (
                  <button
                    key={status}
                    type="button"
                    role="menuitem"
                    disabled
                    className={css.statusMenuOption}
                    data-status={status}
                    data-disabled
                  >
                    {status}
                  </button>
                )
              }
              return (
                <button
                  key={status}
                  type="button"
                  role="menuitem"
                  className={css.statusMenuOption}
                  data-status={status}
                  onClick={event => {
                    event.preventDefault()
                    event.stopPropagation()
                    setOpen(false)
                    onUpdate(status)
                  }}
                >
                  {status}
                </button>
              )
            })}
          </div>,
          document.body,
        )
        : null}
    </span>
  )
}
