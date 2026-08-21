import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import { IconChevronLeftOutline14, IconCodeOutline16, IconFolderOpenOutline16, Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentProfile, AgentWorkspaceEntry, AgentWorkspaceFile } from '../agent-settings-types.ts'
import { ChaosClient } from './api.ts'
import type { ChaosTranslate } from './locales.ts'
import css from './AgentWorkspaceDialog.module.css'

export function AgentWorkspaceDialog({ connection, profile, onClose, t }: {
  connection: ConnectionHandle
  profile: AgentProfile
  onClose(): void
  t: ChaosTranslate
}): JSX.Element {
  const client = useMemo(() => new ChaosClient(connection), [connection])
  const request = useRef(0)
  const [dir, setDir] = useState('')
  const [entries, setEntries] = useState<AgentWorkspaceEntry[]>([])
  const [file, setFile] = useState<AgentWorkspaceFile | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const loadDirectory = useCallback((path: string): void => {
    const current = ++request.current
    setLoading(true)
    setError(null)
    setFile(null)
    client.agentWorkspace(profile.actor.id, path).then(rows => {
      if (request.current !== current) return
      setDir(path)
      setEntries(rows)
      setLoading(false)
    }, (reason: unknown) => {
      if (request.current !== current) return
      setError(reason instanceof Error ? reason.message : String(reason))
      setLoading(false)
    })
  }, [client, profile.actor.id])

  useEffect(() => {
    loadDirectory('')
    return () => { request.current += 1 }
  }, [loadDirectory])

  const openFile = (path: string): void => {
    const current = ++request.current
    setLoading(true)
    setError(null)
    client.agentWorkspaceFile(profile.actor.id, path).then(value => {
      if (request.current !== current) return
      setFile(value)
      setLoading(false)
    }, (reason: unknown) => {
      if (request.current !== current) return
      setError(reason instanceof Error ? reason.message : String(reason))
      setLoading(false)
    })
  }

  const parent = dir.split('/').slice(0, -1).join('/')
  return (
    <Modal open onClose={onClose} title={t('workspace.title', { name: profile.actor.displayName })}
      closeLabel={t('workspace.close')} contentClassName={css.body as string}>
      <div className={css.toolbar}>
        <button type="button" className={css.iconButton} disabled={dir === '' || loading}
          aria-label={t('workspace.up')} onClick={() => { loadDirectory(parent) }}>
          <IconChevronLeftOutline14 size={14} />
        </button>
        <span className={css.path}>/{file?.path ?? dir}</span>
      </div>
      {error !== null && <p className={css.error} role="alert">{t('workspace.failed', { error })}</p>}
      {loading && <p className={css.state} role="status">{t('workspace.loading')}</p>}
      {!loading && file !== null && (
        <div className={css.preview}>
          <button type="button" className={css.back} onClick={() => { loadDirectory(dir) }}>{t('workspace.back')}</button>
          {file.binary
            ? <p className={css.state}>{t('workspace.binary')}</p>
            : <pre>{file.content ?? ''}</pre>}
          {file.truncated && <p className={css.state}>{t('workspace.truncated')}</p>}
        </div>
      )}
      {!loading && file === null && entries.length === 0 && <p className={css.state}>{t('workspace.empty')}</p>}
      {!loading && file === null && entries.length > 0 && <div className={css.list} role="list">
        {entries.map(entry => <button key={entry.path} type="button" role="listitem" className={css.entry}
          disabled={entry.kind === 'symlink'} onClick={() => { entry.kind === 'directory' ? loadDirectory(entry.path) : openFile(entry.path) }}>
          {entry.kind === 'directory' ? <IconFolderOpenOutline16 size={16} /> : <IconCodeOutline16 size={16} />}
          <span>{entry.name}</span>
          <small>{entry.kind === 'directory' ? t('workspace.folder') : `${entry.size} B`}</small>
        </button>)}
      </div>}
    </Modal>
  )
}
