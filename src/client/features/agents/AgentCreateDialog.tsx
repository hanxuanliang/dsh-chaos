import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import { Button, Modal, type MenuEntry } from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentPresetSummary, CreatedAgent } from '../../../agent-settings-types.ts'
import { ChaosClient, type CreateAgentRequest, type LlmModelGroup } from '../../data/api.ts'
import type { ChaosTranslate } from '../../locales.ts'
import { PanelHeader } from '../../shared/layout/index.ts'
import { ErrorBanner, HelpHint } from '../../shared/ui/index.ts'
import { AvatarChip } from '../../shared/ui/AvatarChip.tsx'
import { hasAgentHandle, initialAgentHandle } from './agent-handle.ts'
import { PillSelect } from './PillSelect.tsx'
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

/**
 * Circle-style single-screen creation: a borderless identity block first,
 * one row of pill metadata selectors, then a footer bar carrying the derived
 * handle preview and the primary action. One screen beats two tabs at five
 * fields — everything is visible at once, so no step can hide a gap.
 */
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
  const runtimeReady = provider !== '' && model !== '' && presetId !== ''
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

  const identity = (
    <div className={css.identity}>
      <div className={css.titleRow}>
        <input
          id="chaos-agent-create-name"
          className={css.titleInput}
          value={displayName}
          onChange={event => { setDisplayName(event.target.value); setFailure(null) }}
          maxLength={64}
          placeholder={t('create.namePlaceholder')}
          autoComplete="off"
          autoFocus
          disabled={submitting}
          aria-invalid={handleExists || undefined}
        />
      </div>
      <textarea
        id="chaos-agent-create-charter"
        className={css.textarea}
        value={description}
        onChange={event => { setDescription(event.target.value); setFailure(null) }}
        maxLength={800}
        placeholder={t('create.charterPlaceholder')}
        disabled={submitting}
        aria-label={t('create.charter')}
      />
      <div className={css.identityMeta}>
        {handleExists
          ? <span className={css.handleError} role="alert">{t('create.nameExists')}</span>
          : <>
              <HelpHint label={t('create.charterHint')} />
              <span className={css.count}>{String(description.length)}/800</span>
            </>}
      </div>
    </div>
  )

  const routeFailed = catalogError !== null
  const routeEntries: MenuEntry[] = catalog?.groups.map(group => ({ id: group.id, label: group.name })) ?? []
  const modelEntries: MenuEntry[] = models.map(item => ({ id: item.id, label: item.name }))
  const presetEntries: MenuEntry[] = presets?.map(item => ({
    id: item.id,
    label: item.name?.trim() || item.id,
    disabled: item.broken !== undefined,
  })) ?? []
  const providerGroup = catalog?.groups.find(group => group.id === provider)
  const modelItem = models.find(item => item.id === model)
  const presetItem = presets?.find(item => item.id === presetId)

  const metadata = (
    <div className={css.metadata}>
      {catalogLoading && <span className={css.hint} role="status">{t('create.routeLoading')}</span>}
      {routeFailed && (
        <ErrorBanner action={<Button variant="ghost" size="sm" onClick={loadCatalog}>{t('create.routeRetry')}</Button>}>
          {t('create.routeFailed', { error: catalogError })}
        </ErrorBanner>
      )}
      <div className={css.pillRow}>
        <PillSelect
          label={t('create.provider')}
          placeholder={t('create.providerPlaceholder')}
          value={providerGroup?.name ?? provider}
          entries={routeEntries}
          selectedId={provider}
          disabled={submitting || catalogLoading || routeFailed}
          onSelect={id => { setProvider(id); setModel(''); setFailure(null) }}
        />
        <PillSelect
          label={t('create.model')}
          placeholder={models.length === 0 ? t('create.modelNone') : t('create.modelPick')}
          value={modelItem?.name ?? model}
          entries={modelEntries}
          selectedId={model}
          disabled={submitting || provider === '' || models.length === 0}
          onSelect={id => { setModel(id); setFailure(null) }}
        />
        <PillSelect
          label={t('create.preset')}
          placeholder={t('create.presetPlaceholder')}
          value={presetItem?.name ?? presetId}
          entries={presetEntries}
          selectedId={presetId}
          disabled={submitting || presetsLoading}
          onSelect={id => { setPresetId(id); setFailure(null) }}
        />
      </div>
      {presetsLoading && <span className={css.hint} role="status">{t('create.presetLoading')}</span>}
      {presetsError !== null && (
        <ErrorBanner action={<Button variant="ghost" size="sm" onClick={onPresetsRetry}>{t('create.presetRetry')}</Button>}>
          {t('create.presetFailed', { error: presetsError })}
        </ErrorBanner>
      )}
    </div>
  )

  const actions = (
    <footer className={css.actions}>
      <div className={css.actionsLeading}>
        {handle !== '' && !handleExists && (
          <span className={css.handlePreview} data-handle={handle}>
            {t('create.handlePreview', { handle })}
          </span>
        )}
      </div>
      <div className={css.actionsTrailing}>
        <Button variant="outline" size="sm" disabled={submitting} onClick={onCancel}>{t('create.cancel')}</Button>
        <Button variant="primary" size="sm" disabled={!canSubmit} onClick={submit}>
          {submitting ? t('create.submitting') : t('create.submit')}
        </Button>
      </div>
    </footer>
  )

  const content = (
    <>
      {identity}
      {metadata}
      {failure !== null && <ErrorBanner>{t('create.failed', { error: failure })}</ErrorBanner>}
      {actions}
    </>
  )

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
        <div className={css.inlineSections}>{content}</div>
      </div>
    )
  }

  return (
    <div className={css.form} data-variant={variant}>
      {content}
    </div>
  )
}

/** Channel flow wrapper: the same single-screen form in a host Modal. */
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
