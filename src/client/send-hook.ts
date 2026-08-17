/**
 * Official composer send wrap: if a Channel or Thread is selected, the
 * native box writes the collab ledger and does not prompt the Session.
 * Images fall through to the host (Channel ledger is text-only).
 */

interface ConversationSendFace {
  sendSession(
    session: unknown,
    text: string,
    imageIds: readonly string[],
    mode: string,
  ): Promise<void>
}

export interface ChannelSendController {
  getSnapshot(): {
    asTask: boolean
    railTab: 'channels' | 'agents' | 'thread'
    selectedTargetId?: string
    threadPanelId?: string
  }
  send(text: string): Promise<unknown>
  sendAsTask(text: string): Promise<void>
  sendToThread(text: string): Promise<void>
  setAsTask(asTask: boolean): void
}

const HOOK_MARKER = '__dshChaosSendHooked'

export function installChannelSendHook(
  conversation: unknown,
  controller: ChannelSendController,
): () => void {
  const face = conversation as ConversationSendFace | null
  if (face === null || typeof face !== 'object') return () => {}
  if (typeof face.sendSession !== 'function') return () => {}
  const bag = face as unknown as Record<string, unknown>
  if (bag[HOOK_MARKER] === true) return () => {}

  const original = face.sendSession.bind(face)
  face.sendSession = async (session, text, imageIds, mode): Promise<void> => {
    if (imageIds.length > 0) {
      return original(session, text, imageIds, mode)
    }
    const state = controller.getSnapshot()
    const intoThread = state.threadPanelId !== undefined
    const intoChannel = state.selectedTargetId !== undefined
    if (!intoThread && !intoChannel) {
      return original(session, text, imageIds, mode)
    }
    const trimmed = text.trim()
    if (trimmed === '') return
    if (intoThread) {
      await controller.sendToThread(trimmed)
      return
    }
    if (state.asTask) {
      await controller.sendAsTask(trimmed)
      controller.setAsTask(false)
      return
    }
    await controller.send(trimmed)
  }
  bag[HOOK_MARKER] = true
  return () => {
    face.sendSession = original
    delete bag[HOOK_MARKER]
  }
}
