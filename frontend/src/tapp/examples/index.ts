import type { ExampleTapp } from './tapps/types'
import { helloWorldTapp } from './tapps/helloWorld'
import { rssReaderTapp } from './tapps/rssReader'

export { helloWorldTapp } from './tapps/helloWorld'
export { rssReaderTapp } from './tapps/rssReader'

export type { ExampleTapp } from './tapps/types'

export const EXAMPLE_TAPPS: ExampleTapp[] = [
  helloWorldTapp,
  rssReaderTapp,
]

export default EXAMPLE_TAPPS
