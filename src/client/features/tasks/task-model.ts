import type { NativeTask } from '../../../native.ts'
import type { ChaosKey, ChaosTranslate } from '../../locales.ts'

export type TaskStatus = NativeTask['status']

export const TASK_LANES: readonly TaskStatus[] = ['todo', 'in_progress', 'in_review', 'done']

export const TASK_LANE_LABEL_KEY: Record<TaskStatus, ChaosKey> = {
  todo: 'tasks.lane.todo',
  in_progress: 'tasks.lane.inProgress',
  in_review: 'tasks.lane.inReview',
  done: 'tasks.lane.done',
}

/** Must remain in lockstep with collab-core's task_transition_allowed. */
export const TASK_TRANSITIONS: Record<TaskStatus, readonly TaskStatus[]> = {
  todo: ['in_progress'],
  in_progress: ['todo', 'in_review'],
  in_review: ['in_progress', 'done'],
  done: ['in_progress'],
}

export function formatTaskTime(ms: number, t: ChaosTranslate): string {
  const minutes = Math.floor((Date.now() - ms) / 60000)
  if (minutes < 1) return t('tasks.time.justNow')
  if (minutes < 60) return `${minutes}m`
  if (minutes < 60 * 24) return `${Math.floor(minutes / 60)}h`
  const date = new Date(ms)
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}

export function splitTaskAnchor(text: string): { title: string, excerpt: string } {
  const trimmed = text.trim()
  const cut = trimmed.indexOf('\n')
  if (cut < 0) return { title: trimmed, excerpt: '' }
  return { title: trimmed.slice(0, cut), excerpt: trimmed.slice(cut + 1).trim() }
}
