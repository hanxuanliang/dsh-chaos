/** Push `#root` right so the official conversation shrinks. Pencil same trick. */

export const CHAOS_WORKBENCH_DOCK_ATTRIBUTE = 'chaosWorkbenchDockOwner'

export const WORKBENCH_MIN_WIDTH = 480
export const WORKBENCH_MAX_WIDTH = 800
export const WORKBENCH_DEFAULT_WIDTH = 640

export interface WorkbenchDockLease {
  update: (width: number) => void
  release: () => void
}

function dockWidth(width: number): string {
  return `${String(Math.max(0, Math.round(width)))}px`
}

export function claimWorkbenchDock(
  root: HTMLElement,
  owner: string,
  initialWidth: number,
  computedMarginRight = 0,
): WorkbenchDockLease | undefined {
  const existingOwner = root.dataset[CHAOS_WORKBENCH_DOCK_ATTRIBUTE]
  if (existingOwner !== undefined && existingOwner !== owner) return undefined
  if (existingOwner === undefined && (
    root.style.marginRight.trim() !== ''
    || (Number.isFinite(computedMarginRight) && computedMarginRight > 0.5)
  )) return undefined

  const previousMarginRight = root.style.marginRight
  const previousMinWidth = root.style.minWidth
  root.dataset[CHAOS_WORKBENCH_DOCK_ATTRIBUTE] = owner
  root.style.minWidth = '0'

  let released = false
  const update = (width: number): void => {
    if (released || root.dataset[CHAOS_WORKBENCH_DOCK_ATTRIBUTE] !== owner) return
    root.style.marginRight = dockWidth(width)
  }
  const release = (): void => {
    if (released) return
    released = true
    if (root.dataset[CHAOS_WORKBENCH_DOCK_ATTRIBUTE] !== owner) return
    root.style.marginRight = previousMarginRight
    root.style.minWidth = previousMinWidth
    delete root.dataset[CHAOS_WORKBENCH_DOCK_ATTRIBUTE]
  }

  update(initialWidth)
  return { update, release }
}
