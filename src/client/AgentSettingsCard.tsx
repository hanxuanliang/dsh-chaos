import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import { Button, IconPlusOutline16, RiskConfirmation } from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentPresetSummary, AgentProfile, CreatedAgent } from '../agent-settings-types.ts'
import { ChaosClient } from './api.ts'
import { AgentCreateDialog } from './AgentCreateDialog.tsx'
import { AgentWorkspaceDialog } from './AgentWorkspaceDialog.tsx'
import type { ChaosTranslate } from './locales.ts'
import { AgentDetail } from './blocks/AgentDetail.tsx'
import { AgentList } from './blocks/AgentList.tsx'
import streamCss from './blocks/MessageStream.module.css'
import css from './AgentSettingsCard.module.css'

export interface AgentSettingsCardProps { connection: ConnectionHandle; t: ChaosTranslate }
type Phase = 'loading' | 'ready' | 'error'
const SKELETON_ROWS = [0, 1, 2]
function errorText(reason: unknown): string { return reason instanceof Error ? reason.message : String(reason) }

/** Identity-first expandable Agent settings surface. */
export function AgentSettingsCard({ connection, t }: AgentSettingsCardProps): JSX.Element {
  const client = useMemo(() => new ChaosClient(connection), [connection])
  const request = useRef(0)
  const presetRequest = useRef(0)
  const [phase, setPhase] = useState<Phase>('loading')
  const [profiles, setProfiles] = useState<AgentProfile[]>([])
  const [selectedId, setSelectedId] = useState<string | undefined>(undefined)
  const [query, setQuery] = useState('')
  const [loadError, setLoadError] = useState<string | null>(null)
  const [refreshing, setRefreshing] = useState(false)
  const [presets, setPresets] = useState<AgentPresetSummary[]>([])
  const [presetsLoading, setPresetsLoading] = useState(true)
  const [presetsError, setPresetsError] = useState<string | null>(null)
  const [createOpen, setCreateOpen] = useState(false)
  const [workspaceProfile, setWorkspaceProfile] = useState<AgentProfile | null>(null)
  const [deleteProfile, setDeleteProfile] = useState<AgentProfile | null>(null)
  const [deleteAcknowledged, setDeleteAcknowledged] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [actionError, setActionError] = useState<string | null>(null)

  const load = useCallback((initial: boolean): void => {
    const current = ++request.current
    if (initial) setPhase('loading'); else setRefreshing(true)
    client.agentProfiles().then(rows => {
      if (request.current !== current) return
      setProfiles(rows)
      setSelectedId(previous => previous !== undefined && rows.some(row => row.actor.id === previous)
        ? previous
        : undefined)
      setLoadError(null)
      setPhase('ready')
    }, reason => {
      if (request.current !== current) return
      if (initial) { setLoadError(errorText(reason)); setPhase('error') } else setActionError(errorText(reason))
    }).finally(() => { if (request.current === current) setRefreshing(false) })
  }, [client])

  const loadPresets = useCallback((): void => {
    const current = ++presetRequest.current
    setPresetsLoading(true)
    setPresetsError(null)
    client.agentPresets().then(rows => {
      if (presetRequest.current === current) { setPresets(rows); setPresetsLoading(false) }
    }, reason => {
      if (presetRequest.current === current) { setPresetsError(errorText(reason)); setPresetsLoading(false) }
    })
  }, [client])

  useEffect(() => {
    load(true); loadPresets()
    return () => { request.current += 1; presetRequest.current += 1 }
  }, [load, loadPresets])

  const selected = profiles.find(profile => profile.actor.id === selectedId)
  const normalizedQuery = query.trim().toLocaleLowerCase()
  const filteredProfiles = normalizedQuery === '' ? profiles : profiles.filter(profile => (
    profile.actor.displayName.toLocaleLowerCase().includes(normalizedQuery)
    || profile.actor.handle.toLocaleLowerCase().includes(normalizedQuery)
    || profile.charter.summary.toLocaleLowerCase().includes(normalizedQuery)
  ))
  const visibleSelected = filteredProfiles.some(profile => profile.actor.id === selectedId) ? selected : undefined
  const updateProfile = (updated: AgentProfile): void => {
    setProfiles(rows => rows.map(row => row.actor.id === updated.actor.id ? updated : row))
  }
  const created = (result: CreatedAgent): void => {
    setProfiles(rows => [...rows.filter(row => row.actor.id !== result.profile.actor.id), result.profile])
    setSelectedId(result.profile.actor.id)
    setActionError(result.setupError === undefined ? null : t('agents.setupFailed', { error: result.setupError }))
  }
  const confirmDelete = (): void => {
    if (deleteProfile === null || deleting) return
    setDeleting(true)
    client.deleteAgent(deleteProfile.actor.id).then(() => {
      setProfiles(rows => rows.filter(row => row.actor.id !== deleteProfile.actor.id))
      setSelectedId(undefined)
      setDeleteProfile(null)
      setDeleteAcknowledged(false)
    }, reason => { setActionError(errorText(reason)) }).finally(() => { setDeleting(false) })
  }

  return (
    <section className={css.page} aria-label={t('agents.title')} aria-busy={phase === 'loading' || refreshing}>
      <header className={css.headerRow}>
        <div className={css.titleBlock}><h1>{t('agents.title')}</h1><p>{t('agents.subtitle')}</p></div>
        <Button variant="outline" size="sm" icon={<IconPlusOutline16 size={16} />} onClick={() => { setCreateOpen(true); setActionError(null) }}>{t('agents.create')}</Button>
      </header>
      {actionError !== null && <p className={css.banner} role="alert">{actionError}</p>}
      {refreshing && <p className={css.refreshing} role="status">{t('agents.refreshing')}</p>}
      {phase === 'loading' && <div className={css.skeleton} role="status" aria-label={t('agents.loadingAria')}>{SKELETON_ROWS.map(index => <div key={index} className={streamCss.skeletonRow} />)}</div>}
      {phase === 'error' && <div className={css.empty} role="alert"><p className={css.emptyText}>{t('agents.loadFailed', { error: loadError ?? '' })}</p><Button variant="outline" size="sm" onClick={() => { load(true) }}>{t('agents.retry')}</Button></div>}
      {phase === 'ready' && profiles.length === 0 && <div className={css.empty}><p className={css.emptyText}>{t('agents.empty')}</p><Button variant="outline" size="sm" onClick={() => { setCreateOpen(true) }}>{t('agents.create')}</Button></div>}
      {phase === 'ready' && profiles.length > 0 && <div className={css.workspace}>
        <AgentList profiles={filteredProfiles} total={profiles.length} query={query} selectedId={visibleSelected?.actor.id}
          onQueryChange={setQuery}
          onSelect={profile => { setSelectedId(current => current === profile.actor.id ? undefined : profile.actor.id) }}
          expandedContent={visibleSelected === undefined ? undefined : <AgentDetail connection={connection} profile={visibleSelected} presets={presets}
            onUpdated={updateProfile}
            onWorkspace={() => { setWorkspaceProfile(visibleSelected) }}
            onDelete={() => { setDeleteProfile(visibleSelected); setDeleteAcknowledged(false) }} t={t} />} t={t} />
      </div>}
      {createOpen && <AgentCreateDialog connection={connection} presets={presets} presetsLoading={presetsLoading}
        presetsError={presetsError} onPresetsRetry={loadPresets} onClose={() => { setCreateOpen(false) }} onCreated={created} t={t} />}
      {workspaceProfile !== null && <AgentWorkspaceDialog connection={connection} profile={workspaceProfile} onClose={() => { setWorkspaceProfile(null) }} t={t} />}
      {deleteProfile !== null && <RiskConfirmation open title={t('agents.deleteTitle', { name: deleteProfile.actor.displayName })}
        description={t('agents.deleteDescription')} acknowledgeLabel={t('agents.deleteAcknowledge')} cancelLabel={t('agents.cancel')}
        confirmLabel={deleting ? t('agents.deleting') : t('agents.deleteConfirm')} acknowledged={deleteAcknowledged}
        disabled={deleting} onAcknowledgedChange={setDeleteAcknowledged} onCancel={() => { if (!deleting) setDeleteProfile(null) }} onConfirm={confirmDelete} />}
    </section>
  )
}
