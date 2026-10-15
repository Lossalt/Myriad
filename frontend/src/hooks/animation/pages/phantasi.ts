import { useMemo } from 'react'

import { isExlight, useAnimationLevel } from '../../useAnimationLevel'

type AnimationConfig = ReturnType<typeof useAnimationLevel>

// 按 level 缓存，避免每次新对象。
const ANIM_CONFIG_CACHE = new Map<
  string,
  ReturnType<typeof usePhantasiAnimationConfig>
>()

export function usePhantasiAnimationConfig(): AnimationConfig {
  const baseConfig = useAnimationLevel()

  return useMemo(() => {
    const cacheKey = baseConfig.level
    const cached = ANIM_CONFIG_CACHE.get(cacheKey)
    if (cached?.level === baseConfig.level) {
      return cached
    }

    ANIM_CONFIG_CACHE.set(cacheKey, baseConfig)
    return baseConfig
  }, [baseConfig])
}

export const phantasiAnimationPresets = {

  readerEnter: {
    initial: { opacity: 0, y: 40 },
    animate: { opacity: 1, y: 0 },
    exit: { opacity: 0, y: 40 },
  },
} as const

export function getPhantasiTransition(
  animConfig: AnimationConfig,
  type: 'reader' = 'reader',
): { duration: number; ease: [number, number, number, number] } {
  const durations = {
    reader:
      isExlight(animConfig)
        ? 0
        : animConfig.level === 'light'
          ? 0.2
          : 0.4,
  }

  return {
    duration: durations[type],
    ease: [0.16, 1, 0.3, 1],
  }
}
