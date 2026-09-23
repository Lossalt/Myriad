#!/usr/bin/env node
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { basename, dirname, join, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
/**
 * First-paint JS/CSS budget for the home shell.
 * Counts compressed (gzip) bytes of assets referenced by index.html plus
 * statically imported chunks, including the lazy Home route. Agora / Config must stay out of
 * that set.
 */
import { promisify } from 'node:util'
import { gzip } from 'node:zlib'

const gzipAsync = promisify(gzip)
const here = dirname(fileURLToPath(import.meta.url))
const frontendRoot = resolve(here, '..')
const distDir = resolve(frontendRoot, 'dist')
const baselinePath = resolve(here, 'home-budget.baseline.json')

const SLACK = 0.15
// A new first-paint file costs a request before the home screen can render.
const FILE_SLACK = 2

/**
 * Chunks that load on demand. Each was measured out of first paint; a static
 * import that drags one back fails the budget.
 */
export const LAZY_ONLY_CHUNKS = [
  'Config',
  'motion-vendor',
  'motion-dom',
  'surfaceLenses',
  'NotificationPanelList',
  'AgentGlobalActions',
]

// Rolldown names chunks `<name>-<hash>.<ext>`; the hash may itself contain '-'.
function isChunk(file, name) {
  return new RegExp(`^${name.replaceAll(/[.*+?^${}()|[\]\\]/g, '\\$&')}-[\\w-]+\\.(?:js|css)$`).test(basename(file))
}

function findIndexHtml(root = distDir) {
  const candidates = [
    join(root, 'index.html'),
    join(root, 'client', 'index.html'),
  ]
  return candidates.find((path) => existsSync(path)) ?? null
}

function collectReferencedAssets(html, htmlDir) {
  const assets = new Set()
  const pattern = /(?:src|href)=["']([^"']+\.(?:js|css))["']/g
  for (const match of html.matchAll(pattern)) {
    const href = match[1]
    if (href.startsWith('http') || href.startsWith('data:')) continue
    const abs = resolve(htmlDir, href.replaceAll(/^\//g, ''))
    if (existsSync(abs)) assets.add(abs)
  }
  return assets
}

function displayPath(file, root) {
  if (file.startsWith(root)) return file.slice(root.length + 1)
  if (file.startsWith(frontendRoot)) return file.slice(frontendRoot.length + 1)
  return file
}

function walkStaticImports(entryFiles) {
  const seen = new Set(entryFiles)
  const queue = Iterator.from(entryFiles).toArray()
  const importRe =
    /(?:from|import)\s*["'](\.{0,2}\/[^"']+\.js)["']|import\(["'](\.{0,2}\/[^"']+\.js)["']\)/g
  while (queue.length > 0) {
    const file = queue.pop()
    if (!file.endsWith('.js') || !existsSync(file)) continue
    if (/(?:^|\/)Home-[^/]+\.js$/.test(file)) {
      for (const name of readdirSync(dirname(file))) {
        if (/^Home-[^/]+\.css$/.test(name)) seen.add(resolve(dirname(file), name))
      }
    }
    const source = readFileSync(file, 'utf8')
    for (const match of source.matchAll(importRe)) {
      const spec = match[1] || match[2]
      if (!spec) continue
      // Home is lazy at the router boundary but required for the home screen.
      const isDynamic = match[0].startsWith('import(')
      if (isDynamic && !/(?:^|\/)Home-[^/]+\.js$/.test(spec)) continue
      const next = resolve(dirname(file), spec)
      if (!seen.has(next) && existsSync(next)) {
        seen.add(next)
        queue.push(next)
      }
    }
  }
  return seen
}

async function gzipSize(path) {
  const buf = await gzipAsync(readFileSync(path))
  return buf.byteLength
}

export async function measureHomeBudget(root = distDir) {
  const htmlPath = findIndexHtml(root)
  if (!htmlPath) {
    throw new Error(`home budget: no index.html under ${root}`)
  }
  const html = readFileSync(htmlPath, 'utf8')
  const htmlDir = dirname(htmlPath)
  const referenced = collectReferencedAssets(html, htmlDir)
  const firstPaint = walkStaticImports(Iterator.from(referenced).toArray())

  let js = 0
  let css = 0
  const files = []
  for (const file of firstPaint) {
    const size = await gzipSize(file)
    files.push({ file: displayPath(file, root), gzip: size })
    if (file.endsWith('.css')) css += size
    else js += size
  }
  const ranked = files.toSorted((a, b) => b.gzip - a.gzip)

  const blob = Iterator.from(firstPaint)
    .filter((file) => file.endsWith('.js'))
    .map((file) => readFileSync(file, 'utf8'))
    .toArray()
    .join('\n')

  const lazyOnlyChunks = LAZY_ONLY_CHUNKS.filter(name =>
    Iterator.from(firstPaint).some(file => isChunk(file, name)),
  )

  return {
    jsGzipBytes: js,
    cssGzipBytes: css,
    totalGzipBytes: js + css,
    firstPaintFiles: firstPaint.size,
    lazyOnlyChunks,
    files: ranked,
    loadsAgora: /agora-rtc-sdk-ng|agora-rtm/.test(blob),
    // Filename, not a lazy-import string left inside App.
    loadsConfigRoute: lazyOnlyChunks.includes('Config'),
  }
}

export async function measureVoiceBudget(root = distDir) {
  const html = findIndexHtml(root)
  if (!html) throw new Error('voice budget: missing production build')
  const assetsDir = join(dirname(html), 'assets')
  const entries = readdirSync(assetsDir)
    .filter(name => /^(?:AgoraRTC_N-production|agora-rtm)-.*\.js$/.test(name))
    .map(name => join(assetsDir, name))
  if (entries.length !== 2) throw new Error('voice budget: expected both Agora SDK chunks')
  const files = await Promise.all([...walkStaticImports(entries)].map(async file => ({
    file: displayPath(file, root), gzip: await gzipSize(file),
  })))
  return { totalGzipBytes: files.reduce((sum, file) => sum + file.gzip, 0), files }
}

function loadBaseline() {
  return JSON.parse(readFileSync(baselinePath, 'utf8'))
}

function withinSlack(actual, baseline) {
  return actual <= Math.ceil(baseline * (1 + SLACK))
}

async function main() {
  const write = process.argv.includes('--write')
  const check = process.argv.includes('--check') || !write
  if (!existsSync(distDir)) {
    console.error('home budget: dist/ missing; run pnpm build first')
    process.exit(2)
  }
  const measured = await measureHomeBudget()
  const voice = await measureVoiceBudget()
  if (write) {
    writeFileSync(
      baselinePath,
      `${JSON.stringify(
        {
          jsGzipBytes: measured.jsGzipBytes,
          cssGzipBytes: measured.cssGzipBytes,
          totalGzipBytes: measured.totalGzipBytes,
          firstPaintFiles: measured.firstPaintFiles,
        },
        null,
        2,
      )}\n`,
    )
    console.log(`wrote ${baselinePath}`)
  }
  console.log(
    JSON.stringify(
      {
        jsGzipBytes: measured.jsGzipBytes,
        cssGzipBytes: measured.cssGzipBytes,
        totalGzipBytes: measured.totalGzipBytes,
        firstPaintFiles: measured.firstPaintFiles,
        lazyOnlyChunks: measured.lazyOnlyChunks,
        voiceGzipBytes: voice.totalGzipBytes,
        loadsAgora: measured.loadsAgora,
        loadsConfigRoute: measured.loadsConfigRoute,
        top: measured.files.slice(0, 8),
      },
      null,
      2,
    ),
  )
  if (check && existsSync(baselinePath)) {
    const baseline = loadBaseline()
    const failures = []
    if (!withinSlack(measured.jsGzipBytes, baseline.jsGzipBytes)) {
      failures.push(
        `JS gzip ${measured.jsGzipBytes} exceeds baseline ${baseline.jsGzipBytes} +15%`,
      )
    }
    if (!withinSlack(measured.cssGzipBytes, baseline.cssGzipBytes)) {
      failures.push(
        `CSS gzip ${measured.cssGzipBytes} exceeds baseline ${baseline.cssGzipBytes} +15%`,
      )
    }
    if (baseline.firstPaintFiles && measured.firstPaintFiles > baseline.firstPaintFiles + FILE_SLACK) {
      failures.push(
        `first-paint files ${measured.firstPaintFiles} exceed baseline ${baseline.firstPaintFiles} +${FILE_SLACK}`,
      )
    }
    if (voice.totalGzipBytes > 1_000_000) {
      failures.push(`Voice SDK gzip ${voice.totalGzipBytes} exceeds 1000000 bytes`)
    }
    if (measured.loadsAgora) {
      failures.push('first-paint JS contains Agora SDK')
    }
    for (const name of measured.lazyOnlyChunks) {
      failures.push(`first-paint graph statically imports the on-demand ${name} chunk`)
    }
    if (failures.length > 0) {
      console.error(failures.join('\n'))
      process.exit(1)
    }
  }
}

const invoked = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (invoked) {
  main().catch((error) => {
    console.error(error)
    process.exit(1)
  })
}
