import type { BodyControl, ScoreMove } from '../../../services/agent/types'
import type { FaceLife } from '../events/performanceContract'
import type { ResolvedScore } from '../motion/scoreTimeline'
import { angleZForHeadRoll, HEAD_TILT_RADIANS } from './poseScale'
import { scoreMoveOffsets, scoreMoveSeconds } from './scoreMoves'

/**
 * Everything the director has asked of the body, laid over time. Nothing in
 * here takes a control and gives it back: each beat rises, stays and settles
 * by its own envelope, and at any moment a control goes where the beats on it
 * pull, each by how strongly it is playing. A newer beat on the same control
 * fades in as an older one fades out.
 *
 * Her standing acting (what she does while nothing else is going on) plays
 * as strongly as she is free for it: it fades back as she speaks or as the
 * director's reply beats play, and comes up again after, by itself.
 */

/** A beat without its own length stays this long. */
const DEFAULT_HOLD_SECONDS = 3
const MIN_RELEASE_SECONDS = 0.4
/** How quickly she becomes taken up by speech or a reply, and free again (rad/s). */
const ENGAGE_RATE = 5
const RETURN_RATE = 1.6
/** How quickly one standing score gives way to the next (rad/s). */
const STANDING_RATE = 2.5

interface PoseItem {
  id: string
  scoreId: number
  standing: boolean
  /** Part of the life of the look her face holds: it neither takes her up nor waits for her. */
  face?: boolean
  control: BodyControl
  value: number
  at: number
  attack: number
  hold: number
  release: number
}

interface MoveItem {
  id: string
  scoreId: number
  standing: boolean
  face?: boolean
  at: number
  move: ScoreMove
}

interface Loop {
  scoreId: number
  /** The score's beats relative to its start, to play again each period. */
  beats: ResolvedScore['beats']
  startNowMs: number
  periodSeconds: number
  /** Player time of the cycle most recently laid out. */
  cycleAt: number
  cycle: number
}

export interface ActingSample {
  value: number
  weight: number
}

/**
 * Semantic up is positive; the renderer's image-space Y points down. A
 * directed head tilt says how much it should show, as a share of the full
 * tilt (HEAD_TILT_RADIANS): the director's slight (0.35) is about 8°, clear
 * (0.65) about 15° and strong the full 24°. Everyday motion keeps the low,
 * slow end of angleZ to itself.
 */
export function mappedControlValue(key: BodyControl, value: number): number {
  if (key === 'gazeVertical') return -value
  if (key === 'headTilt') return angleZForHeadRoll(value * HEAD_TILT_RADIANS)
  if (key === 'hairSway') return value * 1.5
  if (key === 'chestSway') return value * 4
  return value
}

export class ActingField {
  private poses: PoseItem[] = []
  private moves: MoveItem[] = []
  private replyScoreId = 0
  private standingScoreId = 0
  private loop: Loop | null = null
  /** The look her face holds, and its life playing again each period. */
  private face: { name: string; loop: Loop } | null = null
  private faceScoreId = 0
  private speaking = false
  private engaged = 0
  /** Per standing score: how much it is still the one playing. */
  private readonly standingShares = new Map<number, number>()
  readonly values = new Map<BodyControl, ActingSample>()
  readonly offsets = new Map<BodyControl, number>()
  readonly moveWeights = new Map<BodyControl, number>()

  setSpeaking(speaking: boolean): void {
    this.speaking = speaking
  }

  /**
   * A score from the director, moved from the clock (`nowMs`) onto the
   * player's (`time`). A revised score moves its beats that have not started;
   * a new one drops the unstarted beats of the one it follows, and what is
   * under way plays out.
   */
  setScore(score: Readonly<ResolvedScore>, time: number, nowMs: number): void {
    const standing = score.standing !== undefined
    const current = standing ? this.standingScoreId : this.replyScoreId
    if (score.id !== current) {
      this.poses = this.poses.filter((item) => item.face || item.standing !== standing || item.at <= time)
      this.moves = this.moves.filter((item) => item.face || item.standing !== standing || item.at <= time)
      if (standing) {
        this.standingScoreId = score.id
        this.standingShares.set(score.id, this.standingShares.get(score.id) ?? 0)
        this.loop = score.standing!.loopMs > 0
          ? { scoreId: score.id, beats: score.beats, startNowMs: nowMs, periodSeconds: score.standing!.loopMs / 1000, cycleAt: time, cycle: 0 }
          : null
      } else {
        this.replyScoreId = score.id
      }
    }
    this.lay(score.id, standing, score.beats, (atMs) => time + (atMs - nowMs) / 1000, '', time)
  }

  /**
   * The look her face holds (null for none). Its life plays from now on,
   * over and over until the look changes; what is under way of the last
   * look's life plays out.
   */
  setFace(name: string | null, life: Readonly<FaceLife> | null, time: number): void {
    if ((this.face?.name ?? null) === (life ? name : null)) return
    this.poses = this.poses.filter((item) => !item.face || item.at <= time)
    this.moves = this.moves.filter((item) => !item.face || item.at <= time)
    if (!name || !life) {
      this.face = null
      return
    }
    this.faceScoreId -= 1
    const beats = life.beats.map((beat, index) => ({ id: `face:${name}:${index}`, atMs: beat.atMs, ...(beat.pose ? { pose: beat.pose } : {}), ...(beat.move ? { move: beat.move } : {}) }))
    this.face = { name, loop: { scoreId: this.faceScoreId, beats, startNowMs: 0, periodSeconds: life.periodMs / 1000, cycleAt: time, cycle: 0 } }
    this.lay(this.faceScoreId, false, beats, (atMs) => time + atMs / 1000, '', time, true)
  }

