import type { ClientContext } from '@deepseek-ai/dsh-client-runtime/client'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
import { ChaosEntry, ChaosPanel, type ChaosPanelInjected } from './ChaosPanel.tsx'
import { ChaosClientController } from './controller.ts'

export const inject = ['slots', 'connection']

/** Browser plugin: one native Sidebar entry plus an additive frame-wide workspace. */
export function apply(ctx: ClientContext): void {
  const connection = ctx.get('connection') as unknown as ConnectionHandle
  const controller = new ChaosClientController(connection.rpc)
  const injectFace = (): ChaosPanelInjected => ({
    hooks: { chaos: controller },
    ensure: () => controller.ensure(),
    refresh: () => controller.refresh(),
    togglePeek: () => controller.togglePeek(),
    openWorkspace: () => controller.openWorkspace(),
    closeSurface: () => controller.closeSurface(),
    selectTarget: targetId => controller.selectTarget(targetId),
    createChannel: name => controller.createChannel(name),
    createDirect: peerId => controller.createDirect(peerId),
    addMember: (targetId, memberId) => controller.addMember(targetId, memberId),
    createThread: rootMessageId => controller.createThread(rootMessageId),
    followThread: threadTargetId => controller.followThread(threadTargetId),
    unfollowThread: threadTargetId => controller.unfollowThread(threadTargetId),
    send: text => controller.send(text),
    createTask: messageId => controller.createTask(messageId),
    claimTask: messageId => controller.claimTask(messageId),
    unclaimTask: task => controller.unclaimTask(task),
    updateTask: (task, status) => controller.updateTask(task, status),
  })

  ctx.effect(() => () => { controller.dispose() }, 'dsh-chaos: Client controller')

  ctx.slots.inject('shell.overlay', () => {
    return ctx.slots.register({
      name: 'shell.overlay',
      id: 'dsh-chaos-workspace',
      order: 100,
      inject: injectFace,
    }, ChaosPanel)
  })

  ctx.slots.inject('sidebar.footer.action', () => ctx.slots.register({
    name: 'sidebar.footer.action',
    id: 'dsh-chaos-entry',
    order: 80,
    label: '协作',
    inject: injectFace,
  }, ChaosEntry))
}
