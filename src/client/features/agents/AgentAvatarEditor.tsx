import { useRef, useState, type ChangeEvent, type JSX } from 'react'
import { Button, IconBrowseOutline16, IconTrashOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentProfile } from '../../../agent-settings-types.ts'
import type { ChaosClient } from '../../data/api.ts'
import type { ChaosTranslate } from '../../locales.ts'
import { AvatarChip } from '../../shared/ui/AvatarChip.tsx'
import { AVATAR_ACCEPT, normalizeAvatarFile } from './avatar-image.ts'
import css from './AgentAvatarEditor.module.css'

function avatarError(t: ChaosTranslate, reason: unknown): string {
  if (!(reason instanceof Error)) return t('agents.avatarFailed', { error: String(reason) })
  if (reason.message === 'type') return t('agents.avatarType')
  if (reason.message === 'size') return t('agents.avatarSize')
  if (reason.message === 'decode' || reason.message === 'read' || reason.message === 'encode') {
    return t('agents.avatarInvalid')
  }
  return t('agents.avatarFailed', { error: reason.message })
}

export function AgentAvatarEditor({ client, profile, onUpdated, t }: {
  client: ChaosClient
  profile: AgentProfile
  onUpdated(profile: AgentProfile): void
  t: ChaosTranslate
}): JSX.Element {
  const inputRef = useRef<HTMLInputElement | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const save = (avatarDataUrl: string | undefined): void => {
    setBusy(true)
    setError(null)
    client.updateAgentAvatar(profile.actor.id, avatarDataUrl, profile.profileVersion)
      .then(onUpdated, reason => { setError(avatarError(t, reason)) })
      .finally(() => { setBusy(false) })
  }

  const choose = (event: ChangeEvent<HTMLInputElement>): void => {
    const file = event.target.files?.[0]
    event.target.value = ''
    if (file === undefined) return
    setBusy(true)
    setError(null)
    normalizeAvatarFile(file)
      .then(avatarDataUrl => client.updateAgentAvatar(
        profile.actor.id,
        avatarDataUrl,
        profile.profileVersion,
      ))
      .then(onUpdated, reason => { setError(avatarError(t, reason)) })
      .finally(() => { setBusy(false) })
  }

  return (
    <div className={css.editor}>
      <AvatarChip
        handle={profile.actor.handle}
        displayName={profile.actor.displayName}
        avatarUrl={profile.actor.avatarDataUrl}
        size="xl"
      />
      <div className={css.copy}>
        <span className={css.label}>{t('agents.avatar')}</span>
        <span className={css.hint}>{t('agents.avatarHint')}</span>
        {error !== null && <span className={css.error} role="alert">{error}</span>}
      </div>
      <div className={css.actions}>
        <input
          ref={inputRef}
          className={css.file}
          type="file"
          accept={AVATAR_ACCEPT}
          aria-label={t('agents.avatarUpload')}
          disabled={busy}
          onChange={choose}
        />
        <Button variant="outline" size="sm" icon={<IconBrowseOutline16 size={16} />} disabled={busy} onClick={() => { inputRef.current?.click() }}>
          {busy ? t('agents.avatarSaving') : t('agents.avatarUpload')}
        </Button>
        {profile.actor.avatarDataUrl !== undefined && (
          <Button variant="outline" size="sm" icon={<IconTrashOutline16 size={16} />} disabled={busy} onClick={() => { save(undefined) }}>
            {t('agents.avatarRemove')}
          </Button>
        )}
      </div>
    </div>
  )
}
