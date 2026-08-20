import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import { Button, IconPlusOutline16, IconTrashOutline16, RiskConfirmation } from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentPresetSummary } from '../agent-settings-types.ts'
import type { NativeActor, NativeRuntimeBinding } from '../native.ts'
import { ChaosClient } from './api.ts'
import { AgentCreateDialog } from './AgentCreateDialog.tsx'
import { avatarSeed } from './avatar.ts'
import type { ChaosTranslate } from './locales.ts'
import css from './AgentSettingsCard.module.css'
import streamCss from './blocks/MessageStream.module.css'

export interface AgentSettingsCardProps {
  connection: ConnectionHandle
  t: ChaosTranslate
}

interface AgentRow {
  actor: NativeActor
  binding?: NativeRuntimeBinding | undefined
}

type Phase = 'loading' | 'ready' | 'error'

const SKELETON_ROWS = [0, 1, 2]

function errorText(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason)
}

/** Dedicated Agents page contributed directly to DSH's settings navigation. */
export function AgentSettingsCard({ connection, t }: AgentSettingsCardProps): JSX.Element {
  const client = useMemo(() => new ChaosClient(connection), [connection])
  const listRequest = useRef(0)
  const presetRequest = useRef(0)
  const [phase, setPhase] = useState<Phase>('loading')
  const [rows, setRows] = useState<AgentRow[]>([])
  const [loadError, setLoadError] = useState<string | null>(null)
  const [refreshing, setRefreshing] = useState(false)
  const [presets, setPresets] = useState<AgentPresetSummary[] | null>(null)
  const [presetsLoading, setPresetsLoading] = useState(true)
  const [presetsError, setPresetsError] = useState<string | null>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<AgentRow | null>(null)
  const [deleteAcknowledged, setDeleteAcknowledged] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [actionError, setActionError] = useState<string | null>(null)

  // Full reread after every mutation; no fine-grained incremental patching.
  const load = useCallback((initial: boolean): void => {
    const request = ++listRequest.current
    if (initial) setPhase('loading')
    else setRefreshing(true)
    void (async (): Promise<void> => {
      try {
        // Identifies the viewer for later surfaces; this page ignores the value.
        await client.snapshot()
        const [actors, bindings] = await Promise.all([client.actors(), client.runtimeBindings()])
        if (listRequest.current !== request) return
        setRows(
          actors
            .filter(actor => actor.kind === 'agent')
            .map(actor => ({ actor, binding: bindings.find(binding => binding.agentId === actor.id) })),
        )
        setPhase('ready')
      } catch (reason) {
        if (listRequest.current !== request) return
        if (initial) {
          setLoadError(errorText(reason))
          setPhase('error')
        } else {
          setActionError(errorText(reason))
        }
      } finally {
        if (listRequest.current === request) setRefreshing(false)
      }
    })()
  }, [client])

  const loadPresets = useCallback((): void => {
    const request = ++presetRequest.current
    setPresetsLoading(true)
    setPresetsError(null)
    client.agentPresets().then(items => {
      if (presetRequest.current !== request) return
      setPresets(items)
      setPresetsLoading(false)
    }, (reason: unknown) => {
      if (presetRequest.current !== request) return
      setPresetsError(errorText(reason))
      setPresetsLoading(false)
    })
  }, [client])

  useEffect(() => {
    load(true)
    loadPresets()
    return () => {
      listRequest.current += 1
      presetRequest.current += 1
    }
  }, [load, loadPresets])

  const confirmDelete = (): void => {
    if (deleteTarget === null || deleting) return
    setDeleting(true)
    setActionError(null)
    client.deleteAgent(deleteTarget.actor.id).then(() => {
      setDeleteTarget(null)
      setDeleteAcknowledged(false)
      load(false)
    }, (reason: unknown) => {
      setDeleteTarget(null)
      setDeleteAcknowledged(false)
      setActionError(errorText(reason))
    }).finally(() => {
      setDeleting(false)
    })
  }

  const routeParts = (row: AgentRow): { preset: string; route: string } | null => {
    const binding = row.binding
    if (binding === undefined) return null
    const named = presets?.find(preset => preset.id === binding.preset)?.name?.trim()
    return {
      preset: named === undefined || named === '' ? binding.preset : named,
      // 'default' is the host sentinel for "follow the host default model".
      route: binding.provider === 'default' && binding.model === 'default'
        ? t('agents.routeDefault')
        : `${binding.provider}/${binding.model}`,
    }
  }

  const openCreateDialog = (): void => {
    setActionError(null)
    setDialogOpen(true)
  }

  return (
    <section className={css.page} aria-label={t('agents.title')} aria-busy={phase === 'loading' || refreshing}>
      <header className={css.headerRow}>
        <div className={css.titleBlock}>
          <h1>{t('agents.title')}</h1>
        </div>
        <Button variant="outline" size="sm" icon={<IconPlusOutline16 size={16} />} aria-label={t('agents.create')} onClick={openCreateDialog} />
      </header>

      {actionError !== null && <p className={css.banner} role="alert">{t('agents.actionFailed', { error: actionError })}</p>}
      {refreshing && <p className={css.refreshing} role="status">{t('agents.refreshing')}</p>}

      {phase === 'loading' && (
        <div className={css.skeleton} role="status" aria-label={t('agents.loadingAria')}>
          {SKELETON_ROWS.map(index => <div key={index} className={streamCss.skeletonRow} />)}
        </div>
      )}

      {phase === 'error' && (
        <div className={css.empty} role="alert">
          <p className={css.emptyText}>{t('agents.loadFailed', { error: loadError ?? '' })}</p>
          <Button variant="outline" size="sm" onClick={() => load(true)}>{t('agents.retry')}</Button>
        </div>
      )}

      {phase === 'ready' && rows.length > 0 && (
        <div className={css.list} role="list" aria-label={t('agents.title')}>
          <div className={css.headRow} aria-hidden="true">
            <span>{t('agents.col.name')}</span>
            <span>{t('agents.col.route')}</span>
            <span />
          </div>
          {rows.map(row => {
            const seed = avatarSeed(row.actor.handle, row.actor.displayName)
            const route = routeParts(row)
            return (
              <div key={row.actor.id} className={css.row} role="listitem">
                <div className={css.cell}>
                  <span className={css.avatar} style={{ background: seed.background }} aria-hidden="true">{seed.initial}</span>
                  <span className={css.nameStack}>
                    <span className={css.name}>{row.actor.displayName}</span>
                    <span className={css.handle}>@{row.actor.handle}</span>
                  </span>
                </div>
                <div className={css.cell}>
                  {route === null
                    ? <span className={css.meta}>—</span>
                    : <>
                        <span className={css.presetChip}>{route.preset}</span>
                        <span className={css.mono}>{route.route}</span>
                      </>}
                </div>
                <div className={css.actionsCell}>
                  <button
                    type="button"
                    className={`${css.iconButton ?? ''} ${css.iconDanger ?? ''}`}
                    aria-label={t('agents.deleteAria', { name: row.actor.displayName })}
                    onClick={() => { setDeleteTarget(row); setDeleteAcknowledged(false); setActionError(null) }}
                  >
                    <IconTrashOutline16 size={14} />
                  </button>
                </div>
              </div>
            )
          })}
        </div>
      )}

      <p className={css.note}>{t('agents.note')}</p>

      {dialogOpen && (
        <AgentCreateDialog
          connection={connection}
          presets={presets}
          presetsLoading={presetsLoading}
          presetsError={presetsError}
          onPresetsRetry={loadPresets}
          onClose={() => { setDialogOpen(false) }}
          onCreated={() => { load(false) }}
          t={t}
        />
      )}

      {deleteTarget !== null && (
        <RiskConfirmation
          open
          title={t('agents.deleteTitle', { name: deleteTarget.actor.displayName })}
          description={t('agents.deleteDescription')}
          acknowledgeLabel={t('agents.deleteAcknowledge')}
          cancelLabel={t('agents.cancel')}
          confirmLabel={deleting ? t('agents.deleting') : t('agents.deleteConfirm')}
          acknowledged={deleteAcknowledged}
          disabled={deleting}
          onAcknowledgedChange={setDeleteAcknowledged}
          onCancel={() => { if (!deleting) setDeleteTarget(null) }}
          onConfirm={confirmDelete}
        />
      )}
    </section>
  )
}
