import type { SpeechInterruptMode, SpeechSegment } from './speechSegmenter'
import {
  markTurnTraceOnce,
  noteTurnTraceDelay,
  noteTurnTraceDrop,
  noteTurnTraceQueue,
} from '../events/turnTrace'

export interface TtsAudioHandle {
  stop: () => void
  /** How far into its audio it has played, 0 to 1. */
  progress?: () => number
}

/** Where a reply read aloud was stopped: this many of its characters said. */
export interface SpeechCutOff {
  messageId: string
  heardChars: number
}

export interface TtsPipelineHost {
  synthesize: (
    segment: SpeechSegment,
    signal: AbortSignal,
  ) => Promise<ArrayBuffer | null>
  play: (
    audio: ArrayBuffer,
    segment: SpeechSegment,
    onEnded: () => void,
  ) => TtsAudioHandle
  onCancel?: (messageId: string) => void
  fallback?: (segment: SpeechSegment, onEnded: () => void) => TtsAudioHandle
}

const MAX_SYNTH = 2

interface QueuedSegment {
  playId: number
  segment: SpeechSegment
}

interface ReadySlot {
  segment: SpeechSegment
  audio: ArrayBuffer | null
}

interface Synthesis {
  segment: SpeechSegment
  controller: AbortController
}

/** Segment.sequence is per-utterance and must not be the play cursor. */
export class TtsPipeline {
  private nextPlayId = 0
  private nextPlay = 1
  private readonly synthesis = new Map<number, Synthesis>()
  private pending: QueuedSegment[] = []
  private readonly ready = new Map<number, ReadySlot>()
  private handle: TtsAudioHandle | null = null
  private playingMessageId: string | null = null
  private playSeq = 0
  /** The reply being read aloud, and how much of it has been said in full. */
  private said = { messageId: '', chars: 0, voiced: false }
  private playingText = ''

  constructor(private readonly host: TtsPipelineHost) {}

  get playing(): boolean {
    return this.handle != null
  }

  isBusyWith(messageId: string): boolean {
    if (this.playingMessageId === messageId) return true
    if (this.pending.some((item) => item.segment.messageId === messageId)) {
      return true
    }
    for (const slot of this.ready.values()) {
      if (slot.segment.messageId === messageId) return true
    }
    for (const task of this.synthesis.values()) {
      if (task.segment.messageId === messageId) return true
    }
    return false
  }

  get queueLength(): number {
    return (
      this.pending.length +
      this.ready.size +
      this.synthesis.size +
      (this.handle ? 1 : 0)
    )
  }

  /** Ordered future segments only */
  upcomingText(messageId: string, generation: number): string {
    return [
      ...this.pending,
      ...Iterator.from(this.ready).map(([playId, slot]) => ({
        playId,
        segment: slot.segment,
      })),
      ...Iterator.from(this.synthesis).map(([playId, task]) => ({
        playId,
        segment: task.segment,
      })),
    ]
      .filter(
        ({ segment }) =>
          segment.messageId === messageId && segment.generation === generation,
      )
      .toSorted((a, b) => a.playId - b.playId)
      .slice(0, 6)
      .map(({ segment }) => segment.text)
      .join('\n')
  }

  enqueue(
    segments: readonly SpeechSegment[],
    mode: SpeechInterruptMode = 'queue',
  ): void {
    if (segments.length === 0) return
    const messageId = segments[0]!.messageId
    if (mode === 'interrupt') this.cancel()
    else if (mode === 'replace') this.replaceMessage(messageId)
    for (const segment of segments) {
      this.nextPlayId += 1
      this.pending.push({ playId: this.nextPlayId, segment })
    }
    markTurnTraceOnce('tts_queued', { n: segments.length })
    this.noteQueue()
    this.pumpSynth()
  }

  /**
   * Where the reply being read aloud stands right now, if it is not said to
   * the end yet: what was played in full, and as much of the segment playing
   * as was heard. Counted in the characters read aloud, without markup.
   */
  cutOffPoint(messageId?: string): SpeechCutOff | null {
    const id = messageId ?? this.playingMessageId ?? this.nextMessageId()
    if (!id || !this.isBusyWith(id)) return null
    // Not started yet: none of it was heard.
    if (this.said.messageId !== id) return { messageId: id, heardChars: 0 }
    // Only a voice can be talked over; words shown are seen whole.
    if (!this.said.voiced) return null
    const playing =
      this.playingMessageId === id && this.handle?.progress
        ? Math.floor(this.playingText.length * this.handle.progress())
        : 0
    return { messageId: id, heardChars: this.said.chars + playing }
  }

  /** The reply whose next segment comes up first. */
  private nextMessageId(): string | null {
    let next: { playId: number, messageId: string } | null = null
    const consider = (playId: number, messageId: string) => {
      if (!next || playId < next.playId) next = { playId, messageId }
    }
    for (const item of this.pending) consider(item.playId, item.segment.messageId)
    for (const [playId, slot] of this.ready) consider(playId, slot.segment.messageId)
    for (const [playId, task] of this.synthesis)
      consider(playId, task.segment.messageId)
    return (next as { messageId: string } | null)?.messageId ?? null
  }

  cancel(messageId?: string): boolean {
    if (messageId) return this.cancelMessage(messageId)
    return this.cancelAll()
  }

