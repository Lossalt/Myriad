import { frameRms } from './audioWav'

export interface CapturedUtterance {
  pcm: Float32Array[]
  sampleRate: number
}

/**
 * Speech this long is taken as meant: what it interrupts is cut off. Shorter
 * sounds only hold her voice until the words are known.
 */
export const CONFIRM_INTERRUPTION_SECONDS = 0.5

/**
 * How long a silence ends what they are saying. It starts where it always
 * was and learns from this person: pauses they make mid-sentence pull it
 * toward the floor, a sentence they pick up again right after it was taken
 * as finished pushes it up. Only ever between the floor and the ceiling, and
 * only for this listening session.
 */
export const ENDPOINT_FLOOR_SECONDS = 0.55
export const ENDPOINT_CEILING_SECONDS = 1.2
/** Weight kept on what was learned before, per pause. */
const ENDPOINT_HISTORY = 0.9
/** Shorter gaps are between syllables, not pauses. */
const MIN_PAUSE_SECONDS = 0.2

export interface CaptureEvent {
  started: boolean
  /** Voiced long enough to be meant, reported once per utterance. */
  confirmed: boolean
  ended: boolean
  utterance?: CapturedUtterance
}

export class UtteranceCapture {
  private preRoll: Float32Array[] = []
  private preSamples = 0
  private clip: Float32Array[] = []
  private clipSamples = 0
  private openSamples = 0
  private quietSamples = 0
  private voicedSamples = 0
  private active = false
  private confirmed = false
  private threshold = 0.035
  private endpointSeconds = ENDPOINT_FLOOR_SECONDS
  /** Samples since the last voiced frame; null before any voice. */
  private silentRun: number | null = null
  /** The silence before this opening, when it may continue the last one. */
  private gapBeforeOpening: number | null = null
  /** The last utterance ended on silence, not on the length limit. */
  private endedOnSilence = false

  constructor(readonly sampleRate: number) {}

  /** The silence, in seconds, that currently ends an utterance. */
  get endpointDelay(): number {
    return this.endpointSeconds
  }

  push(
    frame: Float32Array,
    playback: boolean,
  ): CaptureEvent {
    if (frame.length === 0) return idle()
    const rms = frameRms(frame)
    if (!this.active) {
      this.preRoll.push(frame)
      this.preSamples += frame.length
      const preLimit = Math.round(this.sampleRate * 0.32)
      while (this.preSamples > preLimit && this.preRoll.length > 0) {
        const first = this.preRoll[0]!
        const excess = this.preSamples - preLimit
        if (first.length <= excess) {
          this.preRoll.shift()
          this.preSamples -= first.length
        } else {
          this.preRoll[0] = first.slice(excess)
          this.preSamples -= excess
        }
      }
      this.threshold = playback ? 0.14 : 0.035
      // She answered in between: what follows is a new turn, not a pause.
      if (playback) this.endedOnSilence = false
      const voiced = rms >= this.threshold
      if (voiced && this.openSamples === 0)
        this.gapBeforeOpening = this.endedOnSilence ? this.silentRun : null
      this.openSamples = voiced ? this.openSamples + frame.length : 0
      this.track(voiced, frame.length)
      // A single loud click must not immediately stop the reply.
      if (this.openSamples < this.sampleRate * (playback ? 0.16 : 0.12)) {
        return idle()
      }
      this.active = true
      this.clip = this.preRoll
      this.clipSamples = this.preSamples
      this.preRoll = []
      this.preSamples = 0
      this.voicedSamples = this.openSamples
      this.quietSamples = 0
      this.learnFromRestart()
      return { ...idle(), started: true, confirmed: this.confirm() }
    }

    this.clip.push(frame)
    this.clipSamples += frame.length
    // The louder threshold protects barge-in only while playback is present.
    const voiced = rms >= (playback ? this.threshold : 0.035) * 0.45
    if (voiced) {
      // Her voice can hide or fake their pauses: learn only without it.
      if (!playback) this.learn(this.quietSamples / this.sampleRate)
      this.quietSamples = 0
      this.voicedSamples += frame.length
    } else {
      this.quietSamples += frame.length
    }
    this.track(voiced, frame.length)
    const confirmed = this.confirm()
    const bySilence = this.quietSamples >= this.sampleRate * this.endpointSeconds
    if (!bySilence && this.clipSamples < this.sampleRate * 20) {
      return { ...idle(), confirmed }
    }
    const utterance = this.take()
    this.endedOnSilence = bySilence && utterance != null
    return { ...idle(), confirmed, ended: true, utterance }
  }

  private track(voiced: boolean, samples: number): void {
    if (voiced) this.silentRun = 0
    else if (this.silentRun != null) this.silentRun += samples
  }

  /**
   * They went on right after the last utterance was taken as finished: that
   * silence was a pause, and the endpoint should have covered it.
   */
  private learnFromRestart(): void {
    const gap = this.gapBeforeOpening
    this.gapBeforeOpening = null
    this.endedOnSilence = false
    if (gap == null) return
    const seconds = gap / this.sampleRate
    if (seconds <= ENDPOINT_CEILING_SECONDS) this.learn(seconds)
  }

  private learn(pauseSeconds: number): void {
    if (pauseSeconds < MIN_PAUSE_SECONDS) return
    const pause = Math.min(
      ENDPOINT_CEILING_SECONDS,
      Math.max(ENDPOINT_FLOOR_SECONDS, pauseSeconds),
    )
    this.endpointSeconds =
      ENDPOINT_HISTORY * this.endpointSeconds + (1 - ENDPOINT_HISTORY) * pause
  }

  private confirm(): boolean {
    if (
      this.confirmed ||
      this.voicedSamples < this.sampleRate * CONFIRM_INTERRUPTION_SECONDS
    ) {
      return false
}
    this.confirmed = true
    return true
  }

  finish(): CapturedUtterance | undefined {
    const utterance = this.take()
    this.reset()
    return utterance
  }

  /** Listening stops: nothing carries into the next listener. */
  reset(): void {
    this.clear()
    this.silentRun = null
    this.gapBeforeOpening = null
    this.endedOnSilence = false
  }

  private take(): CapturedUtterance | undefined {
    const utterance =
      this.active && this.voicedSamples >= this.sampleRate * 0.16
        ? { pcm: this.clip, sampleRate: this.sampleRate }
        : undefined
    this.clear()
    return utterance
  }

  private clear(): void {
    this.preRoll = []
    this.preSamples = 0
    this.clip = []
    this.clipSamples = 0
    this.openSamples = 0
    this.quietSamples = 0
    this.voicedSamples = 0
    this.active = false
    this.confirmed = false
  }
}

function idle(): CaptureEvent {
  return { started: false, confirmed: false, ended: false }
}
