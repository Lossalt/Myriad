import type { ReactNode } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { WorkbenchPage } from '../phantasi/skin/PhantasiWorkbenchChrome'
import { PhantasiWorkbenchIcon } from '../phantasi/ui/PhantasiWorkbenchIcon'

/** 博客设置、主题和重建下一批按 API 合同接上。这一页只占住地址。 */
export function StudioBlogPane({ back }: { back: ReactNode }) {
  const { t } = useI18n()
  return (
    <WorkbenchPage
      title={t.studio.blog}
      icon={<PhantasiWorkbenchIcon kind="studio" />}
      back={back}
    >
      <p className="phantasi-workbench__home-row-excerpt">{t.studio.blogPending}</p>
    </WorkbenchPage>
  )
}
