import type { TappInstance } from '../../../types'
import type { TappListInstallRequestInput } from '../../../utils/tappListInstallRequest'
import type { TappBridge } from '../../TappBridge'
import { getDefaultLocale } from '../../../../i18n'
import { userFacingError } from '../../../../utils/userFacingError'
import * as TappApiService from '../../../services/TappApiService'
import { resolveManifestText } from '../../../utils/manifestLocale'
import {
  resolveTappListInstallRequest,

} from '../../../utils/tappListInstallRequest'

function fail(error: unknown) {
  return {
    success: false,
    error: userFacingError(error),
  }
}

function getArgs(message: { payload: unknown }): unknown[] {
  return (message.payload as { args?: unknown[] }).args || []
}

/**
 * Lifecycle goes through the host runtime, never the raw API: uninstalling
 * must stop instances, drop widgets/platforms/caches and emit events so open
 * windows and background cores of that app are torn down.
 */
async function runtimeKnowing(tappId: string) {
  const { getTappRuntime } = await import('../../TappRuntime')
  const runtime = getTappRuntime()
  if (!runtime.getTapp(tappId)) await runtime.syncFromBackend(true)
  return runtime
}

async function syncRuntime() {
  const { getTappRuntime } = await import('../../TappRuntime')
  await getTappRuntime().syncFromBackend(true)
}

