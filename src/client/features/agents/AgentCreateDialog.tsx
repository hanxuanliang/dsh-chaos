import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import { Button, Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentPresetSummary, CreatedAgent } from '../../../agent-settings-types.ts'
import { ChaosClient, type CreateAgentRequest, type LlmModelGroup } from '../../data/api.ts'
import type { ChaosTranslate } from '../../locales.ts'
import { ErrorBanner, Field, TextInput } from '../../shared/ui/index.ts'
import { generatedAgentHandle, isValidAgentHandle } from './agent-handle.ts'
import css from './AgentCreateDialog.module.css'

export interface AgentCreateDialogProps {
  connection: ConnectionHandle
  presets: AgentPresetSummary[] | null
  presetsLoading: boolean
  presetsError: string | null
  existingHandles: readonly string[]
  onPresetsRetry(): void
  onClose(): void
  onCreated(result: CreatedAgent): void
  t: ChaosTranslate
}

/** One-page identity-first creation; runtime failure still returns the durable Profile. */
export function AgentCreateDialog(props: AgentCreateDialogProps): JSX.Element {
  const { connection, presets, presetsLoading, presetsError, existingHandles, onPresetsRetry, onClose, onCreated, t } = props
  const client = useMemo(() => new ChaosClient(connection), [connection])
  const [displayName, setDisplayName] = useState('')
  const [customHandle, setCustomHandle] = useState<string | null>(null)
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
  const suggestedHandle = useMemo(
    () => generatedAgentHandle(displayName, existingHandles),
    [displayName, existingHandles],
  )
  const handle = customHandle ?? suggestedHandle
  const normalizedHandle = handle.trim().toLowerCase()
  const handleExists = existingHandles.some(existing => existing.toLowerCase() === normalizedHandle)
  const handleError = !isValidAgentHandle(normalizedHandle)
    ? t('create.handleInvalid')
    : handleExists
      ? t('create.handleExists')
      : undefined
  const canSubmit = displayName.trim() !== ''
    && handleError === undefined
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
      handle: normalizedHandle,
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
        <Field label={t('create.name')} required>
          <TextInput id="chaos-agent-create-name" value={displayName}
            onChange={event => { setDisplayName(event.target.value); setFailure(null) }} maxLength={64}
            placeholder={t('create.namePlaceholder')} autoComplete="off" autoFocus disabled={submitting} />
        </Field>
        <Field label={t('create.handle')} required hint={t('create.handleHint')} error={handleError}>
          <TextInput id="chaos-agent-create-handle" value={handle}
            onChange={event => { setCustomHandle(event.target.value.toLowerCase()); setFailure(null) }} maxLength={40}
            placeholder={t('create.handlePlaceholder')} autoComplete="off" disabled={submitting} />
        </Field>
        <Field label={t('create.charter')} required hint={t('create.charterHint')}>
          <textarea id="chaos-agent-create-charter" className={css.textarea} value={description}
            onChange={event => { setDescription(event.target.value); setFailure(null) }} maxLength={800}
            placeholder={t('create.charterPlaceholder')} disabled={submitting} />
        </Field>
      </section>

      <section className={css.section} aria-labelledby="chaos-create-runtime">
        <div className={css.sectionHeading}>
          <strong id="chaos-create-runtime">{t('create.runtime')}</strong>
          <span>{t('create.runtimeHint')}</span>
        </div>
        {catalogLoading && <p className={css.hint} role="status">{t('create.routeLoading')}</p>}
        {catalogError !== null && <ErrorBanner action={<Button variant="ghost" size="sm" onClick={loadCatalog}>{t('create.routeRetry')}</Button>}>{t('create.routeFailed', { error: catalogError })}</ErrorBanner>}
        <div className={css.routeGrid}>
          <Field label={t('create.provider')} required>
            <select value={provider} disabled={submitting || catalogLoading}
              onChange={event => { setProvider(event.target.value); setModel(''); setFailure(null) }}>
              <option value="" disabled>{t('create.providerPlaceholder')}</option>
              {catalog?.groups.map(group => <option key={group.id} value={group.id}>{group.name}</option>)}
            </select>
          </Field>
          <Field label={t('create.model')} required>
            <select value={model} disabled={submitting || provider === '' || models.length === 0}
              onChange={event => { setModel(event.target.value); setFailure(null) }}>
              <option value="" disabled>{models.length === 0 ? t('create.modelNone') : t('create.modelPick')}</option>
              {models.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
            </select>
          </Field>
        </div>
        {presetsLoading && <p className={css.hint} role="status">{t('create.presetLoading')}</p>}
        {presetsError !== null && <ErrorBanner action={<Button variant="ghost" size="sm" onClick={onPresetsRetry}>{t('create.presetRetry')}</Button>}>{t('create.presetFailed', { error: presetsError })}</ErrorBanner>}
        <Field label={t('create.preset')} required>
          <select value={presetId} disabled={submitting || presetsLoading}
            onChange={event => { setPresetId(event.target.value); setFailure(null) }}>
            <option value="" disabled>{t('create.presetPlaceholder')}</option>
            {presets?.map(item => <option key={item.id} value={item.id} disabled={item.broken !== undefined}>
              {item.name?.trim() || item.id}{item.broken === undefined ? '' : ` — ${item.broken}`}
            </option>)}
          </select>
        </Field>
      </section>
      {failure !== null && <ErrorBanner>{t('create.failed', { error: failure })}</ErrorBanner>}
    </Modal>
  )
}
