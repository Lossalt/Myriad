/**
 * Talking over her holds her voice instead of cutting it off. The hold
 * becomes a cut off once the speech is long enough to be meant, or once it
 * turns out to have words; anything else (a cough, a door, nothing
 * recognisable) lets her go on from where she stopped.
 */
export interface BargeInHost {
  /** Stop her voice where it is, keeping it. False when nothing was playing. */
  hold: () => boolean
  resume: () => void
  cancel: () => void
}

export interface BargeInOptions {
  /**
   * The longest a hold waits for the words once the speech ended. A
   * transcription that never answers must not leave her silent.
   */
  wordsTimeoutMs?: number
  setTimer?: (run: () => void, ms: number) => unknown
  clearTimer?: (timer: unknown) => void
}

export const BARGE_IN_WORDS_TIMEOUT_MS = 2_000

export class BargeIn {
  /** Talking over her, not yet decided either way. */
  private pending = false
  /** Her voice is stopped where it was. */
  private holding = false
  private ticket = 0
  private timer: unknown = null
  private readonly wordsTimeoutMs: number
  private readonly setTimer: (run: () => void, ms: number) => unknown
  private readonly clearTimer: (timer: unknown) => void

  constructor(
    private readonly host: BargeInHost,
    options: BargeInOptions = {},
  ) {
    this.wordsTimeoutMs = options.wordsTimeoutMs ?? BARGE_IN_WORDS_TIMEOUT_MS
    this.setTimer = options.setTimer ?? ((run, ms) => setTimeout(run, ms))
    this.clearTimer =
      options.clearTimer ??
      ((timer) => clearTimeout(timer as ReturnType<typeof setTimeout>))
  }

  get held(): boolean {
    return this.holding
  }

  /** Someone started talking. */
  begin(): void {
    this.ticket += 1
    this.stopTimer()
    this.pending = true
    if (!this.holding) this.holding = this.host.hold()
  }

  /**
   * Meant: what she was saying is cut off. Also when nothing was audible
   * yet, so the rest of that reply is not said over them.
   */
  confirm(): void {
    this.ticket += 1
    this.stopTimer()
    if (!this.pending) return
    this.pending = false
    this.holding = false
    this.host.cancel()
  }

  /** Too little to be speech at all. */
  dismiss(): void {
    this.ticket += 1
    this.settle()
  }

  /**
   * The speech ended and is being written down. The returned settle says
   * whether it had words; only the latest speech decides. A slow answer
   * lets her voice go on meanwhile, but words still cut off what is left.
   */
  awaitWords(): (words: boolean) => void {
    const ticket = ++this.ticket
    this.stopTimer()
    if (this.holding) {
      this.timer = this.setTimer(() => {
        this.timer = null
        if (ticket === this.ticket) this.release()
      }, this.wordsTimeoutMs)
    }
    return (words) => {
      if (ticket !== this.ticket) return
      if (words) { this.confirm()
}
      else {
        this.ticket += 1
        this.settle()
      }
    }
  }

  /** Listening stopped: nothing will decide, so she goes on. */
  reset(): void {
    this.ticket += 1
    this.settle()
  }

  private settle(): void {
    this.pending = false
    this.release()
  }

  private release(): void {
    this.stopTimer()
    if (!this.holding) return
    this.holding = false
    this.host.resume()
  }

  private stopTimer(): void {
    if (this.timer == null) return
    this.clearTimer(this.timer)
    this.timer = null
  }
}