export function registerTappListHandlers(
  bridge: TappBridge,
  _tappInstance: TappInstance,
): void {
  bridge.registerHandler('tappList.list', async () => {
    try {
      const tapps = await TappApiService.listTapps()
      const locale = getDefaultLocale()
      return {
        success: true,
        data: tapps.map((t) => {
          const text = resolveManifestText(t, locale)
          return {
            id: t.id,
            name: text.name,
            version: t.version,
            description: text.description || '',
            icon: t.icon || '',
            iconSvg: t.iconSvg || '',
            status: t.status,
          }
        }),
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.get', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      const detail = await TappApiService.getTapp(tappId)
      const text = resolveManifestText(detail.manifest, getDefaultLocale())
      return {
        success: true,
        data: {
          id: detail.id,
          name: text.name,
          version: detail.manifest.version,
          description: text.description || '',
          icon: detail.icon || '',
          status: detail.status,
          installed_at: detail.installed_at,
          last_run_at: detail.last_run_at,
        },
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.getRecent', async (message) => {
    const [limit] = getArgs(message) as [number?]
    try {
      const items = await TappApiService.getRecentTapps(limit || 10)
      return { success: true, data: items }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.install', async (message) => {
    const [request] = getArgs(message) as [TappListInstallRequestInput]
    try {
      const resolved = resolveTappListInstallRequest(request)
      if (resolved.kind === 'error') {
        return { success: false, error: resolved.error }
      }
      if (resolved.kind === 'direct') {
        const result = await TappApiService.installDirect({
          manifest: resolved.manifest as Parameters<
            typeof TappApiService.installDirect
          >[0]['manifest'],
          modules: resolved.modules,
          coreStyles: resolved.coreStyles,
          pageStyles: resolved.pageStyles,
          widgetStyles: resolved.widgetStyles,
          pageTemplate: resolved.pageTemplate,
          widgetTemplates: resolved.widgetTemplates,
          widgetCss: resolved.widgetCss,
          pageCss: resolved.pageCss,
          i18n: resolved.i18n,
          assets: resolved.assets,
          permissions: resolved.permissions,
        })
        await syncRuntime()
        return {
          success: true,
          data: { id: result.id, name: result.name, status: result.status },
        }
      }
      const result = await TappApiService.installFromStore({
        source: resolved.catalogRef,
        tappId: resolved.tappId,
        permissions: resolved.permissions,
      })
      await syncRuntime()
      return {
        success: true,
        data: { id: result.id, name: result.name, status: result.status },
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.resolveStoreSource', async (message) => {
    const [tappId] = getArgs(message) as [string]
    if (!tappId || typeof tappId !== 'string') {
      return { success: false, error: 'tappId is required' }
    }
    try {
      const result = await TappApiService.resolveStoreSourceForTapp(tappId)
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.getInstallPackage', async (message) => {
    const [tappId, opts] = getArgs(message) as [
      string,
      { maxBytes?: number } | undefined,
    ]
    if (!tappId || typeof tappId !== 'string') {
      return { success: false, error: 'tappId is required' }
    }
    try {
      const result = await TappApiService.buildInstallPackageFromInstalled(
        tappId,
        { maxBytes: opts?.maxBytes },
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.uninstall', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      await (await runtimeKnowing(tappId)).uninstallTapp(tappId)
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.start', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      await (await runtimeKnowing(tappId)).startTapp(tappId)
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.stop', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      await (await runtimeKnowing(tappId)).stopTapp(tappId)
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.export', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      await TappApiService.exportTapp(tappId)
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })
}

export function registerPhantasiListHandlers(
  bridge: TappBridge,
  _tappInstance: TappInstance,
): void {
  bridge.registerHandler('phantasiList.list', async (message) => {
    const [options = {}] = getArgs(message) as [Record<string, unknown>?]
    try {
      const { getItemPreviews } = await import('../../../../services/phantasiApi')
      const data = await getItemPreviews(
        {
          per_page: (options.limit as number) || 30,
          page: (options.page as number) || 1,
          filter: (options.filter as 'all' | 'unread' | 'starred') || 'all',
          source_id: options.source_id as number | undefined,
        },
        await bridge.hostAttributionHeaders(),
      )
      return {
        success: true,
        data: {
          items: data.items.map((item) => ({
            id: item.id,
            title: item.title,
            link: item.link,
            summary: item.summary || '',
            image: item.image || '',
            author: item.author || '',
            source_name: item.source_name || '',
            source_icon: item.source_icon || '',
            published_at: item.published_at,
            is_read: item.is_read,
            is_starred: item.is_starred,
          })),
          total: data.total,
        },
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.get', async (message) => {
    const [id] = getArgs(message) as [number]
    try {
      const { getItem } = await import('../../../../services/phantasiApi')
      const item = await getItem(id, await bridge.hostAttributionHeaders())
      return {
        success: true,
        data: {
          id: item.id,
          title: item.title,
          link: item.link,
          summary: item.summary || '',
          content: item.content || '',
          image: item.image || '',
          author: item.author || '',
          source_name: item.source_name || '',
          source_icon: item.source_icon || '',
          published_at: item.published_at,
          is_read: item.is_read,
          is_starred: item.is_starred,
        },
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.sources', async () => {
    try {
      const { getSources } = await import('../../../../services/phantasiApi')
      const sources = await getSources(await bridge.hostAttributionHeaders(), {
        view: 'catalog',
      })
      return {
        success: true,
        data: sources.map((s) => ({
          id: s.id,
          name: s.name,
          url: s.url,
          icon: s.icon || '',
          description: s.description || '',
          category: s.category || '',
          item_count: s.item_count,
          unread_count: s.unread_count,
        })),
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.categories', async () => {
    try {
      const { getCategories } = await import('../../../../services/phantasiApi')
      const categories = await getCategories(
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: categories }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.stats', async () => {
    try {
      const { getStats } = await import('../../../../services/phantasiApi')
      const stats = await getStats(await bridge.hostAttributionHeaders())
      return { success: true, data: stats }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.discover', async (message) => {
    const [url] = getArgs(message) as [string]
    try {
      const { discoverSource } = await import('../../../../services/phantasiApi')
      const result = await discoverSource(
        url,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.exportOpml', async () => {
    try {
      const { exportOpml } = await import('../../../../services/phantasiApi')
      const opml = await exportOpml(await bridge.hostAttributionHeaders())
      return { success: true, data: opml }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.markRead', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { markRead } = await import('../../../../services/phantasiApi')
      await markRead(itemId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.markUnread', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { markUnread } = await import('../../../../services/phantasiApi')
      await markUnread(itemId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.star', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { starItem } = await import('../../../../services/phantasiApi')
      await starItem(itemId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.unstar', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { unstarItem } = await import('../../../../services/phantasiApi')
      await unstarItem(itemId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.markAllRead', async (message) => {
    const [options] = getArgs(message) as [
      { source_id?: number; category?: string; before?: number }?,
    ]
    try {
      const { markAllRead } = await import('../../../../services/phantasiApi')
      const count = await markAllRead(
        options,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: { count } }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.getComments', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { getComments } = await import('../../../../services/phantasiApi')
      const result = await getComments(
        itemId,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.createComment', async (message) => {
    const [itemId, req] = getArgs(message) as [
      number,
      {
        selected_text: string
        comment: string
        start_offset?: number
        end_offset?: number
        color?: string
        is_public?: boolean
        parent_id?: number
      },
    ]
    try {
      const { createComment } = await import('../../../../services/phantasiApi')
      const result = await createComment(
        itemId,
        req,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.updateComment', async (message) => {
    const [commentId, req] = getArgs(message) as [
      number,
      { comment?: string; color?: string },
    ]
    try {
      const { updateComment } = await import('../../../../services/phantasiApi')
      const result = await updateComment(
        commentId,
        req,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.deleteComment', async (message) => {
    const [commentId] = getArgs(message) as [number]
    try {
      const { deleteComment } = await import('../../../../services/phantasiApi')
      await deleteComment(commentId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.getReplies', async (message) => {
    const [commentId] = getArgs(message) as [number]
    try {
      const { getCommentReplies } = await import('../../../../services/phantasiApi')
      const result = await getCommentReplies(
        commentId,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.createReply', async (message) => {
    const [itemId, parentId, content] = getArgs(message) as [
      number,
      number,
      string,
    ]
    try {
      const { createReply } = await import('../../../../services/phantasiApi')
      const result = await createReply(
        itemId,
        parentId,
        content,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.addSource', async (message) => {
    const [req] = getArgs(message) as [{ url: string; category?: string }]
    try {
      const { addSource } = await import('../../../../services/phantasiApi')
      const source = await addSource(req, await bridge.hostAttributionHeaders())
      return {
        success: true,
        data: { id: source.id, name: source.name, url: source.url },
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.updateSource', async (message) => {
    const [id, req] = getArgs(message) as [
      number,
      { name?: string; category?: string },
    ]
    try {
      const { updateSource } = await import('../../../../services/phantasiApi')
      const source = await updateSource(
        id,
        req,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: { id: source.id, name: source.name } }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.deleteSource', async (message) => {
    const [id] = getArgs(message) as [number]
    try {
      const { deleteSource } = await import('../../../../services/phantasiApi')
      await deleteSource(id, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.refreshSource', async (message) => {
    const [id] = getArgs(message) as [number]
    try {
      const { refreshSource } = await import('../../../../services/phantasiApi')
      const count = await refreshSource(
        id,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: { new_items: count } }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.importOpml', async (message) => {
    const [opml] = getArgs(message) as [string]
    try {
      const { importOpml } = await import('../../../../services/phantasiApi')
      const result = await importOpml(
        opml,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.createCategory', async (message) => {
    const [req] = getArgs(message) as [{ name: string }]
    try {
      const { createCategory } = await import('../../../../services/phantasiApi')
      await createCategory(req, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.deleteCategory', async (message) => {
    const [id] = getArgs(message) as [number]
    try {
      const { deleteCategory } = await import('../../../../services/phantasiApi')
      await deleteCategory(id, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })
}
