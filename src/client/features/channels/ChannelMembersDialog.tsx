/**
 * Channel members dialog (user 2026-08-19): opened from the header member
 * chip. Anatomy = plocal ChannelMembersDialog grafted onto the host Modal —
 * grouped Humans/Agents rows (avatar + name + @handle), an "Add member"
 * stage that lists agents not yet in the channel with per-row add action.
 * member.remove does not exist on the host RPC (P0 truth), so rows carry no
 * remove affordance. After a successful add the SSE membership_changed frame
 * refreshes the member list in place (store.reloadTargets).
 */
import { useMemo, useState, type JSX } from 'react'
import { Button, IconPlusOutline16, Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeTargetMember } from '../../../native.ts'
import type { CollabStore, CollabStoreSnapshot } from '../../data/store.ts'
import type { ChaosTranslate } from '../../locales.ts'
import { avatarSeed } from '../../shared/avatar.ts'
import css from './DialogSkin.module.css'
import { AvatarChip } from '../../shared/ui/AvatarChip.tsx'

export interface ChannelMembersDialogProps {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channelId: string
  readOnly?: boolean | undefined
  onClose(): void
}

function errorText(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason)
}

function MemberRow({ member, ownerLabel }: { member: NativeTargetMember; ownerLabel: string }): JSX.Element {
  const { actor } = member
  const seed = avatarSeed(actor.handle, actor.displayName)
  return (
    <div className={css.memberRow}>
      <AvatarChip kind={actor.kind} seed={seed} avatarUrl={actor.avatarDataUrl} aria-hidden="true" />
      <span className={css.memberName}>{actor.displayName}</span>
      <span className={css.memberHandle}>@{actor.handle}</span>
      {member.role === 'owner' && <span className={css.memberBadge}>{ownerLabel}</span>}
      {member.role !== 'owner' && actor.kind === 'agent' && <span className={css.memberBadge}>AGENT</span>}
    </div>
  )
}

export function ChannelMembersDialog({ t, store, state, channelId, readOnly = false, onClose }: ChannelMembersDialogProps): JSX.Element {
  const [stage, setStage] = useState<'list' | 'add'>('list')
  const [addingId, setAddingId] = useState<string | null>(null)
  const [failure, setFailure] = useState<string | null>(null)

  const memberships = useMemo(
    () => state.membershipsByChannel[channelId] ?? [],
    [state.membershipsByChannel, channelId],
  )
  const humans = memberships.filter(member => member.actor.kind === 'user')
  const agents = memberships.filter(member => member.actor.kind === 'agent')
  const available = useMemo(() => {
    const inside = new Set(memberships.map(member => member.actor.id))
    return state.actors.filter(actor => actor.kind === 'agent' && !inside.has(actor.id))
  }, [state.actors, memberships])
  const anyAgentAtAll = state.actors.some(actor => actor.kind === 'agent')

  const add = (actorId: string): void => {
    if (addingId !== null) return
    setAddingId(actorId)
    setFailure(null)
    void (async (): Promise<void> => {
      try {
        await store.memberAdd(channelId, actorId)
        setAddingId(null)
      } catch (reason) {
        setFailure(errorText(reason))
        setAddingId(null)
      }
    })()
  }

  return (
    <Modal
      open
      onClose={() => { if (addingId === null) onClose() }}
      title={t('members.title', { count: memberships.length })}
      closeLabel={t('members.close')}
      contentClassName={css.dialogBody as string}
      footer={stage === 'list' && !readOnly
        ? (
          <Button variant="primary" size="sm" className={css.dialogWideButton as string} onClick={() => { setStage('add') }}>
            {t('members.add')}
          </Button>
        )
        : stage === 'add' ? (
          <Button variant="outline" onClick={() => { setStage('list'); setFailure(null) }}>
            {t('members.back')}
          </Button>
        ) : undefined}
    >
      {stage === 'list' && (
        <>
          {memberships.length === 0 && <p className={css.hint}>{t('members.emptyMembers')}</p>}
          {humans.length > 0 && (
            <section>
              <p className={css.dlgGroupLabel}>{t('members.humans')}</p>
              <div className={css.dlgMemberList}>
                {humans.map(member => <MemberRow key={member.actor.id} member={member} ownerLabel={t('members.owner')} />)}
              </div>
            </section>
          )}
          {agents.length > 0 && (
            <section>
              <p className={css.dlgGroupLabel}>{t('members.agents')}</p>
              <div className={css.dlgMemberList}>
                {agents.map(member => <MemberRow key={member.actor.id} member={member} ownerLabel={t('members.owner')} />)}
              </div>
            </section>
          )}
        </>
      )}
      {stage === 'add' && (
        <>
          {available.length === 0 && (
            <p className={css.hint}>{anyAgentAtAll ? t('members.emptyAllIn') : t('members.emptyNoAgents')}</p>
          )}
          {available.length > 0 && (
            <div className={css.dlgMemberList}>
              {available.map((agent) => {
                const seed = avatarSeed(agent.handle, agent.displayName)
                const busy = addingId === agent.id
                /* plocal AddAgentList row: the whole row is the button. */
                return (
                  <button
                    key={agent.id}
                    type="button"
                    className={css.addRow}
                    disabled={addingId !== null}
                    aria-label={t('members.addOne', { name: agent.displayName })}
                    onClick={() => { add(agent.id) }}
                  >
                    <AvatarChip kind="agent" seed={seed} avatarUrl={agent.avatarDataUrl} aria-hidden="true" />
                    <span className={css.memberName}>{agent.displayName}</span>
                    <span className={css.memberHandle}>@{agent.handle}</span>
                    <span className={css.addRowIcon} aria-hidden="true">
                      {busy ? '…' : <IconPlusOutline16 size={14} />}
                    </span>
                  </button>
                )
              })}
            </div>
          )}
          {failure !== null && <p className={css.error} role="alert">{t('members.addFailed', { error: failure })}</p>}
        </>
      )}
    </Modal>
  )
}
