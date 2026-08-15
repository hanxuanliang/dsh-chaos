import type { ClientContext } from '@deepseek-ai/dsh-client-runtime/client'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import { ChaosPanel, type ChaosPanelInjected } from './ChaosPanel.tsx'
import { ChaosClientController } from './controller.ts'

export const inject = ['slots', 'connection']

/** Browser plugin: one additive shell overlay backed by Remote snapshot/SSE. */
export function apply(ctx: ClientContext): void {
  const connection = ctx.get('connection') as unknown as ConnectionHandle
  ctx.slots.inject('shell.overlay', () => {
    const controller = new ChaosClientController(connection.rpc)
    const dispose = ctx.slots.register({
      name: 'shell.overlay',
      id: 'dsh-chaos',
      order: 100,
      inject: (): ChaosPanelInjected => ({
        hooks: { chaos: controller },
        ensure: () => controller.ensure(),
        refresh: () => controller.refresh(),
        selectTarget: targetId => controller.selectTarget(targetId),
        createChannel: name => controller.createChannel(name),
        createDirect: peerId => controller.createDirect(peerId),
        addMember: (targetId, memberId) => controller.addMember(targetId, memberId),
        createThread: rootMessageId => controller.createThread(rootMessageId),
        send: text => controller.send(text),
        createTask: messageId => controller.createTask(messageId),
        claimTask: messageId => controller.claimTask(messageId),
        unclaimTask: task => controller.unclaimTask(task),
        updateTask: (task, status) => controller.updateTask(task, status),
      }),
    }, ChaosPanel)
    return () => {
      dispose()
      controller.dispose()
    }
  })
}
