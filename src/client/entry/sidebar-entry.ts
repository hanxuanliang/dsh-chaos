/**
 * Sidebar entry row for the collab panel, injected directly under the host
 * "New Session" button (mnemon paradigm: DOM injection + MutationObserver
 * self-heal). Sits in the same family stack as the mnemon/taskboard/ssh
 * entries so the panel rows stay grouped.
 */
import css from './CollabPanel.module.css'
import type { CollabPanelController } from './panel-controller.ts'

export const CHAOS_ENTRY_SELECTOR = '[data-dsh-chaos-entry]'

const FAMILY_SELECTOR = '[data-dsh-taskboard-entry], [data-dsh-ssh-entry], [data-dsh-mnemon-entry], [data-dsh-chaos-entry]'

function sidebarRoot(): HTMLElement | undefined {
  const column = document.querySelector<HTMLElement>('[data-pane="sidebar"], [class*="sidebarCol"]')
  if (column === null) return undefined
  return column.querySelector<HTMLElement>('[class*="logoRow"]')?.parentElement
    ?? (column.firstElementChild as HTMLElement | undefined)
}

function newSessionButton(root: HTMLElement): HTMLButtonElement | undefined {
  const nested = root.querySelector<HTMLButtonElement>('button[class*="newSession"]')
  if (nested !== null) return nested
  for (const child of root.children) {
    if (child.tagName === 'BUTTON') return child as HTMLButtonElement
  }
  return undefined
}

function createIcon(): SVGSVGElement {
  const namespace = 'http://www.w3.org/2000/svg'
  const icon = document.createElementNS(namespace, 'svg')
  icon.setAttribute('viewBox', '0 0 16 16')
  icon.setAttribute('width', '14')
  icon.setAttribute('height', '14')
  icon.setAttribute('fill', 'none')
  icon.setAttribute('stroke', 'currentColor')
  icon.setAttribute('stroke-width', '1.3')
  icon.setAttribute('stroke-linejoin', 'round')
  icon.setAttribute('aria-hidden', 'true')
  // Two overlapping diamonds: channels (a place) over conversation (the host).
  const back = document.createElementNS(namespace, 'path')
  back.setAttribute('d', 'M8 1.6 14.4 8 8 14.4 1.6 8Z')
  const front = document.createElementNS(namespace, 'path')
  front.setAttribute('d', 'M8 5 11 8 8 11 5 8Z')
  icon.append(back, front)
  return icon
}

function createEntry(controller: CollabPanelController): { entry: HTMLButtonElement; label: HTMLSpanElement; count: HTMLSpanElement } {
  const entry = document.createElement('button')
  entry.type = 'button'
  entry.dataset.dshChaosEntry = ''
  entry.className = css.entry ?? ''
  const icon = document.createElement('span')
  icon.className = css.entryIcon ?? ''
  icon.append(createIcon())
  const label = document.createElement('span')
  label.className = css.entryLabel ?? ''
  const count = document.createElement('span')
  count.className = css.entryCount ?? ''
  count.hidden = true
  entry.append(icon, label, count)
  entry.addEventListener('click', () => { controller.toggle() })
  return { entry, label, count }
}

function placeEntry(root: HTMLElement, entry: HTMLButtonElement): boolean {
  const button = newSessionButton(root)
  if (button === undefined) return false
  if (entry.parentElement === root) return true
  const row = button.closest('[class*="logoRow"]')
  const base = row !== null && row.parentElement === root ? row : button
  const family = Array.from(root.children).filter(
    (element): element is HTMLElement => element instanceof HTMLElement && element.matches(FAMILY_SELECTOR),
  )
  const anchor = family.at(-1)?.nextElementSibling ?? base.nextElementSibling
  root.insertBefore(entry, anchor)
  return true
}

/** Mount a self-healing official-style entry under the New Session row. */
export function mountCollabSidebarEntry(
  controller: CollabPanelController,
  t: (key: 'panel.entry') => string,
  subscribeLocale?: (listener: () => void) => () => void,
): () => void {
  const { entry, label, count } = createEntry(controller)
  let root: HTMLElement | undefined
  let placed = false

  const syncLabel = (): void => {
    const text = t('panel.entry')
    if (entry.getAttribute('aria-label') !== text) entry.setAttribute('aria-label', text)
    if (entry.title !== text) entry.title = text
    if (label.textContent !== text) label.textContent = text
    void count
  }

  const rootObserver = new MutationObserver(() => {
    if (root === undefined || !root.isConnected) {
      placed = false
      tryPlace()
      return
    }
    if (!root.contains(entry)) placed = placeEntry(root, entry)
  })

  const tryPlace = (): void => {
    syncLabel()
    if (root !== undefined && !root.isConnected) {
      rootObserver.disconnect()
      root = undefined
      placed = false
    }
    if (placed && document.body.contains(entry)) return
    if (placed) {
      rootObserver.disconnect()
      root = undefined
      placed = false
    }
    root ??= sidebarRoot()
    if (root === undefined) return
    placed = placeEntry(root, entry)
    if (placed) rootObserver.observe(root, { childList: true, subtree: true })
  }

  const waitObserver = new MutationObserver(tryPlace)
  waitObserver.observe(document.body, { childList: true, subtree: true })

  const syncActive = (): void => {
    if (controller.getSnapshot().open) entry.dataset.active = 'true'
    else delete entry.dataset.active
  }
  const unsubscribe = controller.subscribe(syncActive)
  const unsubscribeLocale = subscribeLocale?.(syncLabel) ?? (() => {})
  syncActive()
  tryPlace()

  return () => {
    waitObserver.disconnect()
    rootObserver.disconnect()
    unsubscribe()
    unsubscribeLocale()
    entry.remove()
  }
}
