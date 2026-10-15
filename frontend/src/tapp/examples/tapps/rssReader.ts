import type { TappManifest } from '../../types'
import type { ExampleTapp } from './types'
import enUS from '../../../../../tapps/rss-reader/i18n/en-US.json'
import jaJP from '../../../../../tapps/rss-reader/i18n/ja-JP.json'
import zhCN from '../../../../../tapps/rss-reader/i18n/zh-CN.json'
import manifestJson from '../../../../../tapps/rss-reader/manifest.json'
import pageCss from '../../../../../tapps/rss-reader/page.css?raw'
import pageHtml from '../../../../../tapps/rss-reader/page.html?raw'
import pageApi from '../../../../../tapps/rss-reader/page/api.js?raw'
import pageApp from '../../../../../tapps/rss-reader/page/app.js?raw'
import pageIndex from '../../../../../tapps/rss-reader/page/index.js?raw'
import pageSanitize from '../../../../../tapps/rss-reader/page/sanitize.js?raw'

const manifest = manifestJson as TappManifest

/** 商店本地目录里的第一方阅读器。源文件在 `tapps/rss-reader`，这里只负责装进现有安装入口。 */
export const rssReaderTapp: ExampleTapp = {
  manifest,
  code: {
    core: '// 阅读器只有页面层。\n',
    page: pageIndex,
    styles: pageCss,
    pageHtml,
    i18n: {
      'zh-CN': zhCN,
      'en-US': enUS,
      'ja-JP': jaJP,
    },
    packageModules: {
      'page/app.js': pageApp,
      'page/api.js': pageApi,
      'page/sanitize.js': pageSanitize,
    },
  },
  tags: ['official', 'rss', 'reader'],
}
