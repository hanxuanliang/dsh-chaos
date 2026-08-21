import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import {
  Button,
  IconEllipsisOutline16,
  IconFolderOpenOutline16,
  IconTrashOutline16,
  Menu,
  Tooltip,
} from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentMembership, AgentPresetSummary, AgentProfile } from '../../agent-settings-types.ts'
import { ChaosClient, type LlmModelGroup } from '../api.ts'
import type { ChaosTranslate } from '../locales.ts'
import { ErrorBanner } from '../atoms/ErrorBanner.tsx'
import { PillTabs } from '../atoms/PillTabs.tsx'
import css from './AgentDetail.module.css'

type Tab = 'identity' | 'runtime' | 'collaboration'

function reasonText(reason: unknown): string { return reason instanceof Error ? reason.message : String(reason) }

export function AgentDetail({ connection, profile, presets, onUpdated, onWorkspace, onDelete, t }: {
  connection: ConnectionHandle
  profile: AgentProfile
  presets: AgentPresetSummary[]
  onUpdated(profile: AgentProfile): void
  onWorkspace(): void
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

  useEffect(() => {
    setName(profile.actor.displayName)
    setDescription(profile.charter.summary)
    setProvider(profile.binding?.provider ?? '')
    setModel(profile.binding?.model ?? '')
    setPresetId(profile.binding?.preset ?? presets.find(item => item.isDefault)?.id ?? '')
    setIdentityError(null)
    setRuntimeError(null)
    membershipsRequest.current += 1
    setMemberships(null)
    setMembershipsLoading(false)
    setMembershipsError(null)
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
      <div className={css.toolbar}>
        <PillTabs align="lead" ariaLabel={t('agents.detailTabs')} items={([
          ['identity', t('agents.identity')],
          ['runtime', t('agents.runtime')],
          ['collaboration', t('agents.collaboration')],
        ] as const).map(([id, label]) => ({
          id,
          label,
          active: tab === id,
          tabId: `chaos-agent-${profile.actor.id}-tab-${id}`,
          controls: `chaos-agent-${profile.actor.id}-panel-${id}`,
          onClick: () => { setTab(id) },
        }))} />
        <div className={css.actions}>
          <Tooltip label={t('agents.openWorkspace')} side="bottom">
            <button type="button" className={css.iconButton} aria-label={t('agents.openWorkspace')} onClick={onWorkspace}><IconFolderOpenOutline16 size={16} /></button>
          </Tooltip>
          <Menu open={menuOpen} portal compact dense align="end" onClose={() => { setMenuOpen(false) }}
            onSelect={id => { setMenuOpen(false); if (id === 'delete') onDelete() }}
            items={[{ id: 'delete', label: t('agents.delete'), icon: <IconTrashOutline16 size={16} />, danger: true }]}
            anchor={<button type="button" className={css.iconButton} aria-label={t('agents.more')} aria-haspopup="menu" aria-expanded={menuOpen} onClick={() => { setMenuOpen(value => !value) }}><IconEllipsisOutline16 size={16} /></button>} />
        </div>
      </div>

      <div className={css.sections}>
        {tab === 'identity' && <section id={`chaos-agent-${profile.actor.id}-panel-identity`} role="tabpanel" className={css.panel} aria-labelledby={`chaos-agent-${profile.actor.id}-tab-identity`}>
          <div className={css.sectionHeading}>
            <h3>{t('agents.identity')}</h3>
            <p>{t('agents.identityHint')}</p>
          </div>
          {identityError !== null && <ErrorBanner>{t('agents.identityFailed', { error: identityError })}</ErrorBanner>}
          <label className={css.field}><span>{t('agents.name')}</span><input value={name} maxLength={64} disabled={identitySaving} onChange={event => { setName(event.target.value); setIdentityError(null) }} /></label>
          <label className={css.field}><span>{t('agents.charter')}</span><textarea value={description} maxLength={800} disabled={identitySaving} onChange={event => { setDescription(event.target.value); setIdentityError(null) }} /></label>
          <footer className={css.footer}>
            <Button variant="outline" size="sm" disabled={!identityDirty || identitySaving} onClick={() => { setName(profile.actor.displayName); setDescription(profile.charter.summary); setIdentityError(null) }}>{t('agents.discard')}</Button>
            <Button variant="primary" size="sm" disabled={!identityDirty || identitySaving || name.trim() === '' || description.trim() === ''} onClick={saveIdentity}>{identitySaving ? t('agents.saving') : t('agents.saveIdentity')}</Button>
          </footer>
        </section>}

        {tab === 'runtime' && <section id={`chaos-agent-${profile.actor.id}-panel-runtime`} role="tabpanel" className={css.panel} aria-labelledby={`chaos-agent-${profile.actor.id}-tab-runtime`}>
          <div className={css.sectionHeading}>
            <h3>{t('agents.runtime')}</h3>
            <p>{t('agents.runtimeHint')}</p>
          </div>
          {runtimeError !== null && <ErrorBanner>{t('agents.runtimeFailed', { error: runtimeError })}</ErrorBanner>}
          {catalogError !== null && <ErrorBanner>{t('create.routeFailed', { error: catalogError })} <button type="button" className={css.inlineAction} onClick={loadCatalog}>{t('create.routeRetry')}</button></ErrorBanner>}
          {catalog === null && catalogError === null && <p className={css.state} role="status">{t('create.routeLoading')}</p>}
          {catalog !== null && <div className={css.runtimeGrid}>
            <label className={css.field}><span>{t('create.provider')}</span><select value={provider} disabled={runtimeSaving} onChange={event => { setProvider(event.target.value); setModel(''); setRuntimeError(null) }}><option value="" disabled>{t('create.providerPlaceholder')}</option>{catalog.groups.map(group => <option key={group.id} value={group.id}>{group.name}</option>)}</select></label>
            <label className={css.field}><span>{t('create.model')}</span><select value={model} disabled={runtimeSaving || provider === ''} onChange={event => { setModel(event.target.value); setRuntimeError(null) }}><option value="" disabled>{models.length === 0 ? t('create.modelNone') : t('create.modelPick')}</option>{models.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
            <label className={css.field}><span>{t('create.preset')}</span><select value={presetId} disabled={runtimeSaving} onChange={event => { setPresetId(event.target.value); setRuntimeError(null) }}>{presets.map(item => <option key={item.id} value={item.id} disabled={item.broken !== undefined}>{item.name?.trim() || item.id}</option>)}</select></label>
          </div>}
          <p className={css.runtimeWarning}>{t('agents.runtimeWarning')}</p>
          <footer className={css.footer}>
            <Button variant="outline" size="sm" disabled={!runtimeDirty || runtimeSaving} onClick={() => { setProvider(profile.binding?.provider ?? ''); setModel(profile.binding?.model ?? ''); setPresetId(profile.binding?.preset ?? presets.find(item => item.isDefault)?.id ?? ''); setRuntimeError(null) }}>{t('agents.discard')}</Button>
            <Button variant="primary" size="sm" disabled={!runtimeDirty || runtimeSaving || provider === '' || model === '' || presetId === ''} onClick={saveRuntime}>{runtimeSaving ? t('agents.applying') : profile.binding === undefined ? t('agents.configureRuntime') : t('agents.applyRuntime')}</Button>
          </footer>
        </section>}

        {tab === 'collaboration' && <section id={`chaos-agent-${profile.actor.id}-panel-collaboration`} role="tabpanel" className={css.panel} aria-labelledby={`chaos-agent-${profile.actor.id}-tab-collaboration`}>
          <div className={css.sectionHeading}>
            <h3>{t('agents.collaboration')}</h3>
            <p>{t('agents.collaborationHint')}</p>
          </div>
          {membershipsError !== null && <ErrorBanner>{t('agents.membershipsFailed', { error: membershipsError })}</ErrorBanner>}
          {memberships === null && membershipsError === null && <p className={css.state} role="status">{t('agents.membershipsLoading')}</p>}
          {memberships !== null && memberships.length === 0 && <p className={css.state}>{t('agents.membershipsEmpty')}</p>}
          {memberships !== null && memberships.length > 0 && <div className={css.memberships}>{memberships.map(item => <div key={item.target.id} className={css.membership}><span>{item.target.kind === 'channel' ? '#' : '↔'} {item.target.name}</span><small>{item.role}</small></div>)}</div>}
        </section>}
      </div>
    </section>
  )
}
