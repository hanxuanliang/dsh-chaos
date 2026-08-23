import type { NativeActor } from '../../../native.ts'

/**
 * Resolve the Agent directory shown by a composer.
 *
 * Channel composers retain their bootstrap fallback to the actor directory.
 * Thread composers provide their parent Channel id and fail closed until that
 * Channel's membership is loaded, so a non-member Agent can never leak into
 * the Thread mention picker.
 */
export function resolveMentionAgents(
  actors: readonly NativeActor[],
  membersByChannel: Readonly<Record<string, NativeActor[]>>,
  targetId: string,
  parentChannelId?: string,
): NativeActor[] {
  const membershipTargetId = parentChannelId ?? targetId
  const members = membersByChannel[membershipTargetId]
    ?? (parentChannelId === undefined ? actors : [])
  return members.filter(actor => actor.kind === 'agent')
}
