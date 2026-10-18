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

  constructor(readonly sampleRate: number) {}

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
      this.openSamples =
        rms >= this.threshold ? this.openSamples + frame.length : 0
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
      return { ...idle(), started: true, confirmed: this.confirm() }
    }

    this.clip.push(frame)
    this.clipSamples += frame.length
    // The louder threshold protects barge-in only while playback is present.
    if (rms >= (playback ? this.threshold : 0.035) * 0.45) {
      this.quietSamples = 0
      this.voicedSamples += frame.length
    } else {
      this.quietSamples += frame.length
    }
    const confirmed = this.confirm()
    if (
      this.quietSamples < this.sampleRate * 0.55 &&
      this.clipSamples < this.sampleRate * 20
    ) {
      return { ...idle(), confirmed }
    }
    return { ...idle(), confirmed, ended: true, utterance: this.finish() }
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
    const utterance =
      this.active && this.voicedSamples >= this.sampleRate * 0.16
        ? { pcm: this.clip, sampleRate: this.sampleRate }
        : undefined
    this.reset()
    return utterance
  }

  reset(): void {
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
