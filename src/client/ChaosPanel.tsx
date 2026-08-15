import { useMemo, useState, type CSSProperties, type FormEvent } from 'react'
import type { HostObservable, InjectFace, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type { ChaosClientState } from './controller.ts'
import type { NativeTask } from '../native.ts'

export interface ChaosPanelInjected {
  hooks: { chaos: HostObservable<ChaosClientState> }
  ensure: () => Promise<void>
  refresh: () => Promise<void>
  selectTarget: (targetId: string) => Promise<void>
  createChannel: (name: string) => Promise<void>
  createDirect: (peerId: string) => Promise<void>
  addMember: (targetId: string, memberId: string) => Promise<void>
  createThread: (rootMessageId: string) => Promise<void>
  send: (text: string) => Promise<void>
  createTask: (messageId: string) => Promise<void>
  claimTask: (messageId: string) => Promise<void>
  unclaimTask: (task: NativeTask) => Promise<void>
  updateTask: (task: NativeTask, status: NativeTask['status']) => Promise<void>
}

export type ChaosPanelProps = PropsRuntime<'shell.overlay'> & InjectFace<ChaosPanelInjected>

const shell: CSSProperties = {
  position: 'fixed',
  right: 20,
  bottom: 20,
  zIndex: 80,
  pointerEvents: 'auto',
  fontFamily: 'var(--dsw-font-family)',
  color: 'var(--dsw-alias-label-primary, #111827)',
}

const panel: CSSProperties = {
  width: 'min(840px, calc(100vw - 40px))',
  height: 'min(600px, calc(100vh - 80px))',
  display: 'grid',
  gridTemplateColumns: '240px minmax(0, 1fr)',
  overflow: 'hidden',
  border: '1px solid var(--dsw-alias-border-l2, #d1d5db)',
  borderRadius: 14,
  background: 'var(--dsw-alias-bg-layer-1, #fff)',
  boxShadow: 'var(--dsw-shadow-lv3, 0 12px 32px rgba(0, 0, 0, 0.18))',
}

const button: CSSProperties = {
  border: '1px solid var(--dsw-alias-border-l2, #d1d5db)',
  borderRadius: 9,
  background: 'var(--dsw-alias-bg-layer-2, #f3f4f6)',
  color: 'inherit',
  padding: '8px 12px',
  cursor: 'pointer',
}

const input: CSSProperties = {
  minWidth: 0,
  border: '1px solid var(--dsw-alias-border-l2, #d1d5db)',
  borderRadius: 8,
  background: 'var(--dsw-alias-bg-layer-1, #fff)',
  color: 'inherit',
  padding: '8px 10px',
}

const taskTransitions: Record<NativeTask['status'], readonly NativeTask['status'][]> = {
  todo: ['in_progress'],
  in_progress: ['todo', 'in_review'],
  in_review: ['in_progress', 'done'],
  done: ['in_progress'],
}

export function ChaosPanel({
  useChaos,
  ensure,
  refresh,
  selectTarget,
  createChannel,
  createDirect,
  addMember,
  createThread,
  send,
  createTask,
  claimTask,
  unclaimTask,
  updateTask,
}: ChaosPanelProps) {
  const state = useChaos(value => value)
  const [open, setOpen] = useState(false)
  const [channelName, setChannelName] = useState('')
  const [draft, setDraft] = useState('')
  const [peerId, setPeerId] = useState('')
  const [memberId, setMemberId] = useState('')
  const [pending, setPending] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)
  const names = useMemo(
    () => new Map(state.actors.map(actor => [actor.id, actor.displayName])),
    [state.actors],
  )
  const selected = state.targets.find(target => target.id === state.selectedTargetId)
  const taskByMessage = useMemo(
    () => new Map(state.tasks.map(task => [task.messageId, task])),
    [state.tasks],
  )

  const run = async (operation: () => Promise<void>): Promise<void> => {
    setPending(true)
    setFailure(null)
    try {
      await operation()
    } catch (error) {
      setFailure(error instanceof Error ? error.message : String(error))
    } finally {
      setPending(false)
    }
  }

  const toggle = (): void => {
    const next = !open
    setOpen(next)
    if (next) void ensure()
  }

  const submitChannel = (event: FormEvent): void => {
    event.preventDefault()
    const name = channelName.trim()
    if (name === '') return
    void run(async () => {
      await createChannel(name)
      setChannelName('')
    })
  }

  const submitMessage = (event: FormEvent): void => {
    event.preventDefault()
    const text = draft.trim()
    if (text === '') return
    void run(async () => {
      await send(text)
      setDraft('')
    })
  }

  const submitDirect = (event: FormEvent): void => {
    event.preventDefault()
    if (peerId === '') return
    void run(async () => {
      await createDirect(peerId)
      setPeerId('')
    })
  }

  const submitMember = (event: FormEvent): void => {
    event.preventDefault()
    if (selected?.kind !== 'channel' || memberId === '') return
    void run(async () => {
      await addMember(selected.id, memberId)
      setMemberId('')
    })
  }

  if (!open) {
    return <div style={shell}><button style={button} onClick={toggle}>协作</button></div>
  }

  return (
    <div style={shell}>
      <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 8 }}>
        <button style={button} onClick={toggle}>关闭协作</button>
      </div>
      <section style={panel} aria-label="多 Agent 协作">
        <aside style={{ padding: 14, borderRight: '1px solid var(--dsw-alias-border-l2, #d1d5db)', overflow: 'auto' }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
            <strong>协作目标</strong>
            <button style={button} disabled={pending} onClick={() => { void refresh() }}>刷新</button>
          </div>
          <small>{state.stream === 'connected' ? '实时连接' : state.stream}</small>
          <form onSubmit={submitChannel} style={{ display: 'flex', gap: 6, margin: '12px 0' }}>
            <input
              style={{ ...input, flex: 1 }}
              value={channelName}
              onChange={event => { setChannelName(event.target.value) }}
              placeholder="新建 Channel"
            />
            <button style={button} disabled={pending}>新建</button>
          </form>
          <form onSubmit={submitDirect} style={{ display: 'flex', gap: 6, margin: '12px 0' }}>
            <select
              style={{ ...input, flex: 1 }}
              value={peerId}
              onChange={event => { setPeerId(event.target.value) }}
            >
              <option value="">选择直聊对象</option>
              {state.actors.filter(actor => actor.id !== state.actor?.id).map(actor => (
                <option key={actor.id} value={actor.id}>{actor.displayName} (@{actor.handle})</option>
              ))}
            </select>
            <button style={button} disabled={pending || peerId === ''}>直聊</button>
          </form>
          {selected?.kind === 'channel' && (
            <form onSubmit={submitMember} style={{ display: 'flex', gap: 6, margin: '12px 0' }}>
              <select
                style={{ ...input, flex: 1 }}
                value={memberId}
                onChange={event => { setMemberId(event.target.value) }}
              >
                <option value="">选择 Channel 成员</option>
                {state.actors.filter(actor => actor.id !== state.actor?.id).map(actor => (
                  <option key={actor.id} value={actor.id}>{actor.displayName} (@{actor.handle})</option>
                ))}
              </select>
              <button style={button} disabled={pending || memberId === ''}>加入</button>
            </form>
          )}
          <div style={{ display: 'grid', gap: 6 }}>
            {state.targets.map(target => (
              <button
                key={target.id}
                style={{
                  ...button,
                  textAlign: 'left',
                  background: target.id === state.selectedTargetId
                    ? 'var(--dsw-alias-interactive-bg-active, #e5e7eb)'
                    : 'var(--dsw-alias-bg-layer-2, #f3f4f6)',
                }}
                onClick={() => { void selectTarget(target.id) }}
              >
                <small>{target.kind}</small><br />{target.name}
              </button>
            ))}
          </div>
        </aside>
        <main style={{ minWidth: 0, display: 'grid', gridTemplateRows: 'auto 1fr auto', padding: 16 }}>
          <header style={{ borderBottom: '1px solid var(--dsw-alias-border-l2, #d1d5db)', paddingBottom: 10 }}>
            <strong>{selected?.name ?? '请选择协作目标'}</strong>
            {state.actor !== undefined && <small style={{ marginLeft: 10 }}>身份：{state.actor.displayName}</small>}
            {state.error !== undefined && <div role="alert">{state.error}</div>}
            {failure !== null && <div role="alert">{failure}</div>}
          </header>
          <div style={{ overflow: 'auto', padding: '12px 0', display: 'grid', alignContent: 'start', gap: 10 }}>
            {state.status === 'loading' && <p>正在加载……</p>}
            {state.messages.map(message => (
              <article key={message.id} style={{ padding: 10, borderRadius: 9, background: 'var(--dsw-alias-bg-layer-2, #f3f4f6)' }}>
                <strong>{names.get(message.authorId) ?? message.authorId}</strong>
                <p style={{ whiteSpace: 'pre-wrap', margin: '6px 0 0' }}>{message.text}</p>
                <div style={{ display: 'flex', gap: 6, marginTop: 8 }}>
                  {selected?.kind !== 'thread' && (
                    <button style={button} disabled={pending} onClick={() => {
                      void run(() => createThread(message.id))
                    }}>Thread</button>
                  )}
                  {!taskByMessage.has(message.id) && selected?.kind !== 'thread' && (
                    <button style={button} disabled={pending} onClick={() => {
                      void run(() => createTask(message.id))
                    }}>创建 Task</button>
                  )}
                </div>
              </article>
            ))}
            {state.tasks.length > 0 && (
              <section>
                <strong>Tasks</strong>
                {state.tasks.map(task => (
                  <div key={task.messageId} style={{ marginTop: 8 }}>
                    #{task.number} · {task.status} · {task.assigneeId === undefined ? '未认领' : names.get(task.assigneeId) ?? task.assigneeId}
                    <span style={{ display: 'inline-flex', gap: 6, marginLeft: 8 }}>
                      {task.assigneeId === undefined && task.status !== 'done' && (
                        <button style={button} disabled={pending} onClick={() => {
                          void run(() => claimTask(task.messageId))
                        }}>认领</button>
                      )}
                      {task.assigneeId === state.actor?.id && task.status !== 'done' && (
                        <button style={button} disabled={pending} onClick={() => {
                          void run(() => unclaimTask(task))
                        }}>取消认领</button>
                      )}
                      {(task.assigneeId === undefined
                        || task.assigneeId === state.actor?.id
                        || selected?.createdBy === state.actor?.id) && taskTransitions[task.status].map(status => (
                        <button key={status} style={button} disabled={pending} onClick={() => {
                          void run(() => updateTask(task, status))
                        }}>→ {status}</button>
                      ))}
                    </span>
                  </div>
                ))}
              </section>
            )}
          </div>
          <form onSubmit={submitMessage} style={{ display: 'flex', gap: 8 }}>
            <input
              style={{ ...input, flex: 1 }}
              value={draft}
              onChange={event => { setDraft(event.target.value) }}
              placeholder="发送消息"
              disabled={selected === undefined || pending}
            />
            <button style={button} disabled={selected === undefined || pending}>发送</button>
          </form>
        </main>
      </section>
    </div>
  )
}
