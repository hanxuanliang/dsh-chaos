#!/usr/bin/env node

import { spawn } from 'node:child_process'
import { constants as fsConstants } from 'node:fs'
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { delimiter, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const packageJson = JSON.parse(await readFile(join(repoRoot, 'package.json'), 'utf8'))
const expectedDshVersion = packageJson.devDependencies['@deepseek-ai/dsh-agent']
const runStamp = new Date().toISOString().replaceAll(':', '-').replaceAll('.', '-')
const artifactRoot = resolve(process.env.DSH_E2E_ARTIFACTS ?? join(repoRoot, 'artifacts', 'e2e', runStamp))
const keepWorkdir = process.env.DSH_E2E_KEEP === '1'
const headful = process.env.DSH_E2E_HEADFUL === '1'
const timeoutMs = Number(process.env.DSH_E2E_TIMEOUT_MS ?? 30_000)

await mkdir(artifactRoot, { recursive: true })

function assert(condition, message) {
  if (!condition) throw new Error(message)
}

async function executable(path) {
  if (path === undefined || path === '') return false
  try {
    await access(path, fsConstants.X_OK)
    return true
  } catch {
    return false
  }
}

async function findExecutable(explicit, candidates) {
  if (explicit !== undefined) {
    const resolved = resolve(explicit)
    if (await executable(resolved)) return resolved
    throw new Error(`configured executable is not runnable: ${explicit}`)
  }
  const pathEntries = (process.env.PATH ?? '').split(delimiter)
  for (const candidate of candidates) {
    if (candidate.includes('/')) {
      if (await executable(candidate)) return candidate
      continue
    }
    for (const entry of pathEntries) {
      const path = join(entry, candidate)
      if (await executable(path)) return path
    }
  }
  return undefined
}

function collectProcess(command, args, options = {}) {
  const lines = []
  const child = spawn(command, args, {
    cwd: options.cwd ?? repoRoot,
    env: options.env ?? process.env,
    detached: process.platform !== 'win32',
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  for (const [streamName, stream] of [['stdout', child.stdout], ['stderr', child.stderr]]) {
    stream.setEncoding('utf8')
    stream.on('data', chunk => {
      lines.push(`[${streamName}] ${chunk}`)
      if (process.env.DSH_E2E_DEBUG === '1') process.stderr.write(chunk)
    })
  }
  return { child, lines }
}

async function run(command, args, options = {}) {
  const processState = collectProcess(command, args, options)
  const exit = await new Promise((resolveExit, reject) => {
    processState.child.once('error', reject)
    processState.child.once('exit', (code, signal) => resolveExit({ code, signal }))
  })
  if (exit.code !== 0) {
    throw new Error([
      `${command} ${args.join(' ')} failed (${exit.code ?? exit.signal})`,
      ...processState.lines,
    ].join('\n'))
  }
  return processState.lines.join('')
}

async function terminate(processState) {
  if (processState === undefined
    || processState.child.exitCode !== null
    || processState.child.signalCode !== null) return
  const pid = processState.child.pid
  if (pid === undefined) return
  try {
    if (process.platform === 'win32') processState.child.kill('SIGTERM')
    else process.kill(-pid, 'SIGTERM')
  } catch {}
  await Promise.race([
    new Promise(resolveExit => processState.child.once('exit', resolveExit)),
    new Promise(resolveTimeout => setTimeout(resolveTimeout, 2_000)),
  ])
  if (processState.child.exitCode === null && processState.child.signalCode === null) {
    try {
      if (process.platform === 'win32') processState.child.kill('SIGKILL')
      else process.kill(-pid, 'SIGKILL')
    } catch {}
  }
}

async function freePort() {
  return await new Promise((resolvePort, reject) => {
    const server = createServer()
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      assert(typeof address === 'object' && address !== null, 'failed to allocate a local port')
      const port = address.port
      server.close(error => error === undefined ? resolvePort(port) : reject(error))
    })
  })
}

async function waitFor(label, probe, timeout = timeoutMs) {
  const started = Date.now()
  let lastError
  while (Date.now() - started < timeout) {
    try {
      const value = await probe()
      if (value) return value
    } catch (error) {
      lastError = error
    }
    await new Promise(resolveWait => setTimeout(resolveWait, 100))
  }
  const suffix = lastError === undefined ? '' : `: ${String(lastError)}`
  throw new Error(`timed out waiting for ${label}${suffix}`)
}

class CdpPage {
  constructor(webSocketUrl, pageUrl) {
    this.webSocketUrl = webSocketUrl
    this.pageUrl = pageUrl
    this.sequence = 0
    this.pending = new Map()
    this.consoleErrors = []
    this.pageErrors = []
    this.requestFailures = []
  }

  async connect() {
    this.socket = new WebSocket(this.webSocketUrl)
    await new Promise((resolveOpen, reject) => {
      this.socket.onopen = resolveOpen
      this.socket.onerror = reject
    })
    this.socket.onmessage = event => {
      const message = JSON.parse(event.data)
      if (message.id !== undefined) {
        const waiter = this.pending.get(message.id)
        if (waiter === undefined) return
        this.pending.delete(message.id)
        if (message.error !== undefined) waiter.reject(new Error(message.error.message))
        else waiter.resolve(message.result)
        return
      }
      if (message.method === 'Runtime.consoleAPICalled') {
        if (!['error', 'assert'].includes(message.params.type)) return
        const text = (message.params.args ?? [])
          .map(value => value.value ?? value.description ?? value.type)
          .join(' ')
        this.consoleErrors.push(text.slice(0, 1_000))
      }
      if (message.method === 'Runtime.exceptionThrown') {
        const details = message.params.exceptionDetails
        this.pageErrors.push((details.exception?.description ?? details.text).slice(0, 2_000))
      }
      if (message.method === 'Network.loadingFailed') {
        const failure = message.params
        if (failure.canceled || failure.errorText === 'net::ERR_ABORTED') return
        this.requestFailures.push(`${failure.type}: ${failure.errorText}`)
      }
      if (message.method === 'Network.responseReceived') {
        const { response, type } = message.params
        if (response.status < 400 || !['Document', 'Script', 'Fetch', 'XHR'].includes(type)) return
        this.requestFailures.push(`${type}: ${response.status} ${response.url}`)
      }
    }
    await Promise.all([
      this.send('Runtime.enable'),
      this.send('Page.enable'),
      this.send('Network.enable'),
    ])
    await this.send('Page.bringToFront')
  }

  send(method, params = {}) {
    const id = ++this.sequence
    return new Promise((resolveResult, reject) => {
      this.pending.set(id, { resolve: resolveResult, reject })
      this.socket.send(JSON.stringify({ id, method, params }))
    })
  }

  async evaluate(expression) {
    const result = await this.send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    })
    if (result.exceptionDetails !== undefined) {
      throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text)
    }
    return result.result.value
  }

  async waitForExpression(label, expression, timeout = timeoutMs) {
    return await waitFor(label, async () => await this.evaluate(expression), timeout)
  }

  async clickExpression(label, expression) {
    const clicked = await this.evaluate(`(() => {
      const element = ${expression};
      if (!(element instanceof HTMLElement) || element.offsetParent === null) return false;
      element.click();
      return true;
    })()`)
    assert(clicked, `could not click ${label}`)
  }

  async clickTextIfVisible(text) {
    return await this.evaluate(`(() => {
      const expected = ${JSON.stringify(text)};
      const element = [...document.querySelectorAll('button')]
        .find(candidate => candidate.offsetParent !== null && candidate.textContent?.trim() === expected);
      if (!(element instanceof HTMLElement)) return false;
      element.click();
      return true;
    })()`)
  }

  async fill(selector, value) {
    const point = await this.evaluate(`(() => {
      const element = [...document.querySelectorAll(${JSON.stringify(selector)})]
        .find(candidate => candidate instanceof HTMLElement && candidate.offsetParent !== null);
      if (!(element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement)) return null;
      const rect = element.getBoundingClientRect();
      return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
    })()`)
    assert(point !== null, `could not locate visible ${selector}`)
    await this.send('Input.dispatchMouseEvent', {
      type: 'mousePressed',
      x: point.x,
      y: point.y,
      button: 'left',
      clickCount: 1,
    })
    await this.send('Input.dispatchMouseEvent', {
      type: 'mouseReleased',
      x: point.x,
      y: point.y,
      button: 'left',
      clickCount: 1,
    })
    const focused = await this.evaluate(`(() => {
      const element = [...document.querySelectorAll(${JSON.stringify(selector)})]
        .find(candidate => candidate instanceof HTMLElement && candidate.offsetParent !== null);
      return document.activeElement === element;
    })()`)
    assert(focused, `could not focus ${selector} through a real pointer click`)
    await this.send('Input.dispatchKeyEvent', {
      type: 'rawKeyDown',
      modifiers: 2,
      key: 'a',
      code: 'KeyA',
      windowsVirtualKeyCode: 65,
    })
    await this.send('Input.dispatchKeyEvent', {
      type: 'keyUp',
      modifiers: 2,
      key: 'a',
      code: 'KeyA',
      windowsVirtualKeyCode: 65,
    })
    await this.send('Input.insertText', { text: value })
    await this.waitForExpression(
      `${selector} value`,
      `[...document.querySelectorAll(${JSON.stringify(selector)})]
        .some(element => element.offsetParent !== null && element.value === ${JSON.stringify(value)})`,
    )
  }

  async screenshot(name) {
    const result = await this.send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false })
    await writeFile(join(artifactRoot, name), Buffer.from(result.data, 'base64'))
  }

  async reload() {
    await this.send('Page.reload', { ignoreCache: true })
    await this.waitForExpression('reloaded DSH page', `document.readyState === 'complete' && document.title === 'DeepSeek Harness'`)
  }

  async setViewport(width, height) {
    await this.send('Emulation.setDeviceMetricsOverride', {
      width,
      height,
      deviceScaleFactor: 1,
      mobile: false,
    })
  }

  close() {
    this.socket?.close()
  }
}

