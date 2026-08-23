#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { basename, extname, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const tracked = execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard', '-z'], { cwd: root })
  .toString('utf8')
  .split('\0')
  .filter(Boolean)
const failures = []
const forbiddenNames = new Set(['.env', '.npmrc', 'id_rsa', 'id_ed25519'])
const forbiddenExtensions = new Set(['.db', '.key', '.p12', '.pfx', '.pem'])
const secretPatterns = [
  ['npm token', /npm_[A-Za-z0-9]{30,}/],
  ['GitHub token', /gh[opusr]_[A-Za-z0-9]{30,}/],
  ['OpenAI-style secret', /sk-[A-Za-z0-9_-]{20,}/],
  ['private key', /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/],
  ['machine-local home path', /(?:\/home\/[^/\s]+\/|[A-Za-z]:\\Users\\[^\\\s]+\\)/],
]

for (const path of tracked) {
  if (forbiddenNames.has(basename(path)) || forbiddenExtensions.has(extname(path))) {
    failures.push(`${path}: sensitive filename or extension`)
    continue
  }
  let source
  try {
    source = readFileSync(resolve(root, path), 'utf8')
  } catch {
    continue
  }
  for (const [label, pattern] of secretPatterns) {
    if (pattern.test(source)) failures.push(`${path}: possible ${label}`)
  }
}

const history = execFileSync('git', ['log', '--all', '-p', '--format='], {
  cwd: root,
  encoding: 'utf8',
  maxBuffer: 256 * 1024 * 1024,
})
for (const [label, pattern] of secretPatterns.slice(0, 4)) {
  if (pattern.test(history)) failures.push(`Git history: possible ${label}`)
}

if (failures.length > 0) {
  console.error(['public repository check failed', ...failures.map(failure => `- ${failure}`)].join('\n'))
  process.exitCode = 1
} else {
  console.log(`public repository check passed (${tracked.length} candidate files and complete Git patch history scanned)`)
}
