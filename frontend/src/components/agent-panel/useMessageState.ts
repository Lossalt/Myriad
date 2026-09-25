import type { AgentPanelMode } from './agentPanelMode'
import type { ChatMessage, ExecutionStep, TaskExecution } from './engineTypes'
import { useCallback, useEffect, useRef, useState } from 'react'
import { authSubject } from '../../utils/authSubject'
import { BODY_INLINE_CHARS, prepareMessageBody, releaseMessageBody } from './messageBody'
import { boundMessage, retainHotMessages } from './messageBudget'
import { prepareAssistantBody } from './prepareChatBody'

export type MessagesByMode = Record<AgentPanelMode, ChatMessage[]>

export function emptyMessagesByMode(): MessagesByMode {
  return { work: [], chat: [] }
}

export function writeModeMessages(
  bag: MessagesByMode,
  mode: AgentPanelMode,
  next: ChatMessage[],
): MessagesByMode {
  if (bag[mode] === next) return bag
  return { ...bag, [mode]: retainHotMessages(next) }
}

export function findMessageInBag(
  bag: MessagesByMode,
  messageId: string,
): ChatMessage | undefined {
  return findMessageWhereInBag(bag, (message) => message.id === messageId)
}

export function findMessageWhereInBag(
  bag: MessagesByMode,
  predicate: (message: ChatMessage) => boolean,
): ChatMessage | undefined {
  return bag.work.find(predicate) ?? bag.chat.find(predicate)
}

interface MessageChange {
  previous: readonly ChatMessage[]
  index: number
  depth: number
}
const changes = new WeakMap<readonly ChatMessage[], MessageChange>()
const indices = new WeakMap<readonly ChatMessage[], Map<string, number>>()

/** A bounded journal bridges updates React may batch before projection. */
export function changedMessageIndices(
  previous: readonly ChatMessage[],
  next: readonly ChatMessage[],
): Set<number> | null {
  const result = new Set<number>()
  let cursor = next
  while (cursor !== previous) {
    const change = changes.get(cursor)
    if (!change) return null
    result.add(change.index)
    changes.delete(cursor)
    cursor = change.previous
  }
  return result
}

export function mapMessagesById(
  bag: MessagesByMode,
  messageId: string,
  update: (message: ChatMessage) => ChatMessage,
): MessagesByMode {
  let next = bag
  for (const mode of ['work', 'chat'] as const) {
    const list = bag[mode]
    let index = indices.get(list)
    if (!index) {
      index = new Map(list.map((message, position) => [message.id, position]))
      indices.set(list, index)
    }
    const position = index.get(messageId)
    if (position === undefined) continue
    const rawUpdated = update(list[position])
    if (rawUpdated === list[position]) continue
    const updated = boundMessage(rawUpdated)
    const candidate = list.slice()
    candidate[position] = updated
    const mapped = retainHotMessages(candidate)
    const structuralChange = mapped.length !== candidate.length || mapped.some((message, i) => message !== candidate[i])
    if (!structuralChange) indices.set(mapped, index)
    const depth = (changes.get(list)?.depth ?? 0) + 1
    // Prevent an inactive mode from retaining an unbounded chain of old arrays.
    if (!structuralChange && depth <= 4)
      changes.set(mapped, { previous: list, index: position, depth })
    next = { ...next, [mode]: mapped }
  }
  return next
}

