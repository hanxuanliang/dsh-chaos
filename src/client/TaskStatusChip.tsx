import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { NativeTask } from '../native.ts'
import css from './ChaosPanel.module.css'

const MENU_ROW_HEIGHT_PX = 34

/**
 * Task status chip with a dropdown of the server's legal transitions. The
 * menu portals to document.body and positions itself `fixed` from the chip
 * rect: ancestors may create stacking contexts (transform/overflow in the
 * workspace grid), so an in-card absolute menu can be painted over or
 * clipped. Illegal transitions are simply not offered; when the viewer has
 * no update permission the chip renders as a plain label without a menu.
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
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const onDocMouseDown = (event: MouseEvent): void => {
      const target = event.target as Node
      if (rootRef.current?.contains(target) === true) return
      if (menuRef.current?.contains(target) === true) return
      setOpen(false)
    }
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') {
        event.stopPropagation()
        setOpen(false)
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
        const menuHeight = (transitions.length + 1) * MENU_ROW_HEIGHT_PX + 2
        const spaceBelow = window.innerHeight - rect.bottom
        const up = spaceBelow < menuHeight + 8 && rect.top > spaceBelow
        setPlaceUp(up)
        setMenuLeft(Math.max(8, Math.min(rect.left, window.innerWidth - 148)))
        setMenuOffset(up ? window.innerHeight - rect.top + 4 : rect.bottom + 4)
      }
    }
    setOpen(value => !value)
  }

  return (
    <span ref={rootRef} className={css.statusChipRoot}>
      <button
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
          >
            <div role="menuitem" aria-current className={css.statusMenuCurrent} data-status={task.status}>
              {task.status}
              <span aria-hidden>✓</span>
            </div>
            {transitions.map(status => (
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
            ))}
          </div>,
          document.body,
        )
        : null}
    </span>
  )
}
