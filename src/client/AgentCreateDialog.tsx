import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import { Button, Input, Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentPresetSummary, CreatedAgent } from '../agent-settings-types.ts'
import { ChaosClient, type CreateAgentRequest, type LlmModelGroup } from './api.ts'
import type { ChaosTranslate } from './locales.ts'
import css from './AgentCreateDialog.module.css'

export interface AgentCreateDialogProps {
  connection: ConnectionHandle
  presets: AgentPresetSummary[] | null
  presetsLoading: boolean
  presetsError: string | null
  onPresetsRetry(): void
  onClose(): void
  onCreated(result: CreatedAgent): void
  t: ChaosTranslate
}

function generatedHandle(name: string): string {
  const handle = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')
  return (handle === '' ? 'agent' : handle).slice(0, 40)
}

const HANDLE = /^[a-z0-9](?:[a-z0-9-]{0,38}[a-z0-9])?$/

/** One-page identity-first creation; runtime failure still returns the durable Profile. */
export function AgentCreateDialog(props: AgentCreateDialogProps): JSX.Element {
  const { connection, presets, presetsLoading, presetsError, onPresetsRetry, onClose, onCreated, t } = props
  const client = useMemo(() => new ChaosClient(connection), [connection])
  const [displayName, setDisplayName] = useState('')
  const [handle, setHandle] = useState('agent')
  const [handleEdited, setHandleEdited] = useState(false)
  const [description, setDescription] = useState('')
  const [presetId, setPresetId] = useState('')
  const [provider, setProvider] = useState('')
  const [model, setModel] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)
  const [catalog, setCatalog] = useState<{ groups: LlmModelGroup[]; failures: unknown[] } | null>(null)
  const [catalogLoading, setCatalogLoading] = useState(true)
  const [catalogError, setCatalogError] = useState<string | null>(null)
  const catalogRequest = useRef(0)

  useEffect(() => {
    if (!handleEdited) setHandle(generatedHandle(displayName))
  }, [displayName, handleEdited])

  useEffect(() => {
    if (presetId !== '' || presets === null) return
    const fallback = presets.find(item => item.isDefault && item.broken === undefined)
      ?? presets.find(item => item.broken === undefined)
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

  const models = catalog?.groups.find(group => group.id === provider)?.models ?? []
  const canSubmit = displayName.trim() !== ''
    && HANDLE.test(handle)
    && description.trim() !== ''
    && provider !== ''
    && model !== ''
    && presetId !== ''
    && !submitting
    && !catalogLoading
    && catalogError === null

  const submit = (): void => {
    if (!canSubmit) return
    setSubmitting(true)
    setFailure(null)
    const request: CreateAgentRequest = {
      displayName: displayName.trim(),
      handle,
      description: description.trim(),
      provider,
      model,
      presetId,
    }
    client.createAgent(request).then(result => {
      onCreated(result)
      onClose()
    }, (reason: unknown) => {
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
      footer={<>
        <Button variant="outline" disabled={submitting} onClick={onClose}>{t('create.cancel')}</Button>
        <Button variant="primary" disabled={!canSubmit} onClick={submit}>
          {submitting ? t('create.submitting') : t('create.submit')}
        </Button>
      </>}
    >
      <section className={css.section} aria-labelledby="chaos-create-identity">
        <div className={css.sectionHeading}>
          <strong id="chaos-create-identity">{t('create.identity')}</strong>
          <span>{t('create.identityHint')}</span>
        </div>
        <label className={css.field} htmlFor="chaos-agent-create-name">
          <span className={css.labelText}>{t('create.name')}<em className={css.req}>*</em></span>
          <Input id="chaos-agent-create-name" className={css.input as string} value={displayName}
            onChange={event => { setDisplayName(event.target.value); setFailure(null) }} maxLength={64}
            placeholder={t('create.namePlaceholder')} autoComplete="off" autoFocus disabled={submitting} />
        </label>
        <label className={css.field} htmlFor="chaos-agent-create-handle">
          <span className={css.labelText}>{t('create.handle')}<em className={css.req}>*</em></span>
          <div className={css.handleInput}><span aria-hidden="true">@</span><input id="chaos-agent-create-handle"
            value={handle} onChange={event => { setHandleEdited(true); setHandle(event.target.value.toLowerCase()); setFailure(null) }}
            maxLength={40} autoComplete="off" spellCheck={false} disabled={submitting} /></div>
          {!HANDLE.test(handle) && <small className={css.error}>{t('create.handleInvalid')}</small>}
          <small className={css.hint}>{t('create.handleHint')}</small>
        </label>
        <label className={css.field} htmlFor="chaos-agent-create-charter">
          <span className={css.labelText}>{t('create.charter')}<em className={css.req}>*</em></span>
          <textarea id="chaos-agent-create-charter" className={css.textarea} value={description}
            onChange={event => { setDescription(event.target.value); setFailure(null) }} maxLength={800}
            placeholder={t('create.charterPlaceholder')} disabled={submitting} />
          <small className={css.hint}>{t('create.charterHint')}</small>
        </label>
      </section>

      <section className={css.section} aria-labelledby="chaos-create-runtime">
        <div className={css.sectionHeading}>
          <strong id="chaos-create-runtime">{t('create.runtime')}</strong>
          <span>{t('create.runtimeHint')}</span>
        </div>
        {catalogLoading && <p className={css.hint} role="status">{t('create.routeLoading')}</p>}
        {catalogError !== null && <p className={css.error} role="alert">{t('create.routeFailed', { error: catalogError })}{' '}<Button variant="ghost" size="sm" onClick={loadCatalog}>{t('create.routeRetry')}</Button></p>}
        <div className={css.routeGrid}>
          <label className={css.field}>
            <span className={css.labelText}>{t('create.provider')}<em className={css.req}>*</em></span>
            <select className={css.select} value={provider} disabled={submitting || catalogLoading}
              onChange={event => { setProvider(event.target.value); setModel(''); setFailure(null) }}>
              <option value="" disabled>{t('create.providerPlaceholder')}</option>
              {catalog?.groups.map(group => <option key={group.id} value={group.id}>{group.name}</option>)}
            </select>
          </label>
          <label className={css.field}>
            <span className={css.labelText}>{t('create.model')}<em className={css.req}>*</em></span>
            <select className={css.select} value={model} disabled={submitting || provider === '' || models.length === 0}
              onChange={event => { setModel(event.target.value); setFailure(null) }}>
              <option value="" disabled>{models.length === 0 ? t('create.modelNone') : t('create.modelPick')}</option>
              {models.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
            </select>
          </label>
        </div>
        <label className={css.field}>
          <span className={css.labelText}>{t('create.preset')}<em className={css.req}>*</em></span>
          {presetsLoading && <p className={css.hint} role="status">{t('create.presetLoading')}</p>}
          {presetsError !== null && <p className={css.error} role="alert">{t('create.presetFailed', { error: presetsError })}{' '}<Button variant="ghost" size="sm" onClick={onPresetsRetry}>{t('create.presetRetry')}</Button></p>}
          <select className={css.select} value={presetId} disabled={submitting || presetsLoading}
            onChange={event => { setPresetId(event.target.value); setFailure(null) }}>
            <option value="" disabled>{t('create.presetPlaceholder')}</option>
            {presets?.map(item => <option key={item.id} value={item.id} disabled={item.broken !== undefined}>
              {item.name?.trim() || item.id}{item.broken === undefined ? '' : ` — ${item.broken}`}
            </option>)}
          </select>
        </label>
      </section>
      {failure !== null && <p className={css.error} role="alert">{t('create.failed', { error: failure })}</p>}
    </Modal>
  )
}
