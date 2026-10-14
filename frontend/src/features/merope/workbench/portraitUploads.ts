import type { WardrobeItem } from '../persona/wardrobe'
import { uploadFullBodyPortrait, uploadOutfitPortrait, uploadSitePortrait } from '../api'
import { notifyFaceUpdated } from '../events/updates'
import { isFullBodyItem } from '../persona/wardrobe'
import { requestUprightCheck } from './uprightRequests'
import { reportMeropeError } from './workbenchShared'

interface Workbench {
  loadFace: () => Promise<unknown>
  reload: () => Promise<unknown>
  voidSticker: () => void
  applyVisualFromPortrait: (url: string) => Promise<unknown>
  addOutfit: (item: WardrobeItem, options: { wear: boolean }) => Promise<unknown>
  manage: (id: string) => void
}

/**
 * The owner's own pictures put in place, each with what it was seen to need
 * redrawn upright; a redraw taken goes where its upload went.
 */
export function portraitUploads(workbench: Workbench) {
  /** A new master portrait for the worn outfit is in. */
  const adoptPortrait = async (url: string) => {
    reportMeropeError('')
    await workbench.loadFace()
    await workbench.reload()
    workbench.voidSticker()
    try {
      await workbench.applyVisualFromPortrait(url)
    } catch {
      await workbench.loadFace()
    }
    notifyFaceUpdated()
  }

  return {
    /** The owner uploaded a new master portrait for the worn outfit. */
    adoptUploadedPortrait: adoptPortrait,
    /** A bust set's picture redrawn upright, taken. */
    takeUprightBust: async (outfit: WardrobeItem, worn: boolean, file: File) => {
      if (worn) {
        const { portraitUrl } = await uploadSitePortrait(file)
        await adoptPortrait(portraitUrl)
        return
      }
      await uploadOutfitPortrait(outfit.id, file)
      await workbench.reload()
    },
    /** A new set made from the owner's own picture instead of a generated one. */
    createOutfitFromUpload: async (item: WardrobeItem, file: File) => {
      // Added without being worn: a bust keeps the master portrait it has.
      await workbench.addOutfit(item, { wear: false })
      const { uprightReasons } = isFullBodyItem(item)
        ? await uploadFullBodyPortrait(item.id, file)
        : await uploadOutfitPortrait(item.id, file)
      await workbench.reload()
      requestUprightCheck(item.id, uprightReasons)
      workbench.manage(item.id)
    },
  }
}
