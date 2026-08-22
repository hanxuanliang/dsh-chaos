import { useRef, useState, type ChangeEvent, type JSX } from 'react'
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

export function AgentAvatarEditor({ client, profile, onUpdated, onError, t }: {
  client: ChaosClient
  profile: AgentProfile
  onUpdated(profile: AgentProfile): void
  onError(error: string | null): void
  t: ChaosTranslate
}): JSX.Element {
  const inputRef = useRef<HTMLInputElement | null>(null)
  const [busy, setBusy] = useState(false)

  const choose = (event: ChangeEvent<HTMLInputElement>): void => {
    const file = event.target.files?.[0]
    event.target.value = ''
    if (file === undefined) return
    setBusy(true)
    onError(null)
    normalizeAvatarFile(file)
      .then(avatarDataUrl => client.updateAgentAvatar(
        profile.actor.id,
        avatarDataUrl,
        profile.profileVersion,
      ))
      .then(onUpdated, reason => { onError(avatarError(t, reason)) })
      .finally(() => { setBusy(false) })
  }

  return (
    <>
      <button
        type="button"
        className={css.upload}
        aria-label={busy ? t('agents.avatarSaving') : t('agents.avatarUpload')}
        aria-busy={busy}
        title={t('agents.avatarUpload')}
        disabled={busy}
        onClick={() => { inputRef.current?.click() }}
      >
        <AvatarChip
          handle={profile.actor.handle}
          displayName={profile.actor.displayName}
          avatarUrl={profile.actor.avatarDataUrl}
          size="xl"
        />
      </button>
      <input
        ref={inputRef}
        className={css.file}
        type="file"
        accept={AVATAR_ACCEPT}
        aria-label={t('agents.avatarUpload')}
        disabled={busy}
        onChange={choose}
      />
    </>
  )
}
