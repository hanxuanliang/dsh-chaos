/**
 * Center-column overlay mount for the collab panel (mnemon paradigm):
 * container injected as a direct child of the host conversation column,
 * visibility + sibling hiding driven by one html attribute, family mutex via
 * the dsh-panel-activate event, auto-close on sidebar session interactions.
 *
 * The takeover is full-pane (2026-08-19 user decision): while open, the
 * session header and native composer are covered too — the displayed surface
 * is the collab workspace alone. The current session's native composer is
 * still blocked through the official `ctx.conversation.blocks` face (P0-0
 * spike verdict) as invisible defense in depth.
 */
import { createRoot, type Root } from 'react-dom/client'
import type { ChaosClientContext, ChaosConversationService, ChaosSessionsService } from './dsh-compat.ts'
import type { ChaosTranslate } from '../locales.ts'
import { ChaosClient } from '../data/api.ts'
import { CollabStore } from '../data/store.ts'
import css from './CollabPanel.module.css'
import foundations from '../shared/styles/foundations.module.css'
import { classNames } from '../shared/class-names.ts'
import { CollabPanel } from './CollabPanel.tsx'
import { CollabPanelController } from './panel-controller.ts'
import { mountCollabSidebarEntry } from './sidebar-entry.ts'
import { CHAOS_NAVIGATE_CHANNEL_EVENT } from './navigation.ts'

export const CHAOS_PANEL_SELECTOR = '[data-dsh-chaos-panel]'

const CONVERSATION_COLUMN_SELECTOR = '[data-pane="conversation"], [class*="centerCol"]'
const ACTIVE_ATTR = 'data-dsh-chaos-active'
const ACTIVATE_EVENT = 'dsh-panel-activate'
const FAMILY_ATTRS = ['data-dsh-taskboard-active', 'data-dsh-ssh-active', 'data-dsh-mnemon-active']
const SIDEBAR_CONTEXT_SELECTOR = '[class*="sessionRow"], [class*="projectRow"], [class*="searchResultRow"], [class*="searchResultWorkspace"], [class*="newSession"]'

function sessionsOf(ctx: ChaosClientContext): ChaosSessionsService | undefined {
  return (ctx as unknown as { sessions?: ChaosSessionsService }).sessions
}

function conversationOf(ctx: ChaosClientContext): ChaosConversationService | undefined {
  return (ctx as unknown as { conversation?: ChaosConversationService }).conversation
}

function conversationColumn(): HTMLElement | undefined {
  return document.querySelector<HTMLElement>(CONVERSATION_COLUMN_SELECTOR) ?? undefined
}