async function dismissOnboarding(page) {
  const deadline = Date.now() + 3_000
  let quietSince = Date.now()
  while (Date.now() < deadline) {
    const continued = await page.clickTextIfVisible('Continue')
    const deferred = await page.clickTextIfVisible('Configure later')
    if (continued || deferred) quietSince = Date.now()
    else if (Date.now() - quietSince >= 750) return
    await new Promise(resolveWait => setTimeout(resolveWait, 100))
  }
  throw new Error('first-run onboarding did not settle')
}

const dshBin = await findExecutable(process.env.DSH_E2E_BIN, [join(repoRoot, 'node_modules', '.bin', 'dsh'), 'dsh'])
assert(
  dshBin !== undefined,
  'DSH CLI not found. Install the official CLI and set DSH_E2E_BIN to its executable.',
)
const browserBin = await findExecutable(process.env.DSH_E2E_BROWSER, [
  'chromium',
  'chromium-browser',
  'google-chrome',
  'google-chrome-stable',
])
assert(
  browserBin !== undefined,
  'Chromium or Chrome not found. Set DSH_E2E_BROWSER to its executable.',
)

const dshVersionOutput = await run(dshBin, ['--version'])
const dshVersion = dshVersionOutput.match(/\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?/)?.[0]
assert(dshVersion !== undefined, `could not parse DSH CLI version from: ${dshVersionOutput.trim()}`)
const dshWebHelp = await run(dshBin, ['web', '--help'])
if (process.env.DSH_E2E_ALLOW_VERSION_MISMATCH !== '1') {
  assert(
    dshVersion === expectedDshVersion,
    `DSH CLI ${dshVersion} does not match the repository contract ${expectedDshVersion}`,
  )
}

