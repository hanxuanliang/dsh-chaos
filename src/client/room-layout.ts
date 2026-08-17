/** When a room is open, pin the official composer to the bottom like an IM. */

const STYLE_ID = 'dsh-chaos-room-layout'
const HIDDEN = 'data-chaos-hero-hidden'

function ensureStyle(): void {
  if (document.getElementById(STYLE_ID) !== null) return
  const style = document.createElement('style')
  style.id = STYLE_ID
  style.textContent = `
    [data-chaos-room="open"] [data-composer-seat] {
      position: fixed !important;
      left: var(--dsh-official-sidebar-width, 260px);
      right: 0;
      bottom: 0;
      z-index: 37;
      background: var(--dsw-alias-bg-base);
    }
    [data-chaos-hero-hidden] { display: none !important; }
  `
  document.head.appendChild(style)
}

function composerSeat(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[data-composer-seat]')
}

function isKeep(node: HTMLElement): boolean {
  return node.hasAttribute('data-composer-card')
    || node.hasAttribute('data-chaos-dock')
    || node.tagName === 'TEXTAREA'
    || node.getAttribute('contenteditable') === 'true'
}

function containsKeep(node: HTMLElement): boolean {
  return isKeep(node)
    || node.querySelector('[data-composer-card], [data-chaos-dock], textarea, [contenteditable="true"]') !== null
}

function hideTree(root: HTMLElement): HTMLElement[] {
  const hidden: HTMLElement[] = []
  const hide = (node: HTMLElement): void => {
    node.setAttribute(HIDDEN, '')
    hidden.push(node)
  }
  const walk = (node: HTMLElement): void => {
    if (isKeep(node)) return
    if (!containsKeep(node)) {
      hide(node)
      return
    }
    for (const child of Array.from(node.children)) {
      if (child instanceof HTMLElement) walk(child)
    }
  }
  walk(root)
  return hidden
}

export function claimRoomLayout(): () => void {
  ensureStyle()
  const root = document.querySelector<HTMLElement>('[data-phase="hero"], [data-phase="active"], [data-phase="settling"]')
    ?? document.getElementById('root')
  const seat = composerSeat()
  const previousPhase = root?.dataset.phase
  const hidden = seat === null ? [] : hideTree(seat.parentElement ?? seat)
  if (root !== null) {
    root.dataset.phase = 'active'
    root.dataset.chaosRoom = 'open'
  }
  if (seat !== null) {
    document.documentElement.style.setProperty('--dsh-composer-height', `${String(seat.offsetHeight)}px`)
  }

  return () => {
    for (const node of hidden) node.removeAttribute(HIDDEN)
    if (root?.dataset.chaosRoom === 'open') {
      if (previousPhase === undefined) delete root.dataset.phase
      else root.dataset.phase = previousPhase
      delete root.dataset.chaosRoom
    }
    document.documentElement.style.removeProperty('--dsh-composer-height')
  }
}
