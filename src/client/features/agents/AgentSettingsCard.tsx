import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import { Button, IconPlusOutline16, RiskConfirmation } from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentPresetSummary, AgentProfile, CreatedAgent } from '../../../agent-settings-types.ts'
import { ChaosClient } from '../../data/api.ts'
import { AgentCreateForm } from './AgentCreateDialog.tsx'
import type { ChaosTranslate } from '../../locales.ts'
import { AgentDetail } from './AgentDetail.tsx'
import { AgentList } from './AgentList.tsx'
import { ResponsiveDrilldown, SplitPane } from '../../shared/layout/index.ts'
import { EmptyState, ErrorBanner, IconButton, SkeletonList } from '../../shared/ui/index.ts'
import css from './AgentSettingsCard.module.css'

export interface AgentSettingsCardProps {
  connection: ConnectionHandle
  openPath(path: string): Promise<void>
  navigateChannel(targetId: string): void
  t: ChaosTranslate
}
type Phase = 'loading' | 'ready' | 'error'
function errorText(reason: unknown): string { return reason instanceof Error ? reason.message : String(reason) }

/** Identity-first expandable Agent settings surface. */
export function AgentSettingsCard({ connection, openPath, navigateChannel, t }: AgentSettingsCardProps): JSX.Element {
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
    setCreateOpen(false)
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
  const closeDetail = (): void => {
    const id = selectedId
    setSelectedId(undefined)
    window.requestAnimationFrame(() => {
      const trigger = [...document.querySelectorAll<HTMLButtonElement>('[data-entity-id]')]
        .find(button => button.dataset.entityId === id)
      trigger?.focus()
    })
  }
  const list = <AgentList profiles={filteredProfiles} total={profiles.length} query={query} selectedId={visibleSelected?.actor.id}
    onQueryChange={setQuery}
    onSelect={profile => { setSelectedId(profile.actor.id) }} t={t} />
  const detail = visibleSelected === undefined
    ? <EmptyState title={t('agents.select')} />
    : <AgentDetail connection={connection} profile={visibleSelected} presets={presets}
        onBack={closeDetail}
        onUpdated={updateProfile}
        onWorkspace={() => {
          openPath(visibleSelected.workspacePath).catch(reason => { setActionError(t('agents.workspaceOpenFailed', { error: errorText(reason) })) })
        }}
        onNavigateChannel={navigateChannel}
        onDelete={() => { setDeleteProfile(visibleSelected); setDeleteAcknowledged(false) }} t={t} />
  const desktopDetail = visibleSelected === undefined
    ? detail
    : <AgentDetail connection={connection} profile={visibleSelected} presets={presets}
        onUpdated={updateProfile}
        onWorkspace={() => {
          openPath(visibleSelected.workspacePath).catch(reason => { setActionError(t('agents.workspaceOpenFailed', { error: errorText(reason) })) })
        }}
        onNavigateChannel={navigateChannel}
        onDelete={() => { setDeleteProfile(visibleSelected); setDeleteAcknowledged(false) }} t={t} />

  return (
    <section className={css.page} aria-label={t('agents.title')} aria-busy={phase === 'loading' || refreshing}>
      <header className={css.headerRow}>
        <div className={css.titleBlock}><h1>{t('agents.title')}</h1><p>{t('agents.subtitle')}</p></div>
        <IconButton label={t('agents.create')} icon={<IconPlusOutline16 size={14} />} selected={createOpen}
          disabled={phase !== 'ready'} onClick={() => { setCreateOpen(value => !value); setActionError(null) }} />
      </header>
      {actionError !== null && <ErrorBanner>{actionError}</ErrorBanner>}
      {refreshing && <p className={css.refreshing} role="status">{t('agents.refreshing')}</p>}
      {phase === 'loading' && <SkeletonList className={css.skeleton} rows={4} label={t('agents.loadingAria')} />}
      {phase === 'error' && <EmptyState title={t('agents.loadFailed', { error: loadError ?? '' })} action={<Button variant="outline" size="sm" onClick={() => { load(true) }}>{t('agents.retry')}</Button>} />}
      {phase === 'ready' && createOpen && <section className={css.createBlock} data-agent-create-inline aria-label={t('create.title')}>
          <AgentCreateForm connection={connection} presets={presets} presetsLoading={presetsLoading}
            presetsError={presetsError} existingHandles={profiles.map(profile => profile.actor.handle)}
            onPresetsRetry={loadPresets} onCancel={() => { setCreateOpen(false) }} onCreated={created}
            variant="inline" t={t} />
        </section>}
      {phase === 'ready' && !createOpen && profiles.length > 0 && <div className={css.workspace}>
          <ResponsiveDrilldown
            desktop={<SplitPane id="agent-settings" leading={list} trailing={desktopDetail} leadingDefault={280} leadingMin={240} leadingMax={360} trailingMin={420} separatorLabel={t('agents.resize')} />}
            list={list}
            detail={detail}
            detailOpen={visibleSelected !== undefined}
          />
        </div>}
      {deleteProfile !== null && <RiskConfirmation open title={t('agents.deleteTitle', { name: deleteProfile.actor.displayName })}
        description={t('agents.deleteDescription')} acknowledgeLabel={t('agents.deleteAcknowledge')} cancelLabel={t('agents.cancel')}
        confirmLabel={deleting ? t('agents.deleting') : t('agents.deleteConfirm')} acknowledged={deleteAcknowledged}
        disabled={deleting} onAcknowledgedChange={setDeleteAcknowledged} onCancel={() => { if (!deleting) setDeleteProfile(null) }} onConfirm={confirmDelete} />}
    </section>
  )
}