const workRoot = await mkdtemp(join(tmpdir(), 'dsh-chaos-e2e-'))
const dshHome = join(workRoot, 'dsh-home')
const browserHome = join(workRoot, 'browser')
const webPort = await freePort()
const debugPort = await freePort()
const pageUrl = `http://127.0.0.1:${webPort}/`
const dshEnv = {
  ...process.env,
  DSH_HOME: dshHome,
  XDG_CONFIG_HOME: join(workRoot, 'xdg-config'),
  XDG_CACHE_HOME: join(workRoot, 'xdg-cache'),
  NO_PROXY: '127.0.0.1,localhost',
}

let dshProcess
let browserProcess
let page
let failure
try {
  await run(dshBin, ['plugin', '--profile', 'web', 'add', '--workspace-root', repoRoot], { env: dshEnv })

  const webArgs = ['web', '--host', '127.0.0.1', '--port', String(webPort)]
  if (dshWebHelp.includes('--no-open')) webArgs.push('--no-open')
  dshProcess = collectProcess(dshBin, webArgs, { env: dshEnv })
  await waitFor('DSH HTTP server', async () => {
    if (dshProcess.child.exitCode !== null || dshProcess.child.signalCode !== null) {
      throw new Error(`DSH exited early:\n${dshProcess.lines.join('')}`)
    }
    const response = await fetch(pageUrl)
    return response.ok
  })

  const browserArgs = [
    ...(headful ? [] : ['--headless=new']),
    '--no-sandbox',
    '--disable-dev-shm-usage',
    '--disable-background-networking',
    '--disable-component-update',
    '--disable-default-apps',
    '--disable-sync',
    '--no-first-run',
    '--lang=en-US',
    `--remote-debugging-port=${debugPort}`,
    `--user-data-dir=${browserHome}`,
    '--window-size=1200,800',
    pageUrl,
  ]
  browserProcess = collectProcess(browserBin, browserArgs)
  const target = await waitFor('browser DevTools endpoint', async () => {
    if (browserProcess.child.exitCode !== null || browserProcess.child.signalCode !== null) {
      throw new Error(`browser exited early:\n${browserProcess.lines.join('')}`)
    }
    const response = await fetch(`http://127.0.0.1:${debugPort}/json`)
    if (!response.ok) return undefined
    const targets = await response.json()
    return targets.find(candidate => candidate.type === 'page' && candidate.url.startsWith(pageUrl))
      ?? targets.find(candidate => candidate.type === 'page')
  })
  page = new CdpPage(target.webSocketDebuggerUrl, pageUrl)
  await page.connect()
  await page.waitForExpression('DSH application shell', `document.body.innerText.includes('Settings')`)
  await dismissOnboarding(page)
  const loadFailure = await page.evaluate(`document.body.innerText.includes('Failed to load plugins') ? document.body.innerText : ''`)
  assert(loadFailure === '', `DSH failed to load plugins:\n${loadFailure}`)

  await page.waitForExpression('Collab entry', `!![...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.getAttribute('aria-label') === 'Collab')`)
  await page.clickExpression('Collab entry', `[...document.querySelectorAll('button')]
    .find(button => button.getAttribute('aria-label') === 'Collab')`)
  await page.waitForExpression('Channels navigation', `document.querySelector('nav[aria-label="Channels"]')?.offsetParent !== null`)
  await page.waitForExpression('panel header uses underline tabs (plan A)', `(() => {
    const tabs = [...document.querySelectorAll('[role="tab"]')]
      .filter(tab => tab instanceof HTMLElement && tab.offsetParent !== null
        && tab.closest('header') !== null);
    if (tabs.length < 2) return false;
    const active = tabs.find(tab => tab.getAttribute('aria-selected') === 'true');
    if (active === undefined) return false;
    const underline = getComputedStyle(active, '::after');
    return getComputedStyle(active).fontWeight === '600'
      && underline.height !== '' && parseFloat(underline.height) >= 1.5;
  })()`)
  await page.waitForExpression('channel rail is a resizable pane', `(() => {
    const rail = document.querySelector('nav[aria-label="Channels"]');
    if (!(rail instanceof HTMLElement) || rail.offsetParent === null) return false;
    const separator = [...document.querySelectorAll('[aria-label="Resize channel rail"]')]
      .find(element => element instanceof HTMLElement);
    return separator !== undefined && getComputedStyle(separator).cursor === 'col-resize';
  })()`)
  await page.waitForExpression('active Channel group starts expanded', `(() => {
    const button = document.querySelector('button[data-channel-group-toggle="active"]');
    const chevron = button?.querySelector('[data-open="true"]');
    return button?.getAttribute('aria-expanded') === 'true'
      && document.querySelector('[data-channel-group-list="active"]') !== null
      && chevron instanceof HTMLElement
      && getComputedStyle(chevron).transform !== 'none';
  })()`)
  await page.clickExpression('collapse active Channel group', `document.querySelector('button[data-channel-group-toggle="active"]')`)
  await page.waitForExpression('active Channel group collapsed', `(() => {
    const button = document.querySelector('button[data-channel-group-toggle="active"]');
    return button?.getAttribute('aria-expanded') === 'false'
      && document.querySelector('[data-channel-group-list="active"]') === null
      && button?.querySelector('[data-open="true"]') === null;
  })()`)
  await page.clickExpression('expand active Channel group', `document.querySelector('button[data-channel-group-toggle="active"]')`)
  await page.waitForExpression('active Channel group reopened', `document.querySelector('button[data-channel-group-toggle="active"]')?.getAttribute('aria-expanded') === 'true'
    && document.querySelector('[data-channel-group-list="active"]') !== null`)
  await page.waitForExpression('compact empty Channel state has a real action', `(() => {
    const nav = document.querySelector('nav[aria-label="Channels"]');
    const action = [...document.querySelectorAll('button')]
      .find(button => button.offsetParent !== null && button.textContent?.trim() === 'New channel');
    return nav?.textContent?.includes('No channels yet') === true
      && nav.textContent.includes('press +') === false
      && action instanceof HTMLButtonElement;
  })()`)

  const channelName = `e2e-core-${Date.now().toString(36)}`
  const messageText = `E2E message ${Date.now().toString(36)}`
  await page.clickExpression('New channel', `document.querySelector('button[aria-label="New channel"]')`)
  await page.waitForExpression('New Channel dialog', `document.querySelector('input[placeholder="e.g. frontend-sync"]')?.offsetParent !== null`)
  await page.waitForExpression('Channel rules use compact help and counter', `(() => {
    const nameHelp = document.querySelector('button[aria-label="The leading # is optional."]');
    const descriptionHelp = document.querySelector('button[aria-label="Included in the Agent’s channel context."]');
    const stack = document.querySelector('[data-channel-form-stack="create"]');
    const body = document.body.innerText;
    if (!(stack instanceof HTMLElement) || stack.children.length !== 3) return false;
    const nameRect = stack.children[0].getBoundingClientRect();
    const descriptionRect = stack.children[1].getBoundingClientRect();
    const membersRect = stack.children[2].getBoundingClientRect();
    const firstGap = descriptionRect.top - nameRect.bottom;
    const secondGap = membersRect.top - descriptionRect.bottom;
    return nameHelp?.offsetParent !== null
      && descriptionHelp?.offsetParent !== null
      && body.includes('0/280')
      && body.includes('No Agents available')
      && firstGap >= 11 && firstGap <= 13
      && secondGap >= 11 && secondGap <= 13
      && body.includes('A leading # is optional; duplicate names are not allowed here.') === false;
  })()`)
  await page.evaluate(`document.querySelector('button[aria-label="The leading # is optional."]')?.focus()`)
  await page.waitForExpression('Channel name tooltip', `document.body.innerText.includes('The leading # is optional.')`)
  await page.evaluate(`document.querySelector('input[placeholder="e.g. frontend-sync"]')?.focus()`)
  await page.waitForExpression('shared blue-focused Channel input', `(() => {
    const input = document.querySelector('input[placeholder="e.g. frontend-sync"]');
    const frame = input?.parentElement;
    if (!(input instanceof HTMLInputElement)
      || input.dataset.chaosTextInput !== 'true'
      || !(frame instanceof HTMLElement)
      || document.activeElement !== input) return false;
    const style = getComputedStyle(frame);
    const probe = document.createElement('span');
    probe.style.color = 'var(--dsw-alias-state-business-primary)';
    frame.append(probe);
    const brand = getComputedStyle(probe).color;
    probe.remove();
    return style.borderColor === brand && style.boxShadow !== 'none';
  })()`)
  await page.screenshot('channel-create-focus.png')
  await page.fill('input[placeholder="e.g. frontend-sync"]', channelName)
  await page.fill('textarea[placeholder="Describe this channel’s purpose, scope, and collaboration rules."]', 'E2E channel lifecycle validation')
  await page.clickExpression('New Agent from empty member state', `[...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.textContent?.trim() === 'New Agent')`)
  await page.waitForExpression('single-screen Agent creation dialog', `(() => {
    const input = document.querySelector('#chaos-agent-create-name');
    const dialogs = [...document.querySelectorAll('[role="dialog"]')]
      .filter(dialog => dialog instanceof HTMLElement && dialog.offsetParent !== null);
    const dialog = dialogs[0];
    const tabs = [...(dialog?.querySelectorAll('[role="tab"]') ?? [])];
    const pills = [...(dialog?.querySelectorAll('button[aria-haspopup="listbox"]') ?? [])]
      .filter(button => button instanceof HTMLElement && button.offsetParent !== null);
    const actions = [...(dialog?.querySelectorAll('footer button') ?? [])]
      .filter(button => button instanceof HTMLElement && button.offsetParent !== null);
    return input?.offsetParent !== null
      && dialogs.length === 1
      && tabs.length === 0
      && pills.length === 3
      && actions.length === 2
      && document.body.innerText.includes('Handle') === false;
  })()`)
  await page.screenshot('agent-create-single-screen.png')
  await page.clickExpression('Cancel Agent creation', `[...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.textContent?.trim() === 'Cancel')`)
  await page.waitForExpression('Channel draft restored after Agent creation cancel', `(() => {
    const name = document.querySelector('input[placeholder="e.g. frontend-sync"]');
    const description = document.querySelector('textarea[placeholder="Describe this channel’s purpose, scope, and collaboration rules."]');
    return name instanceof HTMLInputElement
      && description instanceof HTMLTextAreaElement
      && name.offsetParent !== null
      && name.value === ${JSON.stringify(channelName)}
      && description.value === 'E2E channel lifecycle validation';
  })()`)
  await page.setViewport(650, 800)
  await page.waitForExpression('compact Channel dialog fits narrow viewport', `(() => {
    const dialog = [...document.querySelectorAll('[role="dialog"]')]
      .find(candidate => candidate instanceof HTMLElement && candidate.offsetParent !== null);
    if (!(dialog instanceof HTMLElement)) return false;
    const rect = dialog.getBoundingClientRect();
    return rect.left >= 12 && rect.right <= 638 && rect.bottom <= 788;
  })()`)
  await page.screenshot('channel-create-narrow.png')
  await page.setViewport(1200, 800)
  await page.clickExpression('Create channel', `[...document.querySelectorAll('button')]
    .find(button => button.textContent?.trim() === 'Create channel')`)
  await page.waitForExpression('created channel', `!!document.querySelector(${JSON.stringify(`section[aria-label="# ${channelName}"]`)})`)
  await page.waitForExpression('closed New Channel dialog', `(() => {
    const input = document.querySelector('input[placeholder="e.g. frontend-sync"]');
    return input === null || input.offsetParent === null;
  })()`)
  // Host Modal teardown also closes the shell overlay. Wait for that contract
  // to settle before deciding whether the Collab entry must be reopened.
  await new Promise(resolveWait => setTimeout(resolveWait, 500))
  await dismissOnboarding(page)

  const channelVisible = await page.evaluate(`document.querySelector(${JSON.stringify(`section[aria-label="# ${channelName}"]`)})?.offsetParent !== null`)
  if (!channelVisible) {
    await page.clickExpression('Collab entry after dialog', `[...document.querySelectorAll('button')]
      .find(button => button.getAttribute('aria-label') === 'Collab')`)
    await new Promise(resolveWait => setTimeout(resolveWait, 350))
  }
  const composerSelector = `textarea[placeholder^="Message #${channelName}"]`
  await page.waitForExpression('channel composer', `[...document.querySelectorAll(${JSON.stringify(composerSelector)})]
    .some(element => element.offsetParent !== null)`)
  await page.clickExpression('Channel members', `[...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.getAttribute('aria-label') === 'Members')`)
  await page.clickExpression('Add member', `[...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.textContent?.trim() === 'Add member')`)
  await page.waitForExpression('member picker has compact Agent action', `(() => {
    const action = [...document.querySelectorAll('button')]
      .find(button => button.offsetParent !== null && button.textContent?.trim() === 'New Agent');
    return document.body.innerText.includes('No Agents available') && action instanceof HTMLButtonElement;
  })()`)
  await page.screenshot('member-empty-action.png')
  await page.clickExpression('Close members dialog', `[...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.getAttribute('aria-label') === 'Close')`)
  await page.waitForExpression('closed members dialog', `![...document.querySelectorAll('button')]
    .some(button => button.offsetParent !== null && button.textContent?.trim() === 'Add member')`)
  await page.fill(composerSelector, messageText)
  await page.waitForExpression('enabled channel send button', `!![...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.getAttribute('aria-label') === 'Send' && !button.disabled)`)
  await page.clickExpression('Send', `[...document.querySelectorAll('button')]
    .find(button => button.getAttribute('aria-label') === 'Send' && !button.disabled)`)
  const committedMessageExpression = `(() => {
    const channel = document.querySelector(${JSON.stringify(`section[aria-label="# ${channelName}"]`)});
    if (!(channel instanceof HTMLElement) || channel.offsetParent === null) return false;
    return [...channel.querySelectorAll('[data-message-id]')]
      .some(row => row instanceof HTMLElement && row.offsetParent !== null && row.textContent?.includes(${JSON.stringify(messageText)}));
  })()`
  await page.waitForExpression('committed Message row', committedMessageExpression)
  await page.waitForExpression('cleared Channel draft', `[...document.querySelectorAll(${JSON.stringify(composerSelector)})]
    .some(element => element.offsetParent !== null && element.value === '')`)

  await page.clickExpression('Channel actions', `[...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.getAttribute('aria-label') === ${JSON.stringify(`Actions for ${channelName}`)})`)
  await page.waitForExpression('Channel actions menu', `!![...document.querySelectorAll('[role="menuitem"]')]
    .find(item => item.offsetParent !== null && item.textContent?.trim() === 'Edit details')`)
  await page.clickExpression('Edit channel details', `[...document.querySelectorAll('[role="menuitem"]')]
    .find(item => item.textContent?.trim() === 'Edit details')`)
  const editedDescription = 'Updated E2E channel purpose'
  await page.waitForExpression('Edit Channel dialog', `document.querySelector('#chaos-channel-edit-description')?.offsetParent !== null`)
  await page.fill('#chaos-channel-edit-description', editedDescription)
  await page.clickExpression('Save channel details', `[...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.textContent?.trim() === 'Save' && !button.disabled)`)
  await page.waitForExpression('closed Edit Channel dialog', `(() => {
    const description = document.querySelector('#chaos-channel-edit-description');
    return description === null || description.offsetParent === null;
  })()`)

  await page.clickExpression('Channel actions after edit', `[...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.getAttribute('aria-label') === ${JSON.stringify(`Actions for ${channelName}`)})`)
  await page.clickExpression('Archive channel', `[...document.querySelectorAll('[role="menuitem"]')]
    .find(item => item.offsetParent !== null && item.textContent?.trim() === 'Archive channel')`)
  await page.waitForExpression('archived read-only Channel', `(() => {
    const channel = document.querySelector(${JSON.stringify(`section[aria-label="# ${channelName}"]`)});
    if (!(channel instanceof HTMLElement) || channel.offsetParent === null) return false;
    const archived = [...channel.querySelectorAll('*')].some(element => element.textContent?.trim() === 'Archived');
    const composer = [...channel.querySelectorAll('textarea')].some(element => element.offsetParent !== null);
    return archived && !composer;
  })()`)
  await page.screenshot('channel-archived.png')
  await page.clickExpression('Archived group', `[...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.textContent?.trim().startsWith('Archived'))`)
  await page.waitForExpression('Archived group expands with rotating chevron and Channel spacing', `(() => {
    const button = document.querySelector('button[data-channel-group-toggle="archived"]');
    const list = document.querySelector('[data-channel-group-list="archived"]');
    const chevron = button?.querySelector('[data-open="true"]');
    if (!(button instanceof HTMLElement) || !(list instanceof HTMLElement) || !(chevron instanceof HTMLElement)) return false;
    return button.getAttribute('aria-expanded') === 'true'
      && getComputedStyle(chevron).transform !== 'none'
      && list.getBoundingClientRect().top - button.getBoundingClientRect().bottom >= 5;
  })()`)
  await page.clickExpression('Archived Channel actions', `[...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.getAttribute('aria-label') === ${JSON.stringify(`Actions for ${channelName}`)})`)
  await page.waitForExpression('Restore archived Channel action', `!![...document.querySelectorAll('[role="menuitem"]')]
    .find(item => item.offsetParent !== null && item.textContent?.trim() === 'Restore channel')`)
  await page.screenshot('channel-archived-actions.png')
  await page.clickExpression('Restore archived channel', `[...document.querySelectorAll('[role="menuitem"]')]
    .find(item => item.offsetParent !== null && item.textContent?.trim() === 'Restore channel')`)
  await page.waitForExpression('restored Channel composer', `[...document.querySelectorAll(${JSON.stringify(composerSelector)})]
    .some(element => element.offsetParent !== null)`)
  await page.screenshot('wide.png')
  await page.waitForExpression('composer docked to the bottom (IM logic)', `(() => {
    const composer = document.querySelector(${JSON.stringify(composerSelector)});
    if (!(composer instanceof HTMLElement)) return false;
    const rect = composer.getBoundingClientRect();
    return rect.bottom > 0 && window.innerHeight - rect.bottom < 60;
  })()`)

  await page.reload()
  await dismissOnboarding(page)
  await page.waitForExpression('Collab entry after reload', `!![...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.getAttribute('aria-label') === 'Collab')`)
  await page.clickExpression('Collab entry after reload', `[...document.querySelectorAll('button')]
    .find(button => button.getAttribute('aria-label') === 'Collab')`)
  await new Promise(resolveWait => setTimeout(resolveWait, 350))
  await dismissOnboarding(page)
  const persistedChannelVisible = await page.evaluate(`document.querySelector(${JSON.stringify(`section[aria-label="# ${channelName}"]`)})?.offsetParent !== null`)
  if (!persistedChannelVisible) {
    await page.clickExpression('Collab entry after reload onboarding', `[...document.querySelectorAll('button')]
      .find(button => button.getAttribute('aria-label') === 'Collab')`)
  }
  await page.waitForExpression('persisted channel and Message', committedMessageExpression)
  await page.waitForExpression('no visible onboarding dialog', `![...document.querySelectorAll('button')]
    .some(button => button.offsetParent !== null && ['Continue', 'Configure later', 'Save and continue'].includes(button.textContent?.trim()))`)

  const canCollapseSidebar = await page.evaluate(`!![...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.getAttribute('aria-label') === 'Collapse sidebar')`)
  if (canCollapseSidebar) {
    await page.clickExpression('Collapse sidebar', `[...document.querySelectorAll('button')]
      .find(button => button.getAttribute('aria-label') === 'Collapse sidebar')`)
  }
  await page.setViewport(650, 800)
  await page.waitForExpression('usable narrow Channel view', `(() => {
    const channel = document.querySelector(${JSON.stringify(`section[aria-label="# ${channelName}"]`)});
    const composer = [...document.querySelectorAll(${JSON.stringify(composerSelector)})]
      .find(element => element.offsetParent !== null);
    return channel?.offsetParent !== null
      && channel.getBoundingClientRect().width >= 300
      && composer?.getBoundingClientRect().width >= 280;
  })()`)
  await page.screenshot('narrow.png')

  await page.setViewport(1200, 800)
  await page.clickExpression('Settings', `[...document.querySelectorAll('button')]
    .find(button => {
      if (button.offsetParent === null) return false;
      const rect = button.getBoundingClientRect();
      return button.textContent?.trim() === 'Settings'
        || button.getAttribute('aria-label') === 'Settings'
        || button.getAttribute('title') === 'Settings'
        || (rect.left < 64 && rect.top > window.innerHeight - 96);
    })`)
  await page.waitForExpression('Collab Agents settings entry', `!![...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.textContent?.trim() === 'Collab Agents')`)
  await page.clickExpression('Collab Agents settings entry', `[...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.textContent?.trim() === 'Collab Agents')`)
  await page.waitForExpression('compact Collab Agents empty page', `(() => {
    const page = document.querySelector('section[aria-label="Collab Agents"]');
    const create = page?.querySelector('button[aria-label="New Agent"]');
    if (!(page instanceof HTMLElement) || !(create instanceof HTMLButtonElement)) return false;
    const rect = create.getBoundingClientRect();
    return page.offsetParent !== null && !create.disabled
      && rect.width <= 32 && rect.height <= 32
      && page.textContent?.includes('No Agents yet') === false
      && ![...page.querySelectorAll('button')].some(button => button.textContent?.trim() === 'New Agent');
  })()`)
  await page.clickExpression('inline New Agent', `document.querySelector('section[aria-label="Collab Agents"] button[aria-label="New Agent"]')`)
  await page.waitForExpression('Settings uses inline single-screen Agent creation', `(() => {
    const page = document.querySelector('section[aria-label="Collab Agents"]');
    const block = page?.querySelector('[data-agent-create-inline]');
    const form = block?.querySelector('[data-variant="inline"]');
    const dialogs = [...document.querySelectorAll('[role="dialog"]')]
      .filter(dialog => dialog instanceof HTMLElement && dialog.offsetParent !== null);
    const tabs = [...form?.querySelectorAll('[role="tab"]') ?? []];
    const header = form?.querySelector('header');
    const firstInput = form?.querySelector('#chaos-agent-create-name');
    const pills = [...form?.querySelectorAll('button[aria-haspopup="listbox"]') ?? []];
    const textarea = form?.querySelector('#chaos-agent-create-charter');
    const actions = [...form?.querySelectorAll('footer button') ?? []];
    if (!(block instanceof HTMLElement) || !(form instanceof HTMLElement)
      || !(header instanceof HTMLElement)
      || !(firstInput instanceof HTMLInputElement)) return false;
    const blockRect = block.getBoundingClientRect();
    const headerRect = header.getBoundingClientRect();
    return form.offsetParent !== null
      && dialogs.length === 1
      && tabs.length === 0
      && pills.length === 3
      && header.querySelector('h2')?.textContent?.trim() === 'New Agent'
      && Math.abs(headerRect.left - blockRect.left) <= 2
      && Math.abs(headerRect.right - blockRect.right) <= 2
      && textarea instanceof HTMLTextAreaElement
      && actions.length === 2
      && form.textContent?.includes('Handle') === false;
  })()`)
  await page.screenshot('agent-settings-inline.png')
  await page.setViewport(650, 800)
  await page.waitForExpression('inline Agent creation fits narrow Settings viewport', `(() => {
    const block = document.querySelector('[data-agent-create-inline]');
    if (!(block instanceof HTMLElement) || block.offsetParent === null) return false;
    const rect = block.getBoundingClientRect();
    return rect.left >= 0 && rect.right <= 650 && document.documentElement.scrollWidth <= 650;
  })()`)
  await page.screenshot('agent-settings-inline-narrow.png')
  await page.setViewport(1200, 800)

  // Complete a real Agent through the single-screen inline form, then open
  // the detail header's More menu to lock its open state and capture it.
  await page.fill('#chaos-agent-create-name', 'Frontend review')
  await page.fill('#chaos-agent-create-charter', 'Reviews frontend changes.')
  await page.clickExpression('Provider pill', `(() => {
    const form = document.querySelector('[data-agent-create-inline]');
    return [...form?.querySelectorAll('button[aria-haspopup="listbox"]') ?? []][0];
  })()`)
  await page.waitForExpression('Provider menu open (portal, unclipped)', `(() => {
    const menu = [...document.querySelectorAll('[role="menu"]')]
      .find(candidate => candidate instanceof HTMLElement && getComputedStyle(candidate).position === 'fixed');
    if (menu === undefined) return false;
    const rect = menu.getBoundingClientRect();
    const form = document.querySelector('[data-agent-create-inline]');
    const block = form === null ? null : form.closest('[data-agent-create-inline], section');
    const blockRect = block?.getBoundingClientRect();
    return menu.textContent?.includes('DeepSeek') === true
      && rect.height > 0
      && (blockRect === undefined || (rect.right <= blockRect.right && rect.bottom <= window.innerHeight));
  })()`)
  await page.clickExpression('First provider option', `(() => {
    const menu = [...document.querySelectorAll('[role="menu"]')]
      .find(candidate => candidate instanceof HTMLElement && getComputedStyle(candidate).position === 'fixed');
    return [...menu?.querySelectorAll('[role="menuitem"]') ?? []][0];
  })()`)
  await page.clickExpression('Model pill', `(() => {
    const form = document.querySelector('[data-agent-create-inline]');
    return [...form?.querySelectorAll('button[aria-haspopup="listbox"]') ?? []][1];
  })()`)
  await page.waitForExpression('Model menu open (portal, unclipped)', `(() => {
    const menu = [...document.querySelectorAll('[role="menu"]')]
      .find(candidate => candidate instanceof HTMLElement && getComputedStyle(candidate).position === 'fixed');
    if (menu === undefined) return false;
    const rect = menu.getBoundingClientRect();
    return (menu.querySelectorAll('[role="menuitem"]').length ?? 0) > 0
      && rect.height > 0
      && rect.bottom <= window.innerHeight;
  })()`)
  await page.clickExpression('First model option', `(() => {
    const menu = [...document.querySelectorAll('[role="menu"]')]
      .find(candidate => candidate instanceof HTMLElement && getComputedStyle(candidate).position === 'fixed');
    return [...menu?.querySelectorAll('[role="menuitem"]') ?? []][0];
  })()`)
  await page.clickExpression('Create Agent submit', `[...document.querySelectorAll('button')]
    .find(button => button.offsetParent !== null && button.textContent?.trim() === 'Create Agent' && !button.disabled)`)
  await page.waitForExpression('created Agent row in list', `(() => {
    const block = document.querySelector('[data-agent-create-inline]');
    return block === null && document.body.innerText.includes('Frontend review');
  })()`)
  await page.screenshot('agent-created-list.png')
  await page.clickExpression('open Agent detail from row', `(() => {
    const page = document.querySelector('section[aria-label="Collab Agents"]');
    return [...page?.querySelectorAll('button, [role="button"]') ?? []]
      .find(row => row.textContent?.includes('Frontend review'));
  })()`)
  await page.waitForExpression('Agent detail header', `(() => {
    const detail = document.querySelector('section[id^="chaos-agent-"][id$="-detail"]');
    return detail instanceof HTMLElement && detail.offsetParent !== null
      && detail.textContent?.includes('Frontend review');
  })()`)
  await page.clickExpression('Runtime tab', `(() => {
    const detail = document.querySelector('section[id^="chaos-agent-"][id$="-detail"]');
    return [...detail?.querySelectorAll('[role="tab"]') ?? []].find(tab => tab.textContent?.trim() === 'Runtime');
  })()`)
  await page.waitForExpression('runtime tab panel', `(() => {
    const panel = document.querySelector('[role="tabpanel"]');
    const group = document.querySelector('[role="group"][aria-label="Runtime"]');
    return panel instanceof HTMLElement && group instanceof HTMLElement;
  })()`)
  await page.screenshot('agent-detail-runtime-tab.png')
  await page.waitForExpression('runtime pills separated with visible edge', `(() => {
    const group = document.querySelector('[role="group"][aria-label="Runtime"]');
    if (!(group instanceof HTMLElement)) return false;
    const cs = getComputedStyle(group);
    const pills = [...group.querySelectorAll('button')].map(btn => btn.getBoundingClientRect());
    return parseFloat(cs.paddingTop) >= 8
      && parseFloat(cs.borderWidth) >= 1
      && pills.length === 3
      && pills[1].left - pills[0].right >= 8
      && pills[2].left - pills[1].right >= 8;
  })()`)
  await page.screenshot('agent-detail-before-menu.png')
  await page.waitForExpression('Agent detail header carries no More menu and Delete lives in the footer', `(() => {
    const detail = document.querySelector('section[id^="chaos-agent-"][id$="-detail"]');
    if (!(detail instanceof HTMLElement)) return false;
    const header = detail.querySelector('header');
    const menus = header?.querySelectorAll('[role="menu"], button[aria-haspopup="menu"]').length ?? 0;
    const footerButtons = [...detail.querySelectorAll('footer button')]
      .map(button => button.textContent?.trim());
    return menus === 0 && footerButtons.includes('Delete Agent');
  })()`)
  await page.clickExpression('Identity tab', `(() => {
    const detail = document.querySelector('section[id^="chaos-agent-"][id$="-detail"]');
    return [...detail?.querySelectorAll('[role="tab"]') ?? []].find(tab => tab.textContent?.trim() === 'Identity');
  })()`)
  await page.waitForExpression('identity footer also owns Delete', `(() => {
    const panel = document.querySelector('[role="tabpanel"]');
    const buttons = [...panel?.querySelectorAll('footer button') ?? []]
      .map(button => button.textContent?.trim());
    return buttons.includes('Delete Agent') && buttons.some(label => (label ?? '').startsWith('Save'));
  })()`)
  await page.screenshot('agent-detail-footer.png')

  assert(page.consoleErrors.length === 0, `console errors:\n${page.consoleErrors.join('\n')}`)
  assert(page.pageErrors.length === 0, `page errors:\n${page.pageErrors.join('\n')}`)
  assert(page.requestFailures.length === 0, `request failures:\n${page.requestFailures.join('\n')}`)

  await writeFile(join(artifactRoot, 'result.json'), `${JSON.stringify({
    status: 'passed',
    dshVersion,
    channelName,
    assertions: [
      'plugin loaded in an isolated official DSH profile',
      'New Channel name uses the shared blue focus field',
      'Channel creation completed through the real UI and RPC path',
      'Channel Description edit completed through the real UI and RPC path',
      'Archived Channel stayed readable without a composer and restored successfully',
      'Message send completed through the real UI and RPC path',
      'Channel and Message persisted across a full page reload',
      'Channel remained usable at a 650x800 viewport',
      'Settings kept Agent creation inline on one screen with pill selectors',
      'Settings Agent creation remained usable at a 650x800 viewport',
      'no console, page, or request failures were observed',
    ],
  }, null, 2)}\n`)
  console.log(`E2E passed (${dshVersion}); artifacts: ${artifactRoot}`)
} catch (error) {
  failure = error
  if (page !== undefined) {
    try { await page.screenshot('failure.png') } catch {}
  }
  await writeFile(join(artifactRoot, 'failure.txt'), [
    String(error?.stack ?? error),
    '',
    'DSH process:',
    ...(dshProcess?.lines ?? []),
    '',
    'Browser process:',
    ...(browserProcess?.lines ?? []),
  ].join('\n'))
} finally {
  page?.close()
  await terminate(browserProcess)
  await terminate(dshProcess)
  if (!keepWorkdir) await rm(workRoot, { recursive: true, force: true })
  else console.log(`kept isolated work directory: ${workRoot}`)
}

if (failure !== undefined) throw failure
