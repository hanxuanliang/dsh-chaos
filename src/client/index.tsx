import type { ClientContext } from '@deepseek-ai/dsh-client-runtime/client'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
import { ChaosClientController } from './controller.ts'
import { ActivityEntry } from './Entry.tsx'
import { ConversationDock } from './Dock.tsx'
import type { ChaosInjected } from './surface.tsx'

export const inject = ['slots', 'connection'] as const

/**
 * Collaboration surface, rebuilt on official seams only: an `Activity` entry
 * beside Settings opens a docked right-side panel (shell.overlay) with the
 * authoritative Activity inbox; a card lands in the conversation view.
 * No host private DOM, no sidecar services, no shadowing of shipped UI.
 */
export function apply(ctx: ClientContext): void {
  const connection = ctx.get('connection') as unknown as ConnectionHandle
  const controller = new ChaosClientController(connection.rpc)

  const face = (): ChaosInjected => ({
    hooks: { chaos: controller },
    ensure: () => controller.ensure(),
    openActivity: () => { controller.openActivity() },
    closeDock: () => { controller.closeDock() },
    backToList: () => { controller.backToList() },
    openDock: targetId => controller.openDock(targetId),
    loadInbox: () => controller.loadInbox(),
    loadMoreInbox: () => {
      const cursor = controller.getSnapshot().inbox.nextCursor
      return cursor === undefined ? Promise.resolve() : controller.loadInbox(cursor)
    },
    markInboxDone: (targetId, throughSeq) => controller.markInboxDone(targetId, throughSeq),
    send: text => controller.send(text).then(() => undefined),
    createThread: rootMessageId => controller.createThread(rootMessageId),
    openThreadPanel: threadTargetId => controller.openThreadPanel(threadTargetId),
    closeThreadPanel: () => { controller.closeThreadPanel() },
    sendToThread: text => controller.sendToThread(text),
    followThread: threadTargetId => controller.followThread(threadTargetId),
    unfollowThread: threadTargetId => controller.unfollowThread(threadTargetId),
    createTask: messageId => controller.createTask(messageId),
  })

  ctx.effect(() => () => { controller.dispose() }, 'dsh-chaos: controller')

  ctx.slots.inject('sidebar.footer.action', () => ctx.slots.register({
    name: 'sidebar.footer.action',
    id: 'dsh-chaos-activity',
    order: 10,
    inject: face,
  }, ActivityEntry))

  ctx.slots.inject('shell.overlay', () => ctx.slots.register({
    name: 'shell.overlay',
    id: 'dsh-chaos-dock',
    order: 95,
    inject: face,
  }, ConversationDock))
}
