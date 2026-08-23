/**
 * Create-channel dialog (spec §1.1; Modal usage follows the existing
 * AgentCreateDialog.tsx). name.trim must be non-empty after stripping any
 * leading '#'; duplicate names are blocked inline (the backend allows them,
 * product side does not). Confirm = channel.create → member.add per picked
 * initial member → enter the new channel; any RPC failure stays inline with
 * the filled form in place.
 */
import { useMemo, useState, type JSX } from 'react'
import { Button, Input, Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { CollabStore, CollabStoreSnapshot } from '../../data/store.ts'
import type { ChaosTranslate } from '../../locales.ts'
import { avatarSeed } from '../../shared/avatar.ts'
import css from './DialogSkin.module.css'
import { AvatarChip } from '../../shared/ui/AvatarChip.tsx'

export interface ChannelCreateDialogProps {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  onClose(): void
}

function errorText(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason)
}

export function ChannelCreateDialog({ t, store, state, onClose }: ChannelCreateDialogProps): JSX.Element {
  const [name, setName] = useState('')
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set())
  const [submitting, setSubmitting] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)

  const agents = useMemo(() => state.actors.filter(actor => actor.kind === 'agent'), [state.actors])
  const trimmed = name.replace(/^#+/, '').trim()
  const duplicate = trimmed !== ''
    && state.channels.some(channel => channel.name.trim().toLowerCase() === trimmed.toLowerCase())
  const canSubmit = trimmed !== '' && !duplicate && !submitting

  const toggle = (memberId: string): void => {
    setSelected((previous) => {
      const next = new Set(previous)
      if (next.has(memberId)) next.delete(memberId)
      else next.add(memberId)
      return next
    })
    setFailure(null)
  }

  const submit = (): void => {
    if (!canSubmit) return
    setSubmitting(true)
    setFailure(null)
    void (async (): Promise<void> => {
      try {
        const target = await store.createChannel(trimmed)
        for (const memberId of selected) await store.memberAdd(target.id, memberId)
        store.setActiveChannel(target.id)
        onClose()
      } catch (reason) {
        // Keep the form; the channel (plus any members added so far) already
        // exists server-side and in the rail, which the snapshot reread will
        // reconcile — the inline error says so instead of hiding it.
        setFailure(errorText(reason))
        setSubmitting(false)
      }
    })()
  }

  return (
    <Modal
      open
      onClose={() => { if (!submitting) onClose() }}
      title={t('channelCreate.title')}
      closeLabel={t('channelCreate.close')}
      contentClassName={css.dialogBody as string}
      footer={(
        <>
          <Button variant="outline" disabled={submitting} onClick={onClose}>{t('channelCreate.cancel')}</Button>
          <Button variant="primary" disabled={!canSubmit} onClick={submit}>
            {submitting ? t('channelCreate.creating') : t('channelCreate.confirm')}
          </Button>
        </>
      )}
    >
      <label className={css.field} htmlFor="chaos-channel-create-name">
        <span className={css.labelText}>{t('channelCreate.name')}<em className={css.req} aria-hidden="true">*</em></span>
        <Input
          id="chaos-channel-create-name"
          className={css.input as string}
          value={name}
          onChange={(event) => { setName(event.target.value); setFailure(null) }}
          maxLength={64}
          placeholder={t('channelCreate.namePlaceholder')}
          autoComplete="off"
          autoFocus
          spellCheck={false}
          disabled={submitting}
        />
        {duplicate
          ? <small className={css.error} role="alert">{t('channelCreate.nameExists')}</small>
          : <small className={css.hint}>{t('channelCreate.nameHint')}</small>}
      </label>

      <div className={css.field}>
        <span className={css.labelText}>{t('channelCreate.members')}</span>
        {agents.length === 0 && <small className={css.hint}>{t('channelCreate.membersEmpty')}</small>}
        {agents.length > 0 && (
          <div className={css.memberPick} role="group" aria-label={t('channelCreate.members')}>
            {agents.map((agent) => {
              const seed = avatarSeed(agent.handle, agent.displayName)
              const checked = selected.has(agent.id)
              return (
                <label key={agent.id} className={css.memberRow} data-checked={checked || undefined}>
                  <input
                    type="checkbox"
                    checked={checked}
                    disabled={submitting}
                    onChange={() => { toggle(agent.id) }}
                  />
                  <AvatarChip kind="agent" seed={seed} avatarUrl={agent.avatarDataUrl} aria-hidden="true" />
                  <span className={css.memberName}>{agent.displayName}</span>
                  <span className={css.memberHandle}>@{agent.handle}</span>
                </label>
              )
            })}
          </div>
        )}
      </div>

      {failure !== null && <p className={css.error} role="alert">{t('channelCreate.failed', { error: failure })}</p>}
    </Modal>
  )
}
