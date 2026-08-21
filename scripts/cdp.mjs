#!/usr/bin/env node
// Minimal CDP driver for DSH e2e checks (no deps; Node >= 22 native WebSocket).
// Usage: node cdp.mjs <command> [args...]
//   open <url>            — navigate first tab
//   shot <file>           — screenshot to file
//   eval <js>             — Runtime.evaluate and print JSON
//   click <text>          — click first element whose textContent trim equals <text> (button/role=tab/[data-*] scan)
//   clicksub <text>       — click first element whose textContent includes <text>
//   sleep <ms>
//   html <file>           — dump document.body.innerHTML
//   console               — print collected console entries
import { writeFileSync } from 'node:fs'

const DEBUG = process.env.CDP_DEBUG ?? 'http://127.0.0.1:9222'

async function targetWs() {
  const res = await fetch(`${DEBUG}/json`)
  const list = await res.json()
  const page = list.find(t => t.type === 'page')
  if (!page) throw new Error('no page target; list=' + JSON.stringify(list.map(t => t.type)))
  return page.webSocketDebuggerUrl
}

const ws = new WebSocket(await targetWs())
await new Promise((resolve, reject) => {
  ws.onopen = resolve
  ws.onerror = reject
})

let seq = 0
const pending = new Map()
const consoleEntries = []
ws.onmessage = (event) => {
  const msg = JSON.parse(event.data)
  if (msg.id !== undefined && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id)
    pending.delete(msg.id)
    if (msg.error) reject(new Error(msg.error.message))
    else resolve(msg.result)
  } else if (msg.method === 'Runtime.consoleAPICalled') {
    const args = (msg.params.args ?? []).map(a => a.value ?? a.description ?? a.type).join(' ')
    consoleEntries.push(`[${msg.params.type}] ${args}`.slice(0, 500))
  } else if (msg.method === 'Runtime.exceptionThrown') {
    const d = msg.params.exceptionDetails
    consoleEntries.push(`[exception] ${d.text} ${d.exception?.description ?? ''}`.slice(0, 800))
  }
}

function send(method, params = {}) {
  const id = ++seq
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject })
    ws.send(JSON.stringify({ id, method, params }))
  })
}

async function evaluate(expression) {
  const result = await send('Runtime.evaluate', {
    expression,
    awaitPromise: true,
    returnByValue: true,
  })
  if (result.exceptionDetails) {
    throw new Error('page threw: ' + (result.exceptionDetails.exception?.description ?? result.exceptionDetails.text))
  }
  return result.result.value
}

const CLICK_JS = (matchMode, text) => `(() => {
  const needle = ${JSON.stringify(text)};
  const match = (el) => {
    const label = (el.textContent ?? '').trim();
    return ${matchMode === 'exact' ? 'label === needle' : 'label.includes(needle)'};
  };
  const els = [...document.querySelectorAll('button, [role="tab"], [role="button"], [role="menuitem"], a, [data-plugin] *')];
  const hit = els.find(match);
  if (!hit) return { ok: false, tried: els.slice(0, 40).map(e => (e.textContent ?? '').trim().slice(0, 40)) };
  hit.scrollIntoView({ block: 'center' });
  hit.click();
  return { ok: true, tag: hit.tagName, label: (hit.textContent ?? '').trim().slice(0, 60) };
})()`

const [cmd, ...rest] = process.argv.slice(2)
try {
  await send('Runtime.enable')
  switch (cmd) {
    case 'open': {
      await send('Page.enable')
      await send('Page.navigate', { url: rest[0] ?? 'http://127.0.0.1:3081/settings' })
      break
    }
    case 'sleep':
      await new Promise(r => setTimeout(r, Number(rest[0])))
      break
    case 'shot': {
      await send('Page.enable')
      const { data } = await send('Page.captureScreenshot', { format: 'png' })
      writeFileSync(rest[0], Buffer.from(data, 'base64'))
      break
    }
    case 'eval':
      console.log(JSON.stringify(await evaluate(rest.join(' ')), null, 2))
      break
    case 'click':
      console.log(JSON.stringify(await evaluate(CLICK_JS('exact', rest.join(' '))), null, 2))
      break
    case 'clicksub':
      console.log(JSON.stringify(await evaluate(CLICK_JS('sub', rest.join(' '))), null, 2))
      break
    case 'html': {
      const html = await evaluate('document.documentElement.outerHTML')
      writeFileSync(rest[0], html)
      break
    }
    case 'console':
      console.log(consoleEntries.join('\n') || '(no console entries)')
      break
    case 'dragseq': {
      const [x1,y1,x2,y2] = rest.map(Number)
      const points = [[x1,y1],[x1+8,y1+2],[x1+20,y1+6],[x1+42,y1+12],[(x1+x2)/2,(y1+y2)/2],[x2-30,y2-10],[x2-8,y2-2],[x2,y2]]
      await send('Input.dispatchDragEvent', { type:'dragStart', x:x1, y:y1, data: { items: [], dragOperationsMask: 1 } })
      await send('Input.dispatchDragEvent', { type:'dragEnter', x:x1, y:y1, data: { items: [], dragOperationsMask: 1 } })
      for (const [px,py] of points.slice(1)) {
        await send('Input.dispatchDragEvent', { type:'dragOver', x:px, y:py, data: { items: [], dragOperationsMask: 1 } })
        await new Promise(r => setTimeout(r, 60))
      }
      await send('Input.dispatchDragEvent', { type:'drop', x:x2, y:y2, data: { items: [], dragOperationsMask: 1 } })
      break
    }
    case 'clickat': {
      const rx = Number(rest[0])
      const ry = Number(rest[1])
      await send('Input.dispatchMouseEvent', { type: 'mousePressed', x: rx, y: ry, button: 'left', clickCount: 1 })
      await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: rx, y: ry, button: 'left', clickCount: 1 })
      console.log('clickat', rx, ry)
      break
    }
    case 'mousemove': {
      await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: Number(rest[0]), y: Number(rest[1]) })
      break
    }
    case 'type': {
      // Real protocol-level text insertion (works with React controlled
      // inputs; synthetic input events get overwritten by re-renders).
      await send('Input.insertText', { text: rest.join(' ') })
      break
    }
    case 'keypress': {
      await send('Input.dispatchKeyEvent', { type: 'keyDown', key: rest[0], text: rest[0] === 'Enter' ? '\r' : undefined })
      await send('Input.dispatchKeyEvent', { type: 'keyUp', key: rest[0] })
      break
    }
    case 'rect': {
      const value = await evaluate(`(() => { const el = document.querySelector(${JSON.stringify(rest.join(' '))}); if (!el) return null; const r = el.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`)
      console.log(JSON.stringify(value))
      break
    }
    default:
      throw new Error('unknown command ' + cmd)
  }
} finally {
  ws.close()
}
