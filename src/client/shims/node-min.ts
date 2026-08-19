// Browser shims for the node builtins that `vfile` (pulled in by
// react-markdown) imports unconditionally through its minpath/minproc/minurl
// re-export shims. DSH's client module table only hosts the declared
// neverBundle externals, so an unresolved `require("node:*")` bricks the whole
// plugin import. Only VFile path-mutation APIs touch these; react-markdown as
// used here never passes a `file` prop, but the shims stay functional so even
// `new VFile()` (minproc.cwd) cannot crash.

const sep = '/'

function normalizeSlashes(input: string): string {
  return input.replace(/\/{2,}/g, '/')
}

export function join(...parts: string[]): string {
  return normalizeSlashes(parts.filter(Boolean).join(sep))
}

export function dirname(p: string): string {
  const cleaned = p.replace(/\/+$/, '')
  const i = cleaned.lastIndexOf(sep)
  if (i < 0) return '.'
  return i === 0 ? sep : cleaned.slice(0, i)
}

export function basename(p: string, ext?: string): string {
  const b = p.split(sep).pop() ?? ''
  return ext && b.endsWith(ext) ? b.slice(0, -ext.length) : b
}

export function extname(p: string): string {
  const b = p.split(sep).pop() ?? ''
  const i = b.lastIndexOf('.')
  return i > 0 ? b.slice(i) : ''
}

export function fileURLToPath(url: string | URL): string {
  try {
    return decodeURIComponent(new URL(String(url)).pathname)
  } catch {
    return String(url)
  }
}

export default {
  sep,
  join,
  dirname,
  basename,
  extname,
  cwd: () => '/',
}
