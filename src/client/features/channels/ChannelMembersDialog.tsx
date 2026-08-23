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
import type { NativeActor } from '../../../native.ts'
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
  onClose(): void
}

function errorText(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason)
}

function MemberRow({ actor }: { actor: NativeActor }): JSX.Element {
  const seed = avatarSeed(actor.handle, actor.displayName)
  return (
    <div className={css.memberRow}>
      <AvatarChip kind={actor.kind} seed={seed} avatarUrl={actor.avatarDataUrl} aria-hidden="true" />
      <span className={css.memberName}>{actor.displayName}</span>
      <span className={css.memberHandle}>@{actor.handle}</span>
      {actor.kind === 'agent' && <span className={css.memberBadge}>AGENT</span>}
    </div>
  )
}

export function ChannelMembersDialog({ t, store, state, channelId, onClose }: ChannelMembersDialogProps): JSX.Element {
  const [stage, setStage] = useState<'list' | 'add'>('list')
  const [addingId, setAddingId] = useState<string | null>(null)
  const [failure, setFailure] = useState<string | null>(null)

  const members: NativeActor[] = useMemo(
    () => state.membersByChannel[channelId] ?? [],
    [state.membersByChannel, channelId],
  )
  const humans = members.filter(member => member.kind === 'user')
  const agents = members.filter(member => member.kind === 'agent')
  const available = useMemo(() => {
    const inside = new Set(members.map(member => member.id))
    return state.actors.filter(actor => actor.kind === 'agent' && !inside.has(actor.id))
  }, [state.actors, members])
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
      title={t('members.title', { count: members.length })}
      closeLabel={t('members.close')}
      contentClassName={css.dialogBody as string}
      footer={stage === 'list'
        ? (
          <Button variant="primary" size="sm" className={css.dialogWideButton as string} onClick={() => { setStage('add') }}>
            {t('members.add')}
          </Button>
        )
        : (
          <Button variant="outline" onClick={() => { setStage('list'); setFailure(null) }}>
            {t('members.back')}
          </Button>
        )}
    >
      {stage === 'list' && (
        <>
          {members.length === 0 && <p className={css.hint}>{t('members.emptyMembers')}</p>}
          {humans.length > 0 && (
            <section>
              <p className={css.dlgGroupLabel}>{t('members.humans')}</p>
              <div className={css.dlgMemberList}>
                {humans.map(member => <MemberRow key={member.id} actor={member} />)}
              </div>
            </section>
          )}
          {agents.length > 0 && (
            <section>
              <p className={css.dlgGroupLabel}>{t('members.agents')}</p>
              <div className={css.dlgMemberList}>
                {agents.map(member => <MemberRow key={member.id} actor={member} />)}
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
