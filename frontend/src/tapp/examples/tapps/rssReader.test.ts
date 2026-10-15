import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'
import { afterEach, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

interface TappStub {
  permissions: string[]
  i18n: {
    t: (key: string, variables?: Record<string, unknown>) => string
    getLocale: () => string
  }
  lifecycle: { getInfo: () => { permissions: string[]; sandboxed: true } }
  ui: {
    setTitle: (title: string) => Promise<void>
    onLocaleChange: (callback: () => void) => void
  }
  file: { download: (...args: unknown[]) => Promise<void> }
  storage?: {
    get: (key: string) => Promise<unknown>
    set: (key: string, value: unknown) => Promise<void>
  }
  phantasiList: {
    sources: () => Promise<unknown[]>
    categories: () => Promise<unknown[]>
    stats: () => Promise<{ total_unread: number }>
    list: (options?: unknown) => Promise<{ items: Array<{ id: number; title?: string }>; total?: number; next_cursor: string | null }>
    openOriginal: (id: number) => Promise<unknown>
  }
}

const host = globalThis as typeof globalThis & { Tapp?: TappStub }

const require = createRequire(import.meta.url)
const pageDir = join(
  dirname(fileURLToPath(import.meta.url)),
  '../../../../../tapps/rss-reader/page',
)

interface FakeNode {
  tag: string
  className: string
  textContent: string
  hidden: boolean
  children: FakeNode[]
  parent: FakeNode | null
  attrs: Record<string, string>
  classList: { toggle: () => void }
  append: (...nodes: FakeNode[]) => void
  appendChild: (node: FakeNode) => FakeNode
  replaceChildren: (...nodes: FakeNode[]) => void
  setAttribute: (name: string, value: string) => void
  getAttribute: (name: string) => string | null
  querySelector: (selector: string) => FakeNode | null
  querySelectorAll: (selector: string) => FakeNode[]
  closest: (selector: string) => FakeNode | null
  contains: (node: FakeNode) => boolean
  addEventListener: () => void
}

function matches(node: FakeNode, selector: string): boolean {
  if (selector.startsWith('[data-role="') && selector.endsWith('"]')) {
    return node.attrs['data-role'] === selector.slice(12, -2)
  }
  if (selector === '[data-i18n]') return 'data-i18n' in node.attrs
  if (selector === '[data-i18n-placeholder]') return 'data-i18n-placeholder' in node.attrs
  if (selector === '[data-action="toggle-manage"]') {
    return node.attrs['data-action'] === 'toggle-manage'
  }
  if (selector === 'label') return node.tag === 'label'
  return false
}

function walk(node: FakeNode, visit: (node: FakeNode) => void) {
  visit(node)
  for (const child of node.children) walk(child, visit)
}

function createNode(tag: string): FakeNode {
  const node = {
    tag,
    className: '',
    textContent: '',
    hidden: false,
    children: [],
    parent: null,
    attrs: {},
    classList: { toggle() {} },
    append(...nodes: FakeNode[]) {
      for (const child of nodes) {
        child.parent = node
        node.children.push(child)
      }
    },
    appendChild(child: FakeNode) {
      child.parent = node
      node.children.push(child)
      return child
    },
    replaceChildren(...nodes: FakeNode[]) {
      node.children = []
      node.append(...nodes)
    },
    setAttribute(name: string, value: string) {
      node.attrs[name] = String(value)
      if (name === 'hidden') node.hidden = true
    },
    getAttribute(name: string) {
      return name in node.attrs ? node.attrs[name] : null
    },
    querySelector(selector: string) {
      let found: FakeNode | null = null
      walk(node, (current) => {
        if (!found && current !== node && matches(current, selector)) found = current
      })
      return found
    },
    querySelectorAll(selector: string) {
      const found: FakeNode[] = []
      walk(node, (current) => {
        if (current !== node && matches(current, selector)) found.push(current)
      })
      return found
    },
    closest() {
      return null
    },
    contains(target: FakeNode) {
      let found = false
      walk(node, (current) => {
        if (current === target) found = true
      })
      return found
    },
    addEventListener() {},
  } satisfies FakeNode
  return node
}

function installDocument() {
  const root = createNode('div')
  root.attrs.id = 'rss-app'
  globalThis.document = {
    getElementById: (id: string) => (id === 'rss-app' ? root : null),
    createElement: (tag: string) => createNode(tag),
    createTextNode: (text: string) => {
      const node = createNode('#text')
      node.textContent = text
      return node
    },
    documentElement: { lang: '' },
  } as unknown as Document
  return root
}

function installTapp(permissions: string[]) {
  const opened: number[] = []
  host.Tapp = {
    permissions,
    i18n: {
      t: (key: string) => key,
      getLocale: () => 'zh-CN',
    },
    lifecycle: {
      getInfo: () => ({ permissions, sandboxed: true }),
    },
    ui: {
      setTitle: async () => {},
      onLocaleChange: () => {},
    },
    file: { download: async () => {} },
    storage: {
      get: async () => ({}),
      set: async () => {},
    },
    phantasiList: {
      sources: async () => [],
      categories: async () => [],
      stats: async () => ({ total_unread: 0 }),
      list: async () => ({ items: [], next_cursor: null }),
      openOriginal: async (id: number) => {
        opened.push(id)
      },
    },
  }
  return opened
}

afterEach(() => {
  delete host.Tapp
})

describe('rss reader sandbox page', () => {
  it('hides source management unless phantasi:manage is granted', async () => {
    installDocument()
    installTapp(['phantasi:read', 'phantasi:write'])
    const { start } = require(join(pageDir, 'app.js')) as {
      start: (root: FakeNode) => void
    }
    const root = document.getElementById('rss-app') as unknown as FakeNode
    start(root)
    await new Promise((resolve) => setTimeout(resolve, 0))
    assert.equal(root.querySelector('[data-action="toggle-manage"]'), null)

    installTapp(['phantasi:read', 'phantasi:write', 'phantasi:manage'])
    const managed = document.getElementById('rss-app') as unknown as FakeNode
    managed.children = []
    start(managed)
    await new Promise((resolve) => setTimeout(resolve, 0))
    assert.ok(managed.querySelector('[data-action="toggle-manage"]'))
  })

  it('opens on unread and restores a stored filter', async () => {
    installDocument()
    installTapp(['phantasi:read'])
    const { start } = require(join(pageDir, 'app.js')) as {
      start: (root: FakeNode) => void
    }
    const root = document.getElementById('rss-app') as unknown as FakeNode
    start(root)
    await new Promise((resolve) => setTimeout(resolve, 0))
    assert.equal(root.querySelector('[data-role="stream-title"]')?.textContent, 'filterUnread')

    installTapp(['phantasi:read', 'storage:read', 'storage:write'])
    assert.ok(host.Tapp)
    host.Tapp.storage = {
      get: async () => ({ filter: 'starred', category: '', sourceId: null }),
      set: async () => {},
    }
    const restored = document.getElementById('rss-app') as unknown as FakeNode
    restored.children = []
    start(restored)
    await new Promise((resolve) => setTimeout(resolve, 0))
    assert.equal(restored.querySelector('[data-role="stream-title"]')?.textContent, 'filterStarred')
  })

  it('asks the host to open an article by id', async () => {
    const opened = installTapp(['phantasi:read'])
    const api = require(join(pageDir, 'api.js')) as {
      openOriginal: (id: number) => Promise<unknown>
    }
    await api.openOriginal(9)
    assert.deepEqual(opened, [9])
  })

  it('returns list data from the unwrapped host payload', async () => {
    installTapp(['phantasi:read'])
    assert.ok(host.Tapp)
    host.Tapp.phantasiList.list = async () => ({
      items: [{ id: 3, title: 'Hello' }],
      next_cursor: null,
      total: 1,
    })
    const api = require(join(pageDir, 'api.js')) as {
      list: () => Promise<{ items: Array<{ id: number }> }>
    }
    const page = await api.list()
    assert.equal(page.items[0]?.id, 3)
  })
})
