import type { ChaosClientController, ChaosClientState } from './controller.ts'
import css from './ChaosPanel.module.css'

const TAB_ATTR = 'data-dsh-chaos-left'
const INBOX_ATTR = 'data-dsh-chaos-inbox'

export interface ActivitySidebarActions {
  selectTarget: (targetId: string) => Promise<void>
  openThreadPanel: (threadTargetId: string) => Promise<void>
  ensure: () => Promise<void>
}

function sidebarRoot(): HTMLElement | undefined {
  const column = document.querySelector<HTMLElement>('[data-pane="sidebar"], [class*="sidebarCol"]')
  if (column === null) return undefined
  const logoOwner = column.querySelector<HTMLElement>('[class*="logoRow"]')?.parentElement
  return logoOwner ?? (column.firstElementChild as HTMLElement | undefined)
}

function newSessionButton(root: HTMLElement): HTMLButtonElement | undefined {
  const nested = root.querySelector<HTMLButtonElement>('button[class*="newSession"]')
  if (nested !== null) return nested
  for (const child of root.children) {
    if (child.tagName === 'BUTTON') return child as HTMLButtonElement
  }
  return undefined
}

function regionArea(root: HTMLElement): HTMLElement | undefined {
  return root.querySelector<HTMLElement>('[class*="regionArea"]') ?? undefined
}

function inboxRows(state: ChaosClientState): readonly { id: string; kind: 'channel' | 'thread'; label: string }[] {
  const followed = new Set(state.followedThreadIds)
  const channels = state.targets
    .filter(target => target.kind === 'channel')
    .map(target => ({ id: target.id, kind: 'channel' as const, label: `#${target.name}` }))
  const threads = state.targets
    .filter(target => target.kind === 'thread' && followed.has(target.id))
    .map(target => {
      const parent = state.targets.find(candidate => candidate.id === target.parentTargetId)
      return {
        id: target.id,
        kind: 'thread' as const,
        label: parent === undefined ? 'Thread' : `#${parent.name} · thread`,
      }
    })
  return [...channels, ...threads]
}

function paintInbox(
  inbox: HTMLElement,
  state: ChaosClientState,
  actions: ActivitySidebarActions,
): void {
  inbox.replaceChildren()
  const rows = inboxRows(state)
  if (rows.length === 0) {
    const empty = document.createElement('p')
    empty.className = css.activityEmpty ?? ''
    empty.textContent = '没有待看的房间或 Thread。'
    inbox.append(empty)
    return
  }
  for (const row of rows) {
    const button = document.createElement('button')
    button.type = 'button'
    button.className = css.activityRow ?? ''
    button.textContent = row.label
    if (row.id === state.selectedTargetId || row.id === state.threadPanelId) {
      button.dataset.active = 'true'
    }
    button.addEventListener('click', () => {
      if (row.kind === 'thread') void actions.openThreadPanel(row.id)
      else void actions.selectTarget(row.id)
    })
    inbox.append(button)
  }
}

function place(root: HTMLElement, tabs: HTMLElement, inbox: HTMLElement): boolean {
  const button = newSessionButton(root)
  const region = regionArea(root)
  if (button === undefined || region === undefined) return false
  const base = button.parentElement === root ? button : (button.closest('[class*="logoRow"]') ?? button)
  if (tabs.parentElement !== root || tabs.previousElementSibling !== base) {
    root.insertBefore(tabs, base.nextElementSibling)
  }
  if (inbox.parentElement !== root || inbox.previousElementSibling !== tabs) {
    root.insertBefore(inbox, tabs.nextElementSibling)
  }
  return true
}

/** Inject 会话 | Activity under New Session. Activity covers the official session tree. */
export function mountActivitySidebar(
  controller: ChaosClientController,
  actions: ActivitySidebarActions,
): () => void {
  if (typeof document === 'undefined') return () => {}
  if (document.querySelector(`[${TAB_ATTR}]`) !== null) return () => {}

  const tabs = document.createElement('div')
  tabs.setAttribute(TAB_ATTR, '')
  tabs.className = css.activityTabs ?? ''
  const sessions = document.createElement('button')
  sessions.type = 'button'
  sessions.className = css.activityTab ?? ''
  sessions.textContent = '会话'
  const activity = document.createElement('button')
  activity.type = 'button'
  activity.className = css.activityTab ?? ''
  activity.textContent = 'Activity'
  tabs.append(sessions, activity)

  const inbox = document.createElement('div')
  inbox.setAttribute(INBOX_ATTR, '')
  inbox.className = css.activityInbox ?? ''
  inbox.hidden = true

  const setPane = (pane: ChaosClientState['leftPane']): void => {
    controller.setLeftPane(pane)
  }
  sessions.addEventListener('click', () => { setPane('sessions') })
  activity.addEventListener('click', () => { setPane('activity') })

  const sync = (): void => {
    const state = controller.getSnapshot()
    const active = state.leftPane === 'activity'
    sessions.toggleAttribute('data-active', !active)
    activity.toggleAttribute('data-active', active)
    inbox.hidden = !active
    document.documentElement.toggleAttribute('data-dsh-chaos-activity', active)
    if (active) paintInbox(inbox, state, actions)
  }

  let root: HTMLElement | undefined
  let placed = false
  const rootObserver = new MutationObserver(() => {
    if (root === undefined || !root.isConnected) {
      placed = false
      tryPlace()
      return
    }
    if (!root.contains(tabs) || !root.contains(inbox)) placed = place(root, tabs, inbox)
  })
  const tryPlace = (): void => {
    if (root !== undefined && !root.isConnected) {
      rootObserver.disconnect()
      root = undefined
      placed = false
    }
    if (placed && document.body.contains(tabs) && document.body.contains(inbox)) return
    if (placed) {
      rootObserver.disconnect()
      root = undefined
      placed = false
    }
    root ??= sidebarRoot()
    if (root === undefined) return
    placed = place(root, tabs, inbox)
    if (placed) rootObserver.observe(root, { childList: true, subtree: true })
  }

  const waitObserver = new MutationObserver(() => { tryPlace() })
  waitObserver.observe(document.body, { childList: true, subtree: true })
  const stop = controller.subscribe(sync)
  void actions.ensure()
  sync()
  tryPlace()

  return () => {
    waitObserver.disconnect()
    rootObserver.disconnect()
    stop()
    tabs.remove()
    inbox.remove()
    document.documentElement.removeAttribute('data-dsh-chaos-activity')
  }
}
