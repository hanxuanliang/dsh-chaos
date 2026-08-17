import type { ClientContext } from '@deepseek-ai/dsh-client-runtime/client'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
import { ChaosClientController } from './controller.ts'
import { ChaosEntry, Workbench, type ChaosInjected } from './Workbench.tsx'
import type { NativeTask } from '../native.ts'

export const inject = ['slots', 'connection'] as const

/**
 * Collaboration workbench, rebuilt on official seams only:
 * a `sidebar.footer.action` entry beside Settings opens a `shell.overlay`
 * near-fullscreen workbench. No host private DOM, no sidecar services.
 */
export function apply(ctx: ClientContext): void {
  const connection = ctx.get('connection') as unknown as ConnectionHandle
  const controller = new ChaosClientController(connection.rpc)

  const face = (): ChaosInjected => ({
    hooks: { chaos: controller },
    ensure: () => controller.ensure(),
    openWorkbench: () => {
      controller.openWorkbench()
      void controller.ensure()
    },
    closeWorkbench: () => { controller.closeWorkbench() },
    selectTarget: targetId => controller.selectTarget(targetId),
    createChannel: name => controller.createChannel(name),
    send: text => controller.send(text).then(() => undefined),
    createThread: rootMessageId => controller.createThread(rootMessageId),
    openThreadPanel: threadTargetId => controller.openThreadPanel(threadTargetId),
    closeThreadPanel: () => { controller.closeThreadPanel() },
    sendToThread: text => controller.sendToThread(text),
    followThread: threadTargetId => controller.followThread(threadTargetId),
    unfollowThread: threadTargetId => controller.unfollowThread(threadTargetId),
    createTask: messageId => controller.createTask(messageId),
    claimTask: messageId => controller.claimTask(messageId),
    unclaimTask: (task: NativeTask) => controller.unclaimTask(task),
    updateTask: (task: NativeTask, status: NativeTask['status']) => controller.updateTask(task, status),
  })

  ctx.effect(() => () => { controller.dispose() }, 'dsh-chaos: controller')

  ctx.slots.inject('sidebar.footer.action', () => ctx.slots.register({
    name: 'sidebar.footer.action',
    id: 'dsh-chaos-entry',
    order: 10,
    inject: face,
  }, ChaosEntry))

  ctx.slots.inject('shell.overlay', () => ctx.slots.register({
    name: 'shell.overlay',
    id: 'dsh-chaos-workbench',
    order: 100,
    inject: face,
  }, Workbench))
}