export function useMessageState(visibleMode: AgentPanelMode) {
  const [byMode, setByMode] = useState<MessagesByMode>(emptyMessagesByMode)
  const lifetime = useRef(new AbortController())
  useEffect(() => {
    lifetime.current = new AbortController()
    return () => lifetime.current.abort()
  }, [])
  const pendingBodies = useRef(new Map<string, AbortController>())
  const messagesRef = useRef(byMode)
  messagesRef.current = byMode
  const messages = byMode[visibleMode]

  const ownedBodies = useRef(new Map<string, import('./messageBody').MessageBodyRef>())
  useEffect(() => {
    const next = new Map<string, import('./messageBody').MessageBodyRef>()
    for (const message of [...byMode.work, ...byMode.chat]) {
      for (const body of [message.body, message.taskExecution?.reasoningBody]) { if (body) next.set(body.id, body)
}
    }
    for (const [id, body] of ownedBodies.current) { if (!next.has(id)) void releaseMessageBody(body).catch(() => {})
}
    ownedBodies.current = next
  }, [byMode])
  useEffect(() => () => {
    for (const body of ownedBodies.current.values()) void releaseMessageBody(body).catch(() => {})
    ownedBodies.current.clear()
  }, [])

  const setMessages = useCallback(
    (
      next: ChatMessage[] | ((prev: ChatMessage[]) => ChatMessage[]),
      mode: AgentPanelMode = visibleMode,
    ) => {
      setByMode((prev) => {
        const current = prev[mode]
        const value = typeof next === 'function' ? next(current) : next
        return writeModeMessages(prev, mode, value)
      })
    },
    [visibleMode],
  )

  const updateMessage = useCallback(
    async (messageId: string, updates: Partial<ChatMessage>) => {
      const hasContent = updates.content !== undefined
      if (hasContent) pendingBodies.current.get(messageId)?.abort()
      const controller = new AbortController()
      const subject = AbortSignal.any([authSubject.signal, lifetime.current.signal, controller.signal])
      let prepared: import('./messageBody').MessageBodyRef | undefined
      let thought: Awaited<ReturnType<typeof prepareAssistantBody>>['thought'] | undefined
      const originalContent = updates.content
      if (hasContent) pendingBodies.current.set(messageId, controller)
      if (updates.content !== undefined && (updates.content.length > BODY_INLINE_CHARS || /<think>|\[\[(?:wear|music):|⟦wear:/i.test(updates.content)) && !updates.body) {
        try {
          const existing = [...messagesRef.current.work, ...messagesRef.current.chat].find(message => message.id === messageId)
          const assistantBody = existing?.role !== 'user' && updates.role !== 'user' ? await prepareAssistantBody(updates.content, subject) : null
          const result = assistantBody ?? await prepareMessageBody(updates.content, subject)
          thought = assistantBody?.thought
          prepared = result.body
          updates = { ...updates, content: result.content, body: result.body, bodyUnavailable: false }
        } catch {
          if (subject.aborted) {
            if (pendingBodies.current.get(messageId) === controller) pendingBodies.current.delete(messageId)
            return
          }
          updates = { ...updates, content: originalContent!.slice(0, BODY_INLINE_CHARS), body: undefined, bodyUnavailable: true }
        }
      } else if (updates.content !== undefined && !Object.hasOwn(updates, 'body')) {
        updates = { ...updates, body: undefined, bodyUnavailable: false }
      }
      if (pendingBodies.current.get(messageId) === controller) pendingBodies.current.delete(messageId)
      if (subject.aborted) {
        for (const body of [prepared, thought?.body]) { if (body) void releaseMessageBody(body).catch(() => {})
}
        return
      }
      setByMode((prev) => {
        const next = mapMessagesById(prev, messageId, (message) => ({ ...message, ...updates, ...(thought?.content || thought?.body ? { taskExecution: { ...(message.taskExecution ?? { taskId: '', status: 'completed' as const, progress: 100, steps: [] }), reasoning: thought.content, reasoningBody: thought.body } } : {}) }))
        const refs = new Set([...next.work, ...next.chat].flatMap(message => [message.body?.id, message.taskExecution?.reasoningBody?.id]))
        for (const body of [prepared, thought?.body]) { if (body && !refs.has(body.id)) void releaseMessageBody(body).catch(() => {})
}
        return next
      })
    },
    [],
  )

  const updateMessageExecution = useCallback(
    (messageId: string, updates: Partial<TaskExecution>) => {
      setByMode((prev) =>
        mapMessagesById(prev, messageId, (message) => {
          if (!message.taskExecution) return message
          const newProgress =
            updates.progress != null
              ? Math.max(updates.progress, message.taskExecution.progress)
              : message.taskExecution.progress
          return {
            ...message,
            taskExecution: {
              ...message.taskExecution,
              ...updates,
              progress: newProgress,
            },
          }
        }),
      )
    },
    [],
  )

  const addExecutionStep = useCallback(
    (messageId: string, step: ExecutionStep) => {
      setByMode((prev) =>
        mapMessagesById(prev, messageId, (message) => {
          if (!message.taskExecution) return message
          const exists = message.taskExecution.steps.some(
            (item) => item.id === step.id,
          )
          if (exists) {
            return {
              ...message,
              taskExecution: {
                ...message.taskExecution,
                steps: message.taskExecution.steps.map((item) =>
                  item.id === step.id ? { ...item, ...step } : item,
                ),
              },
            }
          }
          return {
            ...message,
            taskExecution: {
              ...message.taskExecution,
              steps: [...message.taskExecution.steps, step],
            },
          }
        }),
      )
    },
    [],
  )

  const updateExecutionStep = useCallback(
    (messageId: string, stepId: string, updates: Partial<ExecutionStep>) => {
      setByMode((prev) =>
        mapMessagesById(prev, messageId, (message) => {
          if (!message.taskExecution) return message
          return {
            ...message,
            taskExecution: {
              ...message.taskExecution,
              steps: message.taskExecution.steps.map((item) =>
                item.id === stepId ? { ...item, ...updates } : item,
              ),
            },
          }
        }),
      )
    },
    [],
  )

  const findMessage = useCallback((messageId: string) => {
    return findMessageInBag(messagesRef.current, messageId)
  }, [])

  return {
    messages,
    setMessages,
    messagesRef,
    byMode,
    findMessage,
    updateMessage,
    updateMessageExecution,
    addExecutionStep,
    updateExecutionStep,
  }
}
