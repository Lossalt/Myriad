import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { DOCUMENT_PERMISSIONS_POLICY } from '../scripts/vite/constants.mjs'
import { spaFallbackPlugin } from '../scripts/vite/spaFallback.mjs'

describe('SPA document middleware', () => {
  it('leaves routing and queries to Vite and React Router in dev and preview', () => {
    const plugin = spaFallbackPlugin()
    for (const attach of [
      plugin.configureServer,
      plugin.configurePreviewServer,
    ]) {
      let handler: any
      attach({
        config: { root: '/tmp', publicDir: '/tmp', build: { outDir: 'dist' } },
        middlewares: {
          use(fn: any) {
            handler = fn
          },
        },
      })
      for (const url of [
        '/tapp/run/abc?x=1',
        '/studio/notes?keep=1',
        '/agent/settings',
        '/api/config/ui',
      ]) {
        const req = { url }
        const headers = new Map()
        let next = false
        handler(
          req,
          {
            getHeader: (key: string) => headers.get(key),
            setHeader: (key: string, value: string) => headers.set(key, value),
          },
          () => {
            next = true
          },
        )
        assert.equal(req.url, url)
        assert.equal(
          headers.get('Permissions-Policy'),
          DOCUMENT_PERMISSIONS_POLICY,
        )
        assert.equal(next, true)
      }
    }
    assert.match(
      readFileSync(new URL('../vite.config.mjs', import.meta.url), 'utf8'),
      /appType: 'spa'/,
    )
  })
})