  /** Advances to `time`; `drivable` says which controls this body has. */
  step(dt: number, time: number, drivable: (control: BodyControl) => boolean): void {
    this.continueLoop(time)
    this.continueFace(time)
    this.poses = this.poses.filter((item) => time < item.at + item.attack + item.hold + item.release)
    this.moves = this.moves.filter((item) => time < item.at + scoreMoveSeconds(item.move))
    // How taken up she is: speaking, or the director's reply beats playing.
    let reply = 0
    for (const item of this.poses) { if (!item.standing && !item.face) reply = Math.max(reply, envelope(item, time))
}
    const step = Number.isFinite(dt) ? Math.max(0, dt) : 0
    const busy = Math.max(this.speaking ? 1 : 0, reply)
    this.engaged += (busy - this.engaged) * (1 - Math.exp(-(busy > this.engaged ? ENGAGE_RATE : RETURN_RATE) * step))
    for (const [id, share] of this.standingShares) {
      const goal = id === this.standingScoreId ? 1 : 0
      const next = share + (goal - share) * (1 - Math.exp(-STANDING_RATE * step))
      if (goal === 0 && next < 0.001 && !this.poses.some((item) => item.scoreId === id)) this.standingShares.delete(id)
      else this.standingShares.set(id, next)
    }
    const gain = (standing: boolean, scoreId: number) =>
      standing ? (this.standingShares.get(scoreId) ?? 0) * (1 - this.engaged) : 1
    this.values.clear()
    const sums = new Map<BodyControl, { weight: number; weighted: number }>()
    for (const item of this.poses) {
      if (!drivable(item.control)) continue
      const strength = envelope(item, time) * gain(item.standing, item.scoreId)
      if (strength <= 0) continue
      const sum = sums.get(item.control) ?? { weight: 0, weighted: 0 }
      sum.weight += strength
      sum.weighted += strength * item.value
      sums.set(item.control, sum)
    }
    for (const [control, sum] of sums) {
      this.values.set(control, { value: sum.weighted / sum.weight, weight: Math.min(1, sum.weight) })
    }
    this.offsets.clear()
    this.moveWeights.clear()
    const offsets: Partial<Record<BodyControl, number>> = {}
    for (const item of this.moves) {
      if (time < item.at) continue
      for (const key of Object.keys(offsets) as BodyControl[]) delete offsets[key]
      const shape = scoreMoveOffsets(item.move, time - item.at, offsets)
      const strength = gain(item.standing, item.scoreId)
      for (const [control, offset] of Object.entries(offsets) as Array<[BodyControl, number]>) {
        if (!drivable(control)) continue
        this.offsets.set(control, (this.offsets.get(control) ?? 0) + offset * strength)
        this.moveWeights.set(control, Math.max(this.moveWeights.get(control) ?? 0, shape * strength))
      }
    }
  }

  /** A looping standing score lays out its next period before the last one ends. */
  private continueLoop(time: number): void {
    const loop = this.loop
    if (!loop || loop.scoreId !== this.standingScoreId) return
    while (time >= loop.cycleAt + loop.periodSeconds - 1) {
      loop.cycle += 1
      loop.cycleAt += loop.periodSeconds
      const cycleAt = loop.cycleAt
      this.lay(loop.scoreId, true, loop.beats, (atMs) => cycleAt + (atMs - loop.startNowMs) / 1000, `#${loop.cycle}`, time)
    }
  }

  private continueFace(time: number): void {
    const loop = this.face?.loop
    if (!loop) return
    while (time >= loop.cycleAt + loop.periodSeconds - 1) {
      loop.cycle += 1
      loop.cycleAt += loop.periodSeconds
      const cycleAt = loop.cycleAt
      this.lay(loop.scoreId, false, loop.beats, (atMs) => cycleAt + atMs / 1000, `#${loop.cycle}`, time, true)
    }
  }

  private lay(
    scoreId: number,
    standing: boolean,
    beats: ResolvedScore['beats'],
    at: (atMs: number) => number,
    suffix: string,
    time: number,
    face = false,
  ): void {
    for (const beat of beats) {
      const id = beat.id + suffix
      const start = at(beat.atMs)
      if (beat.pose) {
        const attack = beat.pose.transitionMs / 1000
        const hold = beat.pose.holdMs > 0 ? beat.pose.holdMs / 1000 : DEFAULT_HOLD_SECONDS
        for (const [control, value] of Object.entries(beat.pose.targets) as Array<[BodyControl, number]>) {
          const known = this.poses.find((item) => item.id === id && item.control === control)
          if (known) {
            if (known.at > time) known.at = start
            continue
          }
          this.poses.push({
            id, scoreId, standing, ...(face ? { face } : {}), control, value: mappedControlValue(control, value),
            at: start, attack, hold, release: Math.max(MIN_RELEASE_SECONDS, attack),
          })
        }
      }
      if (beat.move) {
        const known = this.moves.find((item) => item.id === id)
        if (known) {
          if (known.at > time) known.at = start
        } else {
          this.moves.push({ id, scoreId, standing, ...(face ? { face } : {}), at: start, move: beat.move })
        }
      }
    }
  }
}

function envelope(item: Readonly<PoseItem>, time: number): number {
  const t = time - item.at
  if (t < 0) return 0
  if (t < item.attack) return smooth(t / Math.max(1e-6, item.attack))
  if (t < item.attack + item.hold) return 1
  return 1 - smooth((t - item.attack - item.hold) / item.release)
}

function smooth(t: number): number {
  const x = Math.max(0, Math.min(1, t))
  return x * x * (3 - 2 * x)
}
