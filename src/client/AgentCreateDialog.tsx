import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import { Button, Input, Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentPresetSummary } from '../agent-settings-types.ts'
import { ChaosClient, type CreateAgentRequest, type LlmModelGroup } from './api.ts'
import type { ChaosTranslate } from './locales.ts'
import css from './AgentCreateDialog.module.css'

export interface AgentCreateDialogProps {
  connection: ConnectionHandle
  presets: AgentPresetSummary[] | null
  presetsLoading: boolean
  presetsError: string | null
  onPresetsRetry: () => void
  onClose: () => void
  onCreated: () => void
  t: ChaosTranslate
}

/**
 * One-shot create form. The parent remounts the dialog per open, so stale
 * form state never survives; an RPC failure keeps the filled form in place.
 */
export function AgentCreateDialog({ connection, presets, presetsLoading, presetsError, onPresetsRetry, onClose, onCreated, t }: AgentCreateDialogProps): JSX.Element {
  const client = useMemo(() => new ChaosClient(connection), [connection])
  const [name, setName] = useState('')
  const [presetId, setPresetId] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)
  // '' = follow the host default model (host writes the 'default' sentinel).
  const [provider, setProvider] = useState('')
  const [modelId, setModelId] = useState('')
  const [catalog, setCatalog] = useState<{ groups: LlmModelGroup[]; failures: unknown[] } | null>(null)
  const [catalogLoading, setCatalogLoading] = useState(true)
  const [catalogError, setCatalogError] = useState<string | null>(null)
  const catalogRequest = useRef(0)

  // Default to the host default preset (first healthy preset as fallback).
  useEffect(() => {
    if (presetId !== '' || presets === null) return
    const fallback = presets.find(preset => preset.isDefault && preset.broken === undefined)
      ?? presets.find(preset => preset.broken === undefined)
    if (fallback !== undefined) setPresetId(fallback.id)
  }, [presets, presetId])

  const loadCatalog = useCallback((): void => {
    const request = ++catalogRequest.current
    setCatalogLoading(true)
    setCatalogError(null)
    client.modelCatalog().then(result => {
      if (catalogRequest.current !== request) return
      setCatalog(result)
      setCatalogLoading(false)
    }, (reason: unknown) => {
      if (catalogRequest.current !== request) return
      setCatalogError(reason instanceof Error ? reason.message : String(reason))
      setCatalogLoading(false)
    })
  }, [client])

  useEffect(() => {
    loadCatalog()
    return () => { catalogRequest.current += 1 }
  }, [loadCatalog])

  const trimmedName = name.trim()
  const selected = presets?.find(preset => preset.id === presetId)
  const selectedDescription = selected?.description?.trim() ?? ''
  const providerModels = catalog?.groups.find(group => group.id === provider)?.models ?? []
  // Host RPC rule (src/index.ts createNamedAgent): provider and model must be given together.
  const canSubmit = trimmedName !== '' && presetId !== '' && (provider === '' || modelId !== '') && !submitting

  const submit = (): void => {
    if (!canSubmit) return
    setSubmitting(true)
    setFailure(null)
    const request: CreateAgentRequest = provider === ''
      ? { name: trimmedName, presetId }
      : { name: trimmedName, presetId, provider, model: modelId }
    client.createAgent(request).then(() => {
      onCreated()
      onClose()
    }, (reason: unknown) => {
      // Keep the filled form; the inline error explains the next step.
      setFailure(reason instanceof Error ? reason.message : String(reason))
      setSubmitting(false)
    })
  }

  return (
    <Modal
      open
      onClose={() => { if (!submitting) onClose() }}
      title={t('create.title')}
      closeLabel={t('create.close')}
      contentClassName={css.body as string}
      footer={(
        <>
          <Button variant="outline" disabled={submitting} onClick={onClose}>{t('create.cancel')}</Button>
          <Button variant="primary" disabled={!canSubmit} onClick={submit}>
            {submitting ? t('create.submitting') : t('create.submit')}
          </Button>
        </>
      )}
    >
      <label className={css.field} htmlFor="chaos-agent-create-name">
        <span className={css.labelText}>{t('create.name')}<em className={css.req} aria-hidden="true">*</em></span>
        <Input
          id="chaos-agent-create-name"
          className={css.input as string}
          value={name}
          onChange={event => { setName(event.target.value); setFailure(null) }}
          maxLength={64}
          placeholder={t('create.namePlaceholder')}
          autoComplete="off"
          autoFocus
          spellCheck={false}
          disabled={submitting}
        />
        <small className={css.hint}>{t('create.nameHint')}</small>
      </label>

      <div className={css.field}>
        <span className={css.labelText}>{t('create.preset')}<em className={css.req} aria-hidden="true">*</em></span>
        {presetsLoading && <p className={css.hint} role="status">{t('create.presetLoading')}</p>}
        {presetsError !== null && (
          <p className={css.hint} role="alert">
            {t('create.presetFailed', { error: presetsError })}{' '}
            <Button variant="ghost" size="sm" onClick={onPresetsRetry}>{t('create.presetRetry')}</Button>
          </p>
        )}
        {presets !== null && presets.length === 0 && <p className={css.hint}>{t('create.presetEmpty')}</p>}
        {presets !== null && presets.length > 0 && (
          <select
            className={css.select}
            aria-label={t('create.preset')}
            value={presetId}
            disabled={submitting}
            onChange={event => { setPresetId(event.target.value); setFailure(null) }}
          >
            {presetId === '' && <option value="" disabled>{t('create.presetPlaceholder')}</option>}
            {presets.map(preset => {
              const label = preset.name?.trim() === undefined || preset.name.trim() === '' ? preset.id : preset.name
              return (
                <option key={preset.id} value={preset.id} disabled={preset.broken !== undefined}>
                  {preset.broken === undefined ? label : t('create.presetBroken', { label, reason: preset.broken })}
                </option>
              )
            })}
          </select>
        )}
        {selectedDescription !== '' && <small className={css.hint}>{selectedDescription}</small>}
      </div>

      <div className={css.field}>
        <span className={css.labelText}>{t('create.route')}</span>
        {catalogLoading && <p className={css.hint} role="status">{t('create.routeLoading')}</p>}
        {catalogError !== null && (
          <p className={css.hint} role="alert">
            {t('create.routeFailed', { error: catalogError })}{' '}
            <Button variant="ghost" size="sm" onClick={loadCatalog}>{t('create.routeRetry')}</Button>
          </p>
        )}
        {catalog !== null && (
          <>
            <select
              className={css.select}
              aria-label={t('create.route')}
              value={provider}
              disabled={submitting}
              onChange={event => { setProvider(event.target.value); setModelId(''); setFailure(null) }}
            >
              <option value="">{t('create.providerPlaceholder')}</option>
              {catalog.groups.map(group => <option key={group.id} value={group.id}>{group.name}</option>)}
            </select>
            {provider !== '' && (
              <select
                className={css.select}
                aria-label={t('create.route')}
                value={modelId}
                disabled={submitting || providerModels.length === 0}
                onChange={event => { setModelId(event.target.value); setFailure(null) }}
              >
                {modelId === '' && <option value="" disabled>{providerModels.length === 0 ? t('create.modelNone') : t('create.modelPick')}</option>}
                {providerModels.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
              </select>
            )}
          </>
        )}
        <small className={css.hint}>{t('create.routeHint')}</small>
      </div>

      {failure !== null && <p className={css.error} role="alert">{t('create.failed', { error: failure })}</p>}
    </Modal>
  )
}
