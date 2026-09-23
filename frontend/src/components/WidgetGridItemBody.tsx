import type { WidgetConfig, WidgetType } from './widgetGridTypes'
import React, { Suspense, useCallback, useLayoutEffect, useRef } from 'react'

/** Commits only with its Suspense siblings, so it marks content that is actually on screen. */
function Presented({ onCommit }: { onCommit: () => void }) {
  useLayoutEffect(() => { onCommit() }, [onCommit])
  return null
}

const WidgetGridItemContent = React.memo(
  ({
    widget,
    widgetType,
    isEditMode,
    isPreview,
    onConfigChange,
    onPresentable,
  }: {
    widget: WidgetConfig
    widgetType: WidgetType
    isEditMode: boolean
    isPreview?: boolean
    onConfigChange?: (newConfig: any) => void
    onPresentable: () => void
  }) => {
    const WidgetComponent = widgetType.component
    return (
      <Suspense fallback={null}>
        <WidgetComponent
          config={widget}
          isEditMode={isEditMode}
          isPreview={isPreview}
          onConfigChange={onConfigChange}
        />
        <Presented onCommit={onPresentable} />
      </Suspense>
    )
  },
  (prev, next) =>
    prev.widget.id === next.widget.id &&
    prev.widget.type === next.widget.type &&
    prev.widget.size === next.widget.size &&
    prev.widget.config === next.widget.config &&
    prev.isEditMode === next.isEditMode &&
    prev.isPreview === next.isPreview &&
    prev.widgetType === next.widgetType &&
    prev.onConfigChange === next.onConfigChange,
)

/** Keep callback ownership current without repainting content on every grid move. */
export function WidgetGridItemBody({
  onPresentable,
  ...props
}: Omit<React.ComponentProps<typeof WidgetGridItemContent>, 'onPresentable'> & {
  onPresentable?: () => void
}) {
  const callback = useRef(props.onConfigChange)
  const presented = useRef(onPresentable)
  useLayoutEffect(() => {
    callback.current = props.onConfigChange
    presented.current = onPresentable
  }, [props.onConfigChange, onPresentable])
  const forwardConfigChange = useCallback((value: unknown) => callback.current?.(value), [])
  const forwardPresentable = useCallback(() => presented.current?.(), [])
  return (
    <WidgetGridItemContent
      {...props}
      onConfigChange={props.onConfigChange ? forwardConfigChange : undefined}
      onPresentable={forwardPresentable}
    />
  )
}
