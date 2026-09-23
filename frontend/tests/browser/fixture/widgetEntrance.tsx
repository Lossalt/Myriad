import React, { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { widgetEntranceMotion } from '../../../src/components/widgetEntranceMotion'
import { AnimationPreferenceProvider } from '../../../src/contexts/AnimationPreferenceContext'
import { coordinator } from '../../../src/hooks/animation/coordinator'
import { batchRead, batchWrite, runPageCleanup, scheduleTask, startPage } from '../../../src/hooks/animation/core'
import { useTappStagger } from '../../../src/hooks/animation/pages/tapp'
import { useLoopAnimation } from '../../../src/hooks/animation/useLoopAnimation'
import { usePageReady } from '../../../src/hooks/animation/usePageReady'
import { useVisibilityInterval } from '../../../src/hooks/animation/useVisibilityPause'
import { useWidgetResizeObserver } from '../../../src/hooks/animation/useWidgetResizeObserver'
import { useWidgetEntrance } from '../../../src/hooks/useWidgetEntrance'
import { ensureMotionReady } from '../../../src/lib/lazyMotion'
import {
  MotionEntrance,
  MotionEntranceHost,
} from '../../../src/lib/motionEntrance'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '../../../src/lib/motionShim'

function WidgetEntranceProbe({ index }: { index: number }) {
  const [content, setContent] = useState(0)
  const { phase, canAnimate, onComplete } = useWidgetEntrance(index, false)
  return (
    <motion.div
      data-entrance-probe={index}
      initial={widgetEntranceMotion.hidden}
      animate={
        canAnimate ? widgetEntranceMotion.visible : widgetEntranceMotion.hidden
      }
      transition={widgetEntranceMotion.transition}
      onAnimationComplete={onComplete}
      data-complete={phase === 'complete'}
      style={{ width: 280, height: 180, background: '#d8e6f2', margin: 8 }}
    >
      <MotionEntranceHost phase={phase}>
        <motion.span
          key={content}
          data-entrance-content={content}
          initial={{ opacity: 0, x: -20 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ duration: 0.4, delay: 0.3 }}
          style={{ display: 'inline-block' }}
        >
          Widget {index + 1}
        </motion.span>
        <button onClick={() => setContent((value) => value + 1)}>
          Change content
        </button>
      </MotionEntranceHost>
    </motion.div>
  )
}

function AdmissionProbe() {
  const [index, setIndex] = useState(1)
  const ready = usePageReady()
  const tapp = useTappStagger(index, { baseDelay: 115 })
  const widget = useWidgetEntrance(index, false)
  return <>
    <button onClick={() => setIndex(value => value + 1)}>Reorder</button>
      <output data-page-ready>{String(ready)}</output>
      <output data-admission="tapp">{String(tapp.canAnimate)}</output>
    <output data-admission="widget">{String(widget.canAnimate)}</output>
    <button onClick={tapp.onComplete}>Complete TAPP</button>
    <button onClick={widget.onComplete}>Complete widget</button>
  </>
}

function LifetimeProbe() {
  const [ticks, setTicks] = useState(0)
  const [width, setWidth] = useState(0)
  const [enabled, setEnabled] = useState(true)
  const { observeWidgetResize, unobserveWidgetResize } = useWidgetResizeObserver()
  const { isAnimating } = useLoopAnimation({ duration: 500, trigger: 'mount', enabled })
  useVisibilityInterval(() => setTicks(value => value + 1), { delay: 80 })
  return <>
    <output data-ticks>{ticks}</output>
    <output data-width>{width}</output>
    <output data-loop>{String(isAnimating)}</output>
    <button onClick={() => setEnabled(false)}>Disable loop</button>
    <div
      data-resize-target
      style={{ width: 100, height: 10 }}
      ref={element => {
      if (!element) return
      observeWidgetResize(element, entry => setWidth(entry.contentRect.width))
      return () => unobserveWidgetResize(element)
    }}
    />
  </>
}

function EntranceModesProbe() {
  const [disabled, setDisabled] = useState(true)
  const { phase, canAnimate, onComplete } = useWidgetEntrance(0, disabled)
  return (
    <>
      <button onClick={() => setDisabled((value) => !value)}>
        Toggle motion
      </button>
      <motion.div
        data-mode-probe
        data-phase={phase}
        initial={false}
        animate={
          canAnimate
            ? widgetEntranceMotion.visible
            : widgetEntranceMotion.hidden
        }
        transition={widgetEntranceMotion.transition}
        onAnimationComplete={onComplete}
      >
        <MotionEntranceHost phase={phase}>
          <MotionEntrance>
            <motion.div
              data-mode-content
              initial={{ opacity: 0, x: -20 }}
              animate={{ opacity: 1, x: 0 }}
              transition={{ duration: 0.4 }}
            >
              Already visible content
            </motion.div>
          </MotionEntrance>
        </MotionEntranceHost>
      </motion.div>
    </>
  )
}

Object.assign(window, {
  widgetEntranceFixture: {
    batchRead,
    batchWrite,
    scheduleTask,
    startPage,
    runPageCleanup,
    mountLifetimeProbe: () => {
      startPage('home')
      createRoot(document.getElementById('root')!).render(<StrictMode><LifetimeProbe /></StrictMode>)
    },
    mountAdmissionProbe: () => {
      coordinator.startPageTransition('admission-audit')
      coordinator.updateConfig({ baseConcurrent: 1, burstConcurrent: 1 })
      createRoot(document.getElementById('root')!).render(<AdmissionProbe />)
    },
    mountEntranceModes: async () => {
      await ensureMotionReady()
      coordinator.completePageTransition()
      createRoot(document.getElementById('root')!).render(
        <EntranceModesProbe />,
      )
    },
    mountWelcomeEntrance: async (size: '2x2' | '4x2', immediate = false) => {
      const [
        { WidgetGridItem },
        { WelcomeWidget },
        { I18nProvider },
        { MemoryRouter },
      ] = await Promise.all([
        import('../../../src/components/WidgetGridItem'),
        import('../../../src/components/widgets/WelcomeWidget'),
        import('../../../src/contexts/I18nContext'),
        import('react-router-dom'),
        ensureMotionReady(),
      ])
      const registry =
        await import('../../../src/components/widgets/builtinWidgets')
      if (immediate) await registry.preloadBuiltinWidgets(['welcome'])
      const component = immediate
        ? registry.BUILTIN_WIDGET_BASE_CONFIG.welcome.component
        : WelcomeWidget
      coordinator.startPageTransition('welcome-audit')
      if (immediate) coordinator.completePageTransition('welcome-audit')
      localStorage.setItem('animation-preference', 'standard')
      const container = document.createElement('div')
      container.style.cssText = 'width:900px;height:400px;position:relative'
      document.body.append(container)
      createRoot(container).render(
        <I18nProvider>
          <AnimationPreferenceProvider>
            <MemoryRouter>
              <AnimatePresence initial={false}>
                <motion.div key="home">
                  <WidgetGridItem
                    widget={{
                      id: 'welcome-audit',
                      type: 'welcome',
                      size,
                      position: { x: 0, y: 0 },
                    }}
                    widgetType={{
                      id: 'welcome',
                      name: 'Welcome',
                      defaultSize: '4x2',
                      component,
                    }}
                    isEditMode={false}
                    isPreview
                    isHovered={false}
                    index={immediate ? 0 : 2}
                    onDragStart={() => {}}
                    onMouseEnter={() => {}}
                    onMouseLeave={() => {}}
                    onRemove={() => {}}
                    onResizeStart={() => {}}
                  />
                </motion.div>
              </AnimatePresence>
            </MemoryRouter>
          </AnimationPreferenceProvider>
        </I18nProvider>,
      )
    },
    // A card whose content module is still loading; the test releases it.
    mountHeldEntrance: async () => {
      const [{ WidgetGridItem }, { lazyWithPreload }, { I18nProvider }] = await Promise.all([
        import('../../../src/components/WidgetGridItem'),
        import('../../../src/utils/codeSplitting'),
        import('../../../src/contexts/I18nContext'),
        ensureMotionReady(),
      ])
      const gate = Promise.withResolvers<void>()
      Object.assign(window, { releaseHeldEntrance: gate.resolve })
      const Content = lazyWithPreload(async () => {
        await gate.promise
        return { default: () => <p data-held-content>Ready</p> }
      })
      coordinator.startPageTransition('held-audit')
      coordinator.completePageTransition('held-audit')
      localStorage.setItem('animation-preference', 'standard')
      const container = document.createElement('div')
      container.style.cssText = 'width:900px;height:400px;position:relative'
      document.body.append(container)
      createRoot(container).render(
        <I18nProvider>
          <AnimationPreferenceProvider>
            <WidgetGridItem
              widget={{ id: 'held-audit', type: 'held', size: '2x2', position: { x: 0, y: 0 } }}
              widgetType={{ id: 'held', name: 'Held', defaultSize: '2x2', component: Content, preload: Content.preload }}
              isEditMode={false}
              isHovered={false}
              index={0}
              onDragStart={() => {}}
              onMouseEnter={() => {}}
              onMouseLeave={() => {}}
              onRemove={() => {}}
              onResizeStart={() => {}}
            />
          </AnimationPreferenceProvider>
        </I18nProvider>,
      )
    },
    mountWidgetEntrance: async () => {
      await ensureMotionReady()
      coordinator.completePageTransition()
      const container = document.createElement('div')
      document.body.append(container)
      createRoot(container).render(
        <>
          {[0, 1, 2].map((index) => (
            <WidgetEntranceProbe key={index} index={index} />
          ))}
        </>,
      )
    },
    coordinator,
  },
})
