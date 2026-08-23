import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import { Button, Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentPresetSummary, CreatedAgent } from '../../../agent-settings-types.ts'
import { ChaosClient, type CreateAgentRequest, type LlmModelGroup } from '../../data/api.ts'
import type { ChaosTranslate } from '../../locales.ts'
import { PanelHeader, Toolbar } from '../../shared/layout/index.ts'
import { ErrorBanner, Field, Tabs, TextInput } from '../../shared/ui/index.ts'
import { AvatarChip } from '../../shared/ui/AvatarChip.tsx'
import { hasAgentHandle, initialAgentHandle } from './agent-handle.ts'
import css from './AgentCreateDialog.module.css'

interface AgentCreateCommonProps {
  connection: ConnectionHandle
  presets: AgentPresetSummary[] | null
  presetsLoading: boolean
  presetsError: string | null
  existingHandles: readonly string[]
  onPresetsRetry(): void
  onCreated(result: CreatedAgent): void
  t: ChaosTranslate
}

export interface AgentCreateFormProps extends AgentCreateCommonProps {
  variant: 'dialog' | 'inline'
  onCancel(): void
  onSubmittingChange?(submitting: boolean): void
}

export interface AgentCreateDialogProps extends AgentCreateCommonProps {
  onClose(): void
}

type CreateTab = 'identity' | 'runtime'

