import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
const native = require('../native/dsh_chaos_core.node')
const root = await mkdtemp(join(tmpdir(), 'dsh-chaos-'))

try {
  const core = await native.openCollab(join(root, 'state.db'))
  const owner = await core.createUser('owner', 'Owner')
  // Same name is a no-op; a custom name is never overwritten.
  assert.deepEqual(await core.ensureUser('owner', 'Owner'), owner)
  const kept = await core.ensureUser('owner', 'Local Owner')
  assert.equal(kept.id, owner.id)
  assert.equal(kept.displayName, 'Owner')
  // Only leftover default 'Local User' migrates on the stable handle.
  const legacy = await core.createUser('local-user', 'Local User')
  const renamed = await core.ensureUser('local-user', 'Local Owner')
  assert.equal(renamed.id, legacy.id)
  assert.equal(renamed.displayName, 'Local Owner')
  const alpha = await core.createAgent('alpha', 'Alpha', join(root, 'alpha'))
  const beta = await core.createAgent('beta', 'Beta', join(root, 'beta'))
  const alphaProfile = await core.agentProfile(alpha.id)
  assert.equal(alphaProfile.version, '1')
  assert.equal(alphaProfile.actor.handle, 'alpha')
  assert.deepEqual(alphaProfile.charter, {
    schemaVersion: 1,
    summary: '',
    capabilities: [],
    constraints: [],
  })
  const updatedAlpha = await core.updateAgentProfile(alpha.id, 'Alpha Reviewer', {
    schemaVersion: 1,
    summary: 'Own review',
    capabilities: ['rust'],
    constraints: ['Do not address peers by UUID'],
  }, alphaProfile.version)
  assert.equal(updatedAlpha.version, '2')
  assert.equal(updatedAlpha.actor.displayName, 'Alpha Reviewer')
  const avatar = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII='
  const avatarAlpha = await core.updateAgentAvatar(alpha.id, avatar, updatedAlpha.version)
  assert.equal(avatarAlpha.version, '3')
  assert.equal(avatarAlpha.actor.avatarDataUrl, avatar)
  await assert.rejects(
    core.updateAgentProfile(alpha.id, 'Stale Alpha', alphaProfile.charter, alphaProfile.version),
    /agent_profile_version_conflict/,
  )
  const channel = await core.createChannel('design', 'Design collaboration', owner.id)
  await core.addMember(channel.id, alpha.id, owner.id)
  await core.addMember(channel.id, beta.id, owner.id)
  const identity = await core.identityContext(alpha.id, channel.id)
  assert.equal(identity.agent.actor.displayName, 'Alpha Reviewer')
  assert.equal(identity.agent.actor.avatarDataUrl, avatar)
  assert.equal(identity.target.name, 'design')
  assert.equal(identity.target.description, 'Design collaboration')
  assert.equal(identity.target.lifecycle, 'active')
  assert.equal(identity.members.find(member => member.actor.id === owner.id).role, 'owner')
  assert.equal(identity.members.find(member => member.actor.id === beta.id).actor.handle, 'beta')
  assert.equal(identity.members.find(member => member.actor.id === alpha.id).actor.avatarDataUrl, avatar)
  assert.deepEqual(await core.listTargetMemberships(alpha.id, channel.id), identity.members)

  const sent = await core.sendMessage({
    targetId: channel.id,
    authorId: owner.id,
    clientRequestId: 'native-smoke-send',
    text: 'review this',
  })
  assert.equal(sent.recipientIds.length, 2)
  assert.equal(sent.wakeAgentIds.length, 2)

  const binding = await core.bindRuntime(alpha.id, 'alpha-session-1', 'openai', 'codex', 'default')
  assert.deepEqual(await core.runtimeBinding(alpha.id), binding)
  assert.deepEqual(await core.runtimeBindingForSession(binding.sessionId), binding)
  assert.equal((await core.listRuntimeBindings()).length, 1)
  const wakes = await core.listPendingWakes(20)
  assert.equal(wakes.length, 1)
  assert.equal(wakes[0].pendingSeq, sent.message.seq)
  await core.markNotified(alpha.id, binding.generation, binding.sessionId, wakes[0].pendingSeq)
  assert.equal((await core.listPendingWakes(20)).length, 0)
  await core.rearmRuntimeWake(alpha.id, binding.generation, binding.sessionId)
  assert.equal((await core.listPendingWakes(20)).length, 1)
  await core.markNotified(alpha.id, binding.generation, binding.sessionId, wakes[0].pendingSeq)

  assert.deepEqual(
    await core.readMessage(alpha.id, channel.id, sent.message.id),
    sent.message,
  )
  assert.equal((await core.readMessages(alpha.id, channel.id, '0', 20)).length, 1)
  const inbox = await core.checkInbox(alpha.id, binding.generation, binding.sessionId, 20)
  assert.equal(inbox.messages.length, 1)
  assert.equal(inbox.contexts.length, 1)
  assert.equal(inbox.contexts[0].target.id, channel.id)
  assert.equal(inbox.contexts[0].members.find(member => member.actor.id === beta.id).actor.handle, 'beta')
  await core.markModelSeen(inbox.id, alpha.id, binding.generation, binding.sessionId)

  const direct = await core.createDirect(alpha.id, beta.id)
  assert.deepEqual(await core.createDirect(beta.id, alpha.id), direct)
  const directMessage = await core.sendMessage({
    targetId: direct.id,
    authorId: alpha.id,
    clientRequestId: 'native-direct-send',
    text: 'beta only',
  })
  assert.deepEqual(directMessage.recipientIds, [beta.id])

  const thread = await core.createThread(sent.message.id, alpha.id)
  assert.equal(thread.parentTargetId, channel.id)
  assert.equal(thread.rootMessageId, sent.message.id)
  const firstReply = await core.sendMessage({
    targetId: thread.id,
    authorId: alpha.id,
    clientRequestId: 'native-thread-1',
    text: 'owner follows the root',
  })
  assert.deepEqual(firstReply.recipientIds, [owner.id])
  await core.followThread(thread.id, beta.id)
  const secondReply = await core.sendMessage({
    targetId: thread.id,
    authorId: owner.id,
    clientRequestId: 'native-thread-2',
    text: 'both agents now follow',
  })
  assert.equal(secondReply.recipientIds.length, 2)
  assert.equal((await core.readMessages(alpha.id, thread.id, '0', 20)).length, 2)
  await core.unfollowThread(thread.id, beta.id)
  const afterUnfollow = await core.sendMessage({
    targetId: thread.id,
    authorId: alpha.id,
    clientRequestId: 'native-thread-3',
    text: 'beta no longer receives this',
  })
  assert.deepEqual(afterUnfollow.recipientIds, [owner.id])

  const task = await core.createTask(sent.message.id, owner.id)
  const claimed = await core.claimTask(task.messageId, alpha.id)
  assert.equal(claimed.status, 'in_progress')
  await assert.rejects(core.claimTask(task.messageId, beta.id), /task_already_claimed/)
  const review = await core.updateTaskStatus(
    task.messageId,
    alpha.id,
    'in_review',
    claimed.version,
  )
  const unclaimed = await core.unclaimTask(task.messageId, alpha.id, review.version)
  assert.equal(unclaimed.status, 'in_review')
  assert.equal(unclaimed.assigneeId, undefined)
  assert.equal((await core.listTasks(owner.id, channel.id)).length, 1)

  const activity = await core.inboxList(owner.id, 20)
  assert.equal(activity.activeCount, '2')
  const channelActivity = activity.items.find(item => item.conversationId === channel.id)
  const threadActivity = activity.items.find(item => item.conversationId === thread.id)
  assert.equal(channelActivity.task.status, 'in_review')
  assert.equal(threadActivity.title, 'review this')
  assert.equal(threadActivity.replyCount, '3')
  assert.equal(threadActivity.latestReply.excerpt, 'beta no longer receives this')
  await core.inboxDone(owner.id, channel.id, sent.message.seq)
  assert.equal((await core.inboxList(owner.id, 20)).activeCount, '1')
  await core.inboxDone(owner.id, channel.id, sent.message.seq)
  const revived = await core.sendMessage({
    targetId: channel.id,
    authorId: owner.id,
    clientRequestId: 'native-activity-revive',
    text: 'newer activity revives the row',
  })
  assert.equal(
    (await core.inboxList(owner.id, 20)).items[0].lastActivitySeq,
    revived.message.seq,
  )

  assert.equal((await core.listActors(owner.id)).length, 4)
  const snapshot = await core.snapshot(owner.id)
  assert.equal(snapshot.actor.id, owner.id)
  assert(snapshot.targets.some(target => target.id === channel.id))
  assert(snapshot.followedThreadIds.includes(thread.id))
  assert(snapshot.tasks.some(current => current.messageId === task.messageId))
  const changes = await core.listChanges(owner.id, '0', 500)
  assert(changes.some(change => change.kind === 'message_created'))
  assert(changes.some(change => change.kind === 'agent_profile_changed'))
  assert(changes.some(change => change.kind === 'task_updated'))
  assert(changes.some(change => change.kind === 'activity_done_changed'))
  const managed = await core.createChannel('managed', 'Initial purpose', owner.id)
  const updated = await core.updateChannel(
    managed.id,
    owner.id,
    'managed-renamed',
    'Updated purpose',
    managed.version,
  )
  assert.equal(updated.description, 'Updated purpose')
  const archived = await core.archiveChannel(updated.id, owner.id, updated.version)
  assert.equal(archived.lifecycle, 'archived')
  await assert.rejects(core.sendMessage({
    targetId: archived.id,
    authorId: owner.id,
    clientRequestId: 'archived-native-write',
    text: 'must fail',
  }), /target_not_writable/)
  const restored = await core.restoreChannel(archived.id, owner.id, archived.version)
  assert.equal(restored.lifecycle, 'active')
  const deletedChannel = await core.deleteChannel(restored.id, owner.id, restored.version)
  assert.equal(deletedChannel.lifecycle, 'deleted')
  assert(!(await core.snapshot(owner.id)).targets.some(target => target.id === managed.id))
  const retentionFloor = await core.pruneChangesBefore(Date.now() + 1)
  assert.equal((await core.snapshot(owner.id)).cursor, retentionFloor)
  await assert.rejects(
    core.listChanges(owner.id, '0', 500),
    /change_cursor_resync_required/,
  )
  await core.close()
} finally {
  await rm(root, { recursive: true, force: true })
}
