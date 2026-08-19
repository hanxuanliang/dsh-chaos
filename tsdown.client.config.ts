import { readFile } from 'node:fs/promises'
import { basename, dirname, resolve } from 'node:path'
import { transform } from 'lightningcss'
import type { UserConfig } from 'tsdown'

const id = 'dsh-chaos'
const CSS_VIRTUAL_PREFIX = '\0dsh-chaos-css:'
const CSS_VIRTUAL_SUFFIX = '.mjs'

export default {
  name: `${id}/client`,
  entry: { client: 'src/client/index.tsx' },
  outDir: 'lib',
  format: 'cjs',
  platform: 'browser',
  target: 'es2024',
  alias: {
    // vfile (via react-markdown) re-exports these unconditionally; the DSH
    // client module table has no node builtins, so alias them to a browser
    // shim (src/client/shims/node-min.ts).
    'node:path': resolve(import.meta.dirname, 'src/client/shims/node-min.ts'),
    'node:process': resolve(import.meta.dirname, 'src/client/shims/node-min.ts'),
    'node:url': resolve(import.meta.dirname, 'src/client/shims/node-min.ts'),
  },
  dts: false,
  sourcemap: true,
  clean: false,
  deps: {
    neverBundle: [
      'react',
      'react/jsx-runtime',
      'react-dom',
      '@deepseek-ai/cordis',
      '@deepseek-ai/dsh-client-ui-primitives',
      '@deepseek-ai/dsh-client-ui-slots',
    ],
    // DSH client's module table only hosts the neverBundle externals above;
    // anything in package.json dependencies defaults to externalized and
    // would brick the whole plugin import at runtime (observed 2026-08-19:
    // require("react-markdown") missed the module table). Force-inline.
    alwaysBundle: [
      /^react-markdown($|\/)/,
      /^remark-gfm($|\/)/,
      /^remark-breaks($|\/)/,
    ],
  },
  plugins: [{
    // Match DSH's own client-bundle contract: CSS Modules live inside the
    // plugin factory and install one loader-owned style tag at materialization.
    name: 'dsh-chaos-css-modules-inline',
    resolveId(source: string, importer: string | undefined) {
      if (!source.endsWith('.module.css')) return null
      const path = importer === undefined ? source : resolve(dirname(importer), source)
      return CSS_VIRTUAL_PREFIX + path + CSS_VIRTUAL_SUFFIX
    },
    async load(virtualId: string) {
      if (!virtualId.startsWith(CSS_VIRTUAL_PREFIX)) return null
      const path = virtualId.slice(CSS_VIRTUAL_PREFIX.length, -CSS_VIRTUAL_SUFFIX.length)
      this.addWatchFile(path)
      const source = await readFile(path)
      const { code, exports: cssExports } = transform({
        filename: path,
        code: source,
        cssModules: { pattern: '[hash]_[local]' },
        minify: true,
      })
      const classes: Record<string, string> = {}
      for (const [local, value] of Object.entries(cssExports ?? {})) classes[local] = value.name
      return [
        `const css = ${JSON.stringify(code.toString())};`,
        `const tagId = ${JSON.stringify(`${id}/${basename(path)}`)};`,
        'if (typeof document !== \'undefined\' && document.querySelector(\'style[data-plugin-css=\' + JSON.stringify(tagId) + \']\') === null) {',
        '  const tag = document.createElement(\'style\');',
        `  tag.dataset.plugin = ${JSON.stringify(id)};`,
        '  tag.dataset.pluginCss = tagId;',
        '  tag.textContent = css;',
        '  document.head.appendChild(tag);',
        '}',
        `export default ${JSON.stringify(classes)};`,
      ].join('\n')
    },
  }],
  outputOptions: {
    entryFileNames: 'client.js',
    banner: `window.__ModuleLoader__.load({ id: ${JSON.stringify(id)}, factory: (require) => {`,
    footer: 'return module.exports; } });',
    intro: 'var module = { exports: {} }; var exports = module.exports;',
  },
} satisfies UserConfig
