import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import {
  Button,
  IconEllipsisOutline16,
  IconChevronRightOutline14,
  IconFolderOpenOutline16,
  IconTrashOutline16,
  Menu,
} from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentMembership, AgentPresetSummary, AgentProfile } from '../../../agent-settings-types.ts'
import { ChaosClient, type LlmModelGroup } from '../../data/api.ts'
import type { ChaosTranslate } from '../../locales.ts'
import { PanelHeader, Toolbar } from '../../shared/layout/index.ts'
import { ErrorBanner, Field, IconButton, StatusChip, Tabs } from '../../shared/ui/index.ts'
import { AgentAvatarEditor } from './AgentAvatarEditor.tsx'
import css from './AgentDetail.module.css'

type Tab = 'identity' | 'runtime' | 'collaboration'

function reasonText(reason: unknown): string { return reason instanceof Error ? reason.message : String(reason) }

export function AgentDetail({ connection, profile, presets, onBack, onUpdated, onWorkspace, onNavigateChannel, onDelete, t }: {
  connection: ConnectionHandle
  profile: AgentProfile
  presets: AgentPresetSummary[]
  onBack?: (() => void) | undefined
  onUpdated(profile: AgentProfile): void
  onWorkspace(): void
  onNavigateChannel(targetId: string): void
  onDelete(): void
  t: ChaosTranslate
}): JSX.Element {
  const client = useMemo(() => new ChaosClient(connection), [connection])
  const [tab, setTab] = useState<Tab>('identity')
  const [menuOpen, setMenuOpen] = useState(false)
  const [name, setName] = useState(profile.actor.displayName)
  const [description, setDescription] = useState(profile.charter.summary)
  const [identitySaving, setIdentitySaving] = useState(false)
  const [identityError, setIdentityError] = useState<string | null>(null)
  const [avatarError, setAvatarError] = useState<string | null>(null)
  const [provider, setProvider] = useState(profile.binding?.provider ?? '')
  const [model, setModel] = useState(profile.binding?.model ?? '')
  const [presetId, setPresetId] = useState(profile.binding?.preset ?? presets.find(item => item.isDefault)?.id ?? '')
  const [runtimeSaving, setRuntimeSaving] = useState(false)
  const [runtimeError, setRuntimeError] = useState<string | null>(null)
  const [catalog, setCatalog] = useState<{ groups: LlmModelGroup[]; failures: unknown[] } | null>(null)
  const [catalogLoading, setCatalogLoading] = useState(false)
  const [catalogError, setCatalogError] = useState<string | null>(null)
  const [memberships, setMemberships] = useState<AgentMembership[] | null>(null)
  const [membershipsLoading, setMembershipsLoading] = useState(false)
  const [membershipsError, setMembershipsError] = useState<string | null>(null)
  const catalogRequest = useRef(0)
  const membershipsRequest = useRef(0)
  const previousProfile = useRef(profile)

  useEffect(() => {
    const previous = previousProfile.current
    const changedAgent = previous.actor.id !== profile.actor.id
    setName(current => changedAgent || current === previous.actor.displayName ? profile.actor.displayName : current)
    setDescription(current => changedAgent || current === previous.charter.summary ? profile.charter.summary : current)
    setProvider(current => changedAgent || current === (previous.binding?.provider ?? '') ? (profile.binding?.provider ?? '') : current)
    setModel(current => changedAgent || current === (previous.binding?.model ?? '') ? (profile.binding?.model ?? '') : current)
    setPresetId(current => changedAgent || current === (previous.binding?.preset ?? '')
      ? (profile.binding?.preset ?? presets.find(item => item.isDefault)?.id ?? '')
      : current)
    if (changedAgent) {
      setIdentityError(null)
      setAvatarError(null)
      setRuntimeError(null)
      membershipsRequest.current += 1
      setMemberships(null)
      setMembershipsLoading(false)
      setMembershipsError(null)
    }
    previousProfile.current = profile
  }, [profile, presets])

  const loadCatalog = useCallback((): void => {
    const current = ++catalogRequest.current
    setCatalogLoading(true)
    setCatalogError(null)
    client.modelCatalog().then(value => {
      if (catalogRequest.current === current) setCatalog(value)
    }, reason => {
      if (catalogRequest.current === current) setCatalogError(reasonText(reason))
    }).finally(() => { if (catalogRequest.current === current) setCatalogLoading(false) })
  }, [client])

  useEffect(() => {
    if (tab === 'runtime' && catalog === null && catalogError === null && !catalogLoading) loadCatalog()
    if (tab === 'collaboration' && memberships === null && membershipsError === null && !membershipsLoading) {
      const current = ++membershipsRequest.current
      setMembershipsLoading(true)
      client.agentMemberships(profile.actor.id).then(value => {
        if (membershipsRequest.current === current) setMemberships(value)
      }, reason => {
        if (membershipsRequest.current === current) setMembershipsError(reasonText(reason))
      }).finally(() => { if (membershipsRequest.current === current) setMembershipsLoading(false) })
    }
  }, [catalog, catalogError, catalogLoading, client, loadCatalog, memberships, membershipsError, membershipsLoading, profile.actor.id, tab])

  useEffect(() => () => {
    catalogRequest.current += 1
    membershipsRequest.current += 1
  }, [])

  const identityDirty = name.trim() !== profile.actor.displayName || description.trim() !== profile.charter.summary
  const runtimeDirty = provider !== (profile.binding?.provider ?? '')
    || model !== (profile.binding?.model ?? '')
    || presetId !== (profile.binding?.preset ?? '')
  const models = catalog?.groups.find(group => group.id === provider)?.models ?? []

  const saveIdentity = (): void => {
    if (!identityDirty || identitySaving || name.trim() === '' || description.trim() === '') return
    setIdentitySaving(true)
    setIdentityError(null)
    client.updateAgentProfile(profile.actor.id, name.trim(), description.trim(), profile.profileVersion).then(onUpdated, reason => {
      setIdentityError(reasonText(reason))
    }).finally(() => { setIdentitySaving(false) })
  }

  const saveRuntime = (): void => {
    if (!runtimeDirty || runtimeSaving || provider === '' || model === '' || presetId === '') return
    setRuntimeSaving(true)
    setRuntimeError(null)
    client.replaceAgentRuntime(profile.actor.id, provider, model, presetId, profile.binding?.generation).then(binding => {
      onUpdated({ ...profile, binding })
    }, reason => {
      setRuntimeError(reasonText(reason))
    }).finally(() => { setRuntimeSaving(false) })
  }

  return (
    <section id={`chaos-agent-${profile.actor.id}-detail`} className={css.detail} aria-label={profile.actor.displayName}>
      <PanelHeader
        className={css.detailHeader}
        title={profile.actor.displayName}
        description={`@${profile.actor.handle}`}
        leading={<AgentAvatarEditor client={client} profile={profile} onUpdated={onUpdated} onError={setAvatarError} t={t} />}
        {...(onBack === undefined ? {} : { backLabel: t('agents.back'), onBack })}
        actions={<>
          <StatusChip tone={profile.binding === undefined ? 'warning' : 'success'} label={profile.binding === undefined ? t('agents.unconfigured') : t('agents.configured')} />
          <IconButton label={t('agents.openWorkspace')} icon={<IconFolderOpenOutline16 size={16} />} onClick={onWorkspace} />
          <Menu open={menuOpen} portal compact dense align="end" onClose={() => { setMenuOpen(false) }}
            onSelect={id => { setMenuOpen(false); if (id === 'delete') onDelete() }}
            items={[{ id: 'delete', label: t('agents.delete'), icon: <IconTrashOutline16 size={16} />, danger: true }]}
            anchor={<IconButton label={t('agents.more')} icon={<IconEllipsisOutline16 size={16} />} selected={menuOpen} aria-haspopup="menu" aria-expanded={menuOpen} onClick={() => { setMenuOpen(value => !value) }} />} />
        </>}
      />
      {avatarError !== null && <div className={css.headerError}><ErrorBanner>{avatarError}</ErrorBanner></div>}
      <Toolbar start={(
        <Tabs<Tab> label={t('agents.detailTabs')} value={tab} onValueChange={setTab} align="lead" items={([
          ['identity', t('agents.identity')],
          ['runtime', t('agents.runtime')],
          ['collaboration', t('agents.collaboration')],
        ] as const).map(([id, label]) => ({
          id,
          label,
          active: tab === id,
          tabId: `chaos-agent-${profile.actor.id}-tab-${id}`,
          panelId: `chaos-agent-${profile.actor.id}-panel-${id}`,
        }))} />
      )} />

      <div className={css.sections}>
        {tab === 'identity' && <section id={`chaos-agent-${profile.actor.id}-panel-identity`} role="tabpanel" className={css.panel} aria-labelledby={`chaos-agent-${profile.actor.id}-tab-identity`}>
          {identityError !== null && <ErrorBanner>{t('agents.identityFailed', { error: identityError })}</ErrorBanner>}
          <Field label={t('agents.name')} required><input value={name} maxLength={64} disabled={identitySaving} onChange={event => { setName(event.target.value); setIdentityError(null) }} /></Field>
          <Field label={t('agents.charter')} required help={t('create.charterHint')} meta={`${String(description.length)}/800`}><textarea value={description} maxLength={800} disabled={identitySaving} onChange={event => { setDescription(event.target.value); setIdentityError(null) }} /></Field>
          <footer className={css.footer}>
            <Button variant="outline" size="sm" disabled={!identityDirty || identitySaving} onClick={() => { setName(profile.actor.displayName); setDescription(profile.charter.summary); setIdentityError(null) }}>{t('agents.discard')}</Button>
            <Button variant="primary" size="sm" disabled={!identityDirty || identitySaving || name.trim() === '' || description.trim() === ''} onClick={saveIdentity}>{identitySaving ? t('agents.saving') : t('agents.saveIdentity')}</Button>
          </footer>
        </section>}

        {tab === 'runtime' && <section id={`chaos-agent-${profile.actor.id}-panel-runtime`} role="tabpanel" className={css.panel} aria-labelledby={`chaos-agent-${profile.actor.id}-tab-runtime`}>
          {runtimeError !== null && <ErrorBanner>{t('agents.runtimeFailed', { error: runtimeError })}</ErrorBanner>}
          {catalogError !== null && <ErrorBanner>{t('create.routeFailed', { error: catalogError })} <button type="button" className={css.inlineAction} onClick={loadCatalog}>{t('create.routeRetry')}</button></ErrorBanner>}
          {catalog === null && catalogError === null && <p className={css.state} role="status">{t('create.routeLoading')}</p>}
          {catalog !== null && <div className={css.runtimeGrid}>
            <Field label={t('create.provider')} required><select value={provider} disabled={runtimeSaving} onChange={event => { setProvider(event.target.value); setModel(''); setRuntimeError(null) }}><option value="" disabled>{t('create.providerPlaceholder')}</option>{catalog.groups.map(group => <option key={group.id} value={group.id}>{group.name}</option>)}</select></Field>
            <Field label={t('create.model')} required><select value={model} disabled={runtimeSaving || provider === ''} onChange={event => { setModel(event.target.value); setRuntimeError(null) }}><option value="" disabled>{models.length === 0 ? t('create.modelNone') : t('create.modelPick')}</option>{models.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select></Field>
            <Field label={t('create.preset')} required><select value={presetId} disabled={runtimeSaving} onChange={event => { setPresetId(event.target.value); setRuntimeError(null) }}>{presets.map(item => <option key={item.id} value={item.id} disabled={item.broken !== undefined}>{item.name?.trim() || item.id}</option>)}</select></Field>
          </div>}
          <p className={css.runtimeWarning}>{t('agents.runtimeWarning')}</p>
          <footer className={css.footer}>
            <Button variant="outline" size="sm" disabled={!runtimeDirty || runtimeSaving} onClick={() => { setProvider(profile.binding?.provider ?? ''); setModel(profile.binding?.model ?? ''); setPresetId(profile.binding?.preset ?? presets.find(item => item.isDefault)?.id ?? ''); setRuntimeError(null) }}>{t('agents.discard')}</Button>
            <Button variant="primary" size="sm" disabled={!runtimeDirty || runtimeSaving || provider === '' || model === '' || presetId === ''} onClick={saveRuntime}>{runtimeSaving ? t('agents.applying') : profile.binding === undefined ? t('agents.configureRuntime') : t('agents.applyRuntime')}</Button>
          </footer>
        </section>}

        {tab === 'collaboration' && <section id={`chaos-agent-${profile.actor.id}-panel-collaboration`} role="tabpanel" className={css.panel} aria-labelledby={`chaos-agent-${profile.actor.id}-tab-collaboration`}>
          {membershipsError !== null && <ErrorBanner>{t('agents.membershipsFailed', { error: membershipsError })}</ErrorBanner>}
          {memberships === null && membershipsError === null && <p className={css.state} role="status">{t('agents.membershipsLoading')}</p>}
          {memberships !== null && memberships.length === 0 && <p className={css.state}>{t('agents.membershipsEmpty')}</p>}
          {memberships !== null && memberships.length > 0 && <div className={css.memberships}>{memberships.map(item => item.target.kind === 'channel'
            ? <button key={item.target.id} type="button" className={css.membership} data-navigable="true" aria-label={t('agents.openChannel', { name: item.target.name })} onClick={() => { onNavigateChannel(item.target.id) }}><span># {item.target.name}</span><span className={css.membershipMeta}><small>{item.role}</small><IconChevronRightOutline14 aria-hidden="true" /></span></button>
            : <div key={item.target.id} className={css.membership}><span>↔ {item.target.name}</span><small>{item.role}</small></div>)}</div>}
        </section>}
      </div>
    </section>
  )
}