/** Shared Circle-style, tabbed Agent creation form used inline and in the Channel modal. */
export function AgentCreateForm(props: AgentCreateFormProps): JSX.Element {
  const {
    connection,
    presets,
    presetsLoading,
    presetsError,
    existingHandles,
    onPresetsRetry,
    onCancel,
    onCreated,
    onSubmittingChange,
    t,
    variant,
  } = props
  const client = useMemo(() => new ChaosClient(connection), [connection])
  const [tab, setTab] = useState<CreateTab>('identity')
  const [displayName, setDisplayName] = useState('')
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
  const handle = initialAgentHandle(displayName)
  const handleExists = hasAgentHandle(handle, existingHandles)
  const identityReady = handle !== '' && !handleExists && description.trim() !== ''
  const runtimeReady = provider !== '' && model !== '' && presetId !== '' && !catalogLoading && catalogError === null
  const canSubmit = identityReady && runtimeReady && !submitting

  const submit = (): void => {
    if (!canSubmit) return
    setSubmitting(true)
    onSubmittingChange?.(true)
    setFailure(null)
    const request: CreateAgentRequest = {
      displayName: displayName.trim(),
      description: description.trim(),
      provider,
      model,
      presetId,
    }
    client.createAgent(request).then(onCreated, (reason: unknown) => {
      setFailure(reason instanceof Error ? reason.message : String(reason))
      setSubmitting(false)
      onSubmittingChange?.(false)
    })
  }

  const tabs = <Tabs<CreateTab>
    className={variant === 'dialog' ? css.formTabs : css.inlineTabs}
    value={tab}
    onValueChange={setTab}
    label={t('create.tabs')}
    align={variant === 'dialog' ? 'stretch' : 'lead'}
    items={[
      { id: 'identity', label: t('create.identity'), tabId: 'chaos-create-tab-identity', panelId: 'chaos-create-panel-identity' },
      { id: 'runtime', label: t('create.runtime'), tabId: 'chaos-create-tab-runtime', panelId: 'chaos-create-panel-runtime' },
    ]}
  />
  const panel = tab === 'identity'
    ? (
        <section id="chaos-create-panel-identity" role="tabpanel" aria-labelledby="chaos-create-tab-identity" className={css.panel}>
          <Field label={t('create.name')} required error={handleExists ? t('create.nameExists') : undefined}>
            <TextInput id="chaos-agent-create-name" value={displayName}
              onChange={event => { setDisplayName(event.target.value); setFailure(null) }} maxLength={64}
              placeholder={t('create.namePlaceholder')} autoComplete="off" autoFocus disabled={submitting} />
          </Field>
          <Field label={t('create.charter')} required help={t('create.charterHint')} meta={`${String(description.length)}/800`}>
            <textarea id="chaos-agent-create-charter" className={css.textarea} value={description}
              onChange={event => { setDescription(event.target.value); setFailure(null) }} maxLength={800}
              placeholder={t('create.charterPlaceholder')} disabled={submitting} />
          </Field>
        </section>
      )
    : (
        <section id="chaos-create-panel-runtime" role="tabpanel" aria-labelledby="chaos-create-tab-runtime" className={css.panel}>
          {catalogLoading && <p className={css.hint} role="status">{t('create.routeLoading')}</p>}
          {catalogError !== null && <ErrorBanner action={<Button variant="ghost" size="sm" onClick={loadCatalog}>{t('create.routeRetry')}</Button>}>{t('create.routeFailed', { error: catalogError })}</ErrorBanner>}
          <Field label={t('create.provider')} required>
            <select value={provider} data-placeholder={provider === '' ? 'true' : undefined} disabled={submitting || catalogLoading}
              onChange={event => { setProvider(event.target.value); setModel(''); setFailure(null) }}>
              <option value="" disabled>{t('create.providerPlaceholder')}</option>
              {catalog?.groups.map(group => <option key={group.id} value={group.id}>{group.name}</option>)}
            </select>
          </Field>
          <Field label={t('create.model')} required>
            <select value={model} data-placeholder={model === '' ? 'true' : undefined} disabled={submitting || provider === '' || models.length === 0}
              onChange={event => { setModel(event.target.value); setFailure(null) }}>
              <option value="" disabled>{models.length === 0 ? t('create.modelNone') : t('create.modelPick')}</option>
              {models.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
            </select>
          </Field>
          {presetsLoading && <p className={css.hint} role="status">{t('create.presetLoading')}</p>}
          {presetsError !== null && <ErrorBanner action={<Button variant="ghost" size="sm" onClick={onPresetsRetry}>{t('create.presetRetry')}</Button>}>{t('create.presetFailed', { error: presetsError })}</ErrorBanner>}
          <Field label={t('create.preset')} required>
            <select value={presetId} data-placeholder={presetId === '' ? 'true' : undefined} disabled={submitting || presetsLoading}
              onChange={event => { setPresetId(event.target.value); setFailure(null) }}>
              <option value="" disabled>{t('create.presetPlaceholder')}</option>
              {presets?.map(item => <option key={item.id} value={item.id} disabled={item.broken !== undefined}>
                {item.name?.trim() || item.id}{item.broken === undefined ? '' : ` - ${item.broken}`}
              </option>)}
            </select>
          </Field>
        </section>
      )
  const actions = (
      <footer className={css.actions}>
        <Button variant="outline" disabled={submitting} onClick={onCancel}>{t('create.cancel')}</Button>
        {tab === 'identity'
          ? <Button variant="primary" disabled={!identityReady || submitting} onClick={() => { setTab('runtime') }}>{t('create.next')}</Button>
          : <Button variant="primary" disabled={!canSubmit} onClick={submit}>{submitting ? t('create.submitting') : t('create.submit')}</Button>}
      </footer>
  )
  const content = <>{panel}{failure !== null && <ErrorBanner>{t('create.failed', { error: failure })}</ErrorBanner>}{actions}</>

  if (variant === 'inline') {
    return (
      <div className={css.form} data-variant={variant}>
        <PanelHeader
          className={css.inlineHeader}
          title={displayName.trim() || t('create.title')}
          leading={<AvatarChip kind="agent" size="xl" handle={handle || 'new-agent'} displayName={displayName.trim() || t('create.title')} />}
          backLabel={t('create.cancel')}
          onBack={onCancel}
        />
        <Toolbar className={css.inlineToolbar} start={tabs} label={t('create.tabs')} />
        <div className={css.inlineSections}>{content}</div>
      </div>
    )
  }

  return (
    <div className={css.form} data-variant={variant}>
      {tabs}
      {content}
    </div>
  )
}

/** Channel flow wrapper: the same tabbed form in a single host Modal. */
export function AgentCreateDialog(props: AgentCreateDialogProps): JSX.Element {
  const { onClose, onCreated, t, ...formProps } = props
  const [submitting, setSubmitting] = useState(false)
  return (
    <Modal
      open
      onClose={() => { if (!submitting) onClose() }}
      title={t('create.title')}
      closeLabel={t('create.close')}
      contentClassName={css.dialogBody as string}
    >
      <AgentCreateForm
        {...formProps}
        t={t}
        variant="dialog"
        onCancel={onClose}
        onSubmittingChange={setSubmitting}
        onCreated={(result) => { onCreated(result); onClose() }}
      />
    </Modal>
  )
}