function mountPanel(
  controller: CollabPanelController,
  ctx: ChaosClientContext,
  t: ChaosTranslate,
  store: CollabStore,
): () => void {
  let root: Root | undefined
  let container: HTMLDivElement | undefined

  const ensure = (): void => {
    if (container !== undefined && container.isConnected) return
    if (container !== undefined) {
      root?.unmount()
      root = undefined
      container = undefined
    }
    const body = conversationColumn()
    if (body === undefined) return
    container = document.createElement('div')
    container.dataset.dshChaosPanel = ''
    container.className = classNames(css.panel, foundations.scope)
    body.append(container)
    root = createRoot(container)
    root.render(
      <CollabPanel
        t={t}
        onClose={() => { controller.close() }}
        store={store}
        connection={ctx.connection}
        activeLocale={() => ctx.locale.getSnapshot().active}
      />,
    )
  }

  const waitObserver = new MutationObserver(ensure)
  waitObserver.observe(document.body, { childList: true, subtree: true })

  // --- composer block choreography ---
  // On open, raise the official block on the CURRENT session's composer
  // (reason = localized placeholder owned by us); on close, or when the
  // current session changes underneath, clear the previous block first.
  let blockedSession: string | undefined
  const syncComposerBlock = (): void => {
    const conversation = conversationOf(ctx)
    if (conversation === undefined) return
    const open = controller.getSnapshot().open
    const current = sessionsOf(ctx)?.list.getSnapshot().current
    if (blockedSession !== undefined && (blockedSession !== current || !open)) {
      conversation.blocks.set(blockedSession, undefined)
      blockedSession = undefined
    }
    // set() is idempotent on equal reasons, so locale switches and session
    // switches both funnel through this one write.
    if (open && current !== undefined) {
      conversation.blocks.set(current, { reason: t('panel.composerBlocked') })
      blockedSession = current
    }
  }
  const unsubscribeSessions = sessionsOf(ctx)?.list.subscribe(syncComposerBlock) ?? (() => {})
  const unsubscribeLocale = ctx.locale.subscribe(syncComposerBlock)

  let suppressCompatibilityClose = false
  const applyActive = (): void => {
    if (!controller.getSnapshot().open) {
      document.documentElement.removeAttribute(ACTIVE_ATTR)
      syncComposerBlock()
      return
    }

    // Current task-board/ssh/mnemon releases only close for one another's
    // event names. Send the two compatibility events (suppressing our own
    // close reaction) before announcing ourselves.
    suppressCompatibilityClose = true
    document.dispatchEvent(new CustomEvent(ACTIVATE_EVENT, { detail: 'ssh' }))
    document.dispatchEvent(new CustomEvent(ACTIVATE_EVENT, { detail: 'taskboard' }))
    document.dispatchEvent(new CustomEvent(ACTIVATE_EVENT, { detail: 'mnemon' }))
    suppressCompatibilityClose = false
    for (const attr of FAMILY_ATTRS) document.documentElement.removeAttribute(attr)
    document.documentElement.setAttribute(ACTIVE_ATTR, '')
    document.dispatchEvent(new CustomEvent(ACTIVATE_EVENT, { detail: 'chaos' }))
    syncComposerBlock()
  }

  const onOtherPanelActivate = (event: Event): void => {
    if (suppressCompatibilityClose || !controller.getSnapshot().open) return
    const detail = (event as CustomEvent<unknown>).detail
    if (detail !== 'chaos') controller.close()
  }

  const onSidebarContextClick = (event: MouseEvent): void => {
    if (!controller.getSnapshot().open) return
    const target = event.target
    if (target instanceof Element && target.closest(SIDEBAR_CONTEXT_SELECTOR) !== null) controller.close()
  }

  document.addEventListener('click', onSidebarContextClick, true)
  document.addEventListener(ACTIVATE_EVENT, onOtherPanelActivate)
  const unsubscribe = controller.subscribe(applyActive)
  applyActive()
  ensure()

  return () => {
    document.removeEventListener('click', onSidebarContextClick, true)
    document.removeEventListener(ACTIVATE_EVENT, onOtherPanelActivate)
    waitObserver.disconnect()
    unsubscribe()
    unsubscribeSessions()
    unsubscribeLocale()
    document.documentElement.removeAttribute(ACTIVE_ATTR)
    const conversation = conversationOf(ctx)
    if (conversation !== undefined && blockedSession !== undefined) {
      conversation.blocks.set(blockedSession, undefined)
      blockedSession = undefined
    }
    root?.unmount()
    root = undefined
    container?.remove()
    container = undefined
  }
}

/** Mount the sidebar entry row and the center-column panel as one unit. */
export function mountCollabWorkspace(ctx: ChaosClientContext, t: ChaosTranslate): () => void {
  if (typeof document === 'undefined' || typeof window === 'undefined') return () => {}
  const controller = new CollabPanelController()
  // The store (and its SSE connection) outlives panel visibility: it is
  // created with the mount, bootstraps lazily on first open, and stays
  // connected while closed so unread badges keep ticking (P0-3 input).
  const store = new CollabStore(new ChaosClient(ctx.connection))
  let storeStarted = false
  let pendingChannelId: string | undefined
  const applyPendingNavigation = (): void => {
    if (pendingChannelId === undefined || !store.getSnapshot().bootstrapped) return
    const targetId = pendingChannelId
    pendingChannelId = undefined
    if (store.getSnapshot().channels.some(channel => channel.id === targetId)) store.setActiveChannel(targetId)
  }
  const unsubscribeStoreTrigger = controller.subscribe(() => {
    if (!storeStarted && controller.getSnapshot().open) {
      storeStarted = true
      store.start()
    }
  })
  const unsubscribeNavigation = store.subscribe(applyPendingNavigation)
  const onNavigateChannel = (event: Event): void => {
    const targetId = (event as CustomEvent<unknown>).detail
    if (typeof targetId !== 'string' || targetId === '') return
    pendingChannelId = targetId
    controller.open()
    if (!storeStarted) {
      storeStarted = true
      store.start()
    }
    applyPendingNavigation()
  }
  document.addEventListener(CHAOS_NAVIGATE_CHANNEL_EVENT, onNavigateChannel)
  const disposeEntry = mountCollabSidebarEntry(controller, key => t(key), listener => ctx.locale.subscribe(listener))
  const disposePanel = mountPanel(controller, ctx, t, store)
  return () => {
    disposePanel()
    disposeEntry()
    unsubscribeStoreTrigger()
    unsubscribeNavigation()
    document.removeEventListener(CHAOS_NAVIGATE_CHANNEL_EVENT, onNavigateChannel)
    store.dispose()
  }
}