  private cancelMessage(messageId: string): boolean {
    const stopped = this.playingMessageId === messageId
    const dropped = this.dropMessage(messageId)
    if (stopped) {
      this.stopPlayback()
      this.playingMessageId = null
      this.host.onCancel?.(messageId)
    }
    if (dropped || stopped) noteTurnTraceDrop('cancelled')
    this.noteQueue()
    this.tryPlay()
    this.pumpSynth()
    return stopped
  }

  private cancelAll(): boolean {
    const stopped = this.handle != null
    const hadWork =
      stopped ||
      this.pending.length > 0 ||
      this.ready.size > 0 ||
      this.synthesis.size > 0
    this.stopPlayback()
    this.pending.length = 0
    this.ready.clear()
    const abandoned = Iterator.from(this.synthesis.values()).toArray()
    this.synthesis.clear()
    this.nextPlayId = 0
    this.nextPlay = 1
    for (const task of abandoned) task.controller.abort()
    const id = this.playingMessageId
    this.playingMessageId = null
    this.noteQueue()
    if (hadWork) noteTurnTraceDrop('cancelled')
    if (id) this.host.onCancel?.(id)
    return stopped
  }

  private replaceMessage(messageId: string): void {
    const dropped = this.dropMessage(messageId)
    const stopped = this.playingMessageId === messageId
    if (stopped) {
      this.stopPlayback()
      this.playingMessageId = null
    }
    if (dropped || stopped) noteTurnTraceDrop('queue_replaced')
    this.noteQueue()
    this.tryPlay()
  }

  private dropMessage(messageId: string): boolean {
    let dropped = false
    for (let i = this.pending.length - 1; i >= 0; i--) {
      if (this.pending[i]?.segment.messageId === messageId) {
        this.pending = this.pending.toSpliced(i, 1)
        dropped = true
      }
    }
    for (const [playId, slot] of Iterator.from(this.ready).toArray()) {
      if (slot.segment.messageId === messageId) {
        this.ready.delete(playId)
        dropped = true
      }
    }
    for (const [playId, task] of this.synthesis) {
      if (task.segment.messageId === messageId) {
        this.synthesis.delete(playId)
        task.controller.abort()
        dropped = true
      }
    }
    return dropped
  }

  private pumpSynth(): void {
    while (this.synthesis.size < MAX_SYNTH && this.pending.length > 0) {
      const item = this.pending.shift()!
      const task: Synthesis = {
        segment: item.segment,
        controller: new AbortController(),
      }
      const started = nowMs()
      this.synthesis.set(item.playId, task)
      this.noteQueue()
      let result: Promise<ArrayBuffer | null>
      try {
        result = this.host.synthesize(item.segment, task.controller.signal)
      } catch {
        result = Promise.resolve(null)
      }
      void result
        .catch(() => null)
        .then((audio) => {
          if (this.synthesis.get(item.playId) !== task) return
          this.synthesis.delete(item.playId)
          if (audio) {
            noteTurnTraceDelay('tts', nowMs() - started)
            markTurnTraceOnce('tts_ready')
          } else {
            noteTurnTraceDrop('synth_failed')
          }
          this.ready.set(item.playId, { segment: item.segment, audio })
          this.tryPlay()
          this.pumpSynth()
        })
    }
  }

  private tryPlay(): void {
    if (this.handle) return
    this.skipMissingPlays()
    const slot = this.ready.get(this.nextPlay)
    if (!slot) return
    this.ready.delete(this.nextPlay)
    this.nextPlay += 1
    if (!slot.audio && !this.host.fallback) {
      this.tryPlay()
      return
    }
    this.playingMessageId = slot.segment.messageId
    if (this.said.messageId !== slot.segment.messageId)
      this.said = { messageId: slot.segment.messageId, chars: 0, voiced: false }
    if (slot.audio) this.said.voiced = true
    this.playingText = slot.segment.text
    const playSeq = ++this.playSeq
    let ended = false
    markTurnTraceOnce('playback_started')
    this.handle = { stop: () => undefined }
    const onEnded = () => {
      if (playSeq !== this.playSeq) return
      ended = true
      if (this.said.messageId === slot.segment.messageId)
        this.said.chars += slot.segment.text.length
      this.handle = null
      this.playingMessageId = null
      this.noteQueue()
      this.tryPlay()
    }
    const handle = slot.audio
      ? this.host.play(slot.audio, slot.segment, onEnded)
      : this.host.fallback!(slot.segment, onEnded)
    if (!ended && playSeq === this.playSeq) this.handle = handle
  }

  private stopPlayback(): void {
    this.playSeq += 1
    this.handle?.stop()
    this.handle = null
    this.noteQueue()
  }

  private skipMissingPlays(): void {
    while (
      this.nextPlay <= this.nextPlayId &&
      !this.ready.has(this.nextPlay) &&
      !this.synthesis.has(this.nextPlay) &&
      !this.pending.some((item) => item.playId === this.nextPlay)
    ) {
      this.nextPlay += 1
    }
  }

  private noteQueue(): void {
    noteTurnTraceQueue(this.queueLength)
  }
}

function nowMs(): number {
  return typeof performance === 'undefined' ? Date.now() : performance.now()
}
