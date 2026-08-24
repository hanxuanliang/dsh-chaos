import { useState, type JSX } from 'react'
import { Button, IconArchiveOutline20, Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeTarget } from '../../../native.ts'
import type { CollabStore, CollabStoreSnapshot } from '../../data/store.ts'
import type { ChaosTranslate } from '../../locales.ts'
import { Field, TextInput } from '../../shared/ui/index.ts'
import css from './DialogSkin.module.css'

function errorText(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason)
}

export function ChannelEditDialog({ t, store, state, channel, onArchive, onClose }: {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channel: NativeTarget
  /** 归档动作从行内菜单移入 dialog 危险区(rail 只留悬停图标)。 */
  onArchive(): void
  onClose(): void
}): JSX.Element {
  const [name, setName] = useState(channel.name)
  const [description, setDescription] = useState(channel.description)
  const [submitting, setSubmitting] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)
  const trimmedName = name.replace(/^#+/, '').trim()
  const trimmedDescription = description.trim()
  const duplicate = trimmedName !== '' && state.channels.some(candidate => (
    candidate.id !== channel.id
    && candidate.name.trim().toLowerCase() === trimmedName.toLowerCase()
  ))
  const unchanged = trimmedName === channel.name && trimmedDescription === channel.description
  const canSubmit = trimmedName !== '' && trimmedDescription !== '' && !duplicate && !unchanged && !submitting

  const submit = (): void => {
    if (!canSubmit) return
    setSubmitting(true)
    setFailure(null)
    void store.updateChannel(channel.id, trimmedName, trimmedDescription, channel.version).then(() => {
      onClose()
    }, (reason: unknown) => {
      setFailure(errorText(reason))
      setSubmitting(false)
    })
  }

  return (
    <Modal
      open
      onClose={() => { if (!submitting) onClose() }}
      title={t('channelEdit.title')}
      closeLabel={t('channelEdit.close')}
      contentClassName={css.dialogBody as string}
      footer={(
        <>
          <Button variant="outline" disabled={submitting} onClick={onClose}>{t('channelEdit.cancel')}</Button>
          <Button variant="primary" disabled={!canSubmit} onClick={submit}>
            {submitting ? t('channelEdit.saving') : t('channelEdit.save')}
          </Button>
        </>
      )}
    >
      <div className={css.formStack} data-channel-form-stack="edit">
        <Field
          label={t('channelCreate.name')}
          required
          help={t('channelCreate.nameHint')}
          error={duplicate ? t('channelCreate.nameExists') : undefined}
        >
          <TextInput
            id="chaos-channel-edit-name"
            value={name}
            onChange={(event) => { setName(event.target.value); setFailure(null) }}
            maxLength={64}
            autoComplete="off"
            autoFocus
            spellCheck={false}
            disabled={submitting}
          />
        </Field>
        <Field
          label={t('channelCreate.description')}
          required
          help={t('channelCreate.descriptionHint')}
          meta={t('channelCreate.descriptionCount', { count: description.length })}
        >
          <textarea
            id="chaos-channel-edit-description"
            value={description}
            maxLength={280}
            disabled={submitting}
            onChange={(event) => { setDescription(event.target.value); setFailure(null) }}
          />
        </Field>
      </div>
      {failure !== null && <p className={css.error} role="alert">{t('channelEdit.failed', { error: failure })}</p>}
      <div className={css.dangerZone}>
        <button type="button" className={css.archiveLink} disabled={submitting} onClick={onArchive}>
          <IconArchiveOutline20 size={16} />
          {t('channel.archive')}
        </button>
      </div>
    </Modal>
  )
}
