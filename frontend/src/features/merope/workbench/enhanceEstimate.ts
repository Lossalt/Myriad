import type { TurnKeyformsStatus } from '../turnKeyformsApi'

/**
 * What an enhancement job does, step by step, and about how long is left.
 * The server reports a stage and which part of the work it belongs to
 * (`phase`: a figure's decomposing and its head's are both `decomposing`);
 * the steps and their usual lengths are known here.
 */
export type EnhancePlan = 'bust' | 'figure' | 'figurePlain' | 'refit'

export type EnhanceStepKey =
  | 'upscale'
  | 'figureDecompose'
  | 'plainDecompose'
  | 'headGenerate'
  | 'headDecompose'
  | 'headFit'
  | 'stitch'
  | 'body'
  | 'refit'

export type EnhanceStepState = 'pending' | 'started' | 'completed' | 'failed'

export interface EnhanceStep {
  key: EnhanceStepKey
  state: EnhanceStepState
  /** Units done and in all (drawings, decompositions), where the step counts them. */
  done: number
  total: number
  /** Made alongside the others (a figure's body keys). */
  parallel: boolean
}

export interface EnhanceEstimate {
  steps: EnhanceStep[]
  elapsedSeconds: number
  /** About how long is left, seconds. */
  remainingSeconds: number
  /** Share done, 0…1, by expected time. */
  fraction: number
}

/**
 * Usual lengths, seconds, measured on the Space (2026-10): a ZeroGPU
 * decomposition 210–245, a fit on its CPUs 390–660, a drawing 35–45.
 */
export const ENHANCE_SECONDS = {
  upscale: 60,
  decompose: 230,
  generate: 45,
  fit: 600,
  stitch: 60,
  bodyFit: 270,
} as const

interface StepModel {
  key: EnhanceStepKey
  /** Seconds per unit. */
  unit: number
  units: number
}

const BODY_MOTION = {
  generate: 2,
  decompose: 2,
} as const
/** The body's turn, then its weight shift: each drawn twice, decomposed twice, fitted once. */
const BODY_MOTIONS = ['turn', 'weight'] as const

const PLANS: Record<EnhancePlan, StepModel[]> = {
  bust: [
    { key: 'headGenerate', unit: ENHANCE_SECONDS.generate, units: 4 },
    { key: 'headDecompose', unit: ENHANCE_SECONDS.decompose, units: 5 },
    { key: 'headFit', unit: ENHANCE_SECONDS.fit, units: 1 },
  ],
  figure: [
    { key: 'upscale', unit: ENHANCE_SECONDS.upscale, units: 1 },
    { key: 'figureDecompose', unit: ENHANCE_SECONDS.decompose, units: 3 },
    { key: 'headGenerate', unit: ENHANCE_SECONDS.generate, units: 4 },
    { key: 'headDecompose', unit: ENHANCE_SECONDS.decompose, units: 5 },
    { key: 'headFit', unit: ENHANCE_SECONDS.fit, units: 1 },
    { key: 'stitch', unit: ENHANCE_SECONDS.stitch, units: 1 },
  ],
  figurePlain: [
    { key: 'upscale', unit: ENHANCE_SECONDS.upscale, units: 1 },
    { key: 'plainDecompose', unit: ENHANCE_SECONDS.decompose, units: 4 },
    { key: 'stitch', unit: ENHANCE_SECONDS.stitch, units: 1 },
  ],
  refit: [{ key: 'refit', unit: ENHANCE_SECONDS.fit, units: 1 }],
}

/** The step a status is in, for a plan; null once done (or not one of its stages). */
function currentStep(plan: EnhancePlan, status: TurnKeyformsStatus): EnhanceStepKey | null {
  const { phase, stage } = status
  if (stage === 'done' || stage === 'failed') return null
  if (plan === 'refit') return 'refit'
  if (plan === 'bust') {
    return stage === 'generating' ? 'headGenerate' : stage === 'decomposing' ? 'headDecompose' : 'headFit'
  }
  if (phase === 'head') {
    return stage === 'generating' ? 'headGenerate' : stage === 'decomposing' ? 'headDecompose' : 'headFit'
  }
  if (phase === 'body') return 'body'
  if (stage === 'upscaling') return 'upscale'
  if (stage === 'stitching') return 'stitch'
  return plan === 'figure' ? 'figureDecompose' : 'plainDecompose'
}

/** The body's units left, by kind, from where it stands. */
function bodyLeft(status: TurnKeyformsStatus): { generate: number; decompose: number; fit: number } {
  const left = { generate: 0, decompose: 0, fit: 0 }
  const body = status.body
  const started = body ? BODY_MOTIONS.indexOf(body.motion) : -1
  BODY_MOTIONS.forEach((motion, index) => {
    if (body && (body.stage === 'failed' || index < started)) return
    if (!body || index > started) {
      left.generate += BODY_MOTION.generate
      left.decompose += BODY_MOTION.decompose
      left.fit += 1
      return
    }
    if (body.stage === 'generating') {
      left.generate += Math.max(0, body.total - body.done)
      left.decompose += BODY_MOTION.decompose
      left.fit += 1
    } else if (body.stage === 'decomposing') {
      left.decompose += Math.max(0, body.total - body.done)
      left.fit += 1
    } else if (body.stage === 'fitting') {
      left.fit += 1
    }
  })
  return left
}

function bodyState(status: TurnKeyformsStatus, headDone: boolean): EnhanceStepState {
  const body = status.body
  if (status.stage === 'done') return 'completed'
  if (body?.stage === 'failed') return 'failed'
  if (body?.motion === 'weight' && body.stage === 'done') return 'completed'
  if (body || headDone) return 'started'
  return 'pending'
}

/**
 * The steps of `plan` as `status` has them, and about how long is left.
 * `unitSeconds`: how long the current unit (drawing, decomposition) has run,
 * as far as the page has seen.
 */
export function estimateEnhance(
  plan: EnhancePlan,
  status: TurnKeyformsStatus,
  unitSeconds = 0,
): EnhanceEstimate {
  const models = PLANS[plan]
  const current = currentStep(plan, status)
  const finished = status.stage === 'done'
  const at = current ? models.findIndex((model) => model.key === current) : finished ? models.length : -1
  // Waiting on the body: every step of the head's way is done.
  const headIndex = current === 'body' ? models.length : at
  const steps: EnhanceStep[] = models.map((model, index) => {
    const here = index === headIndex
    return {
      key: model.key,
      state: status.stage === 'failed' && here ? 'failed' : index < headIndex ? 'completed' : here ? 'started' : 'pending',
      done: here ? Math.min(status.done, status.total) : index < headIndex ? model.units : 0,
      total: here ? Math.max(status.total, 1) : model.units,
      parallel: false,
    }
  })
  let headSeconds = 0
  models.forEach((model, index) => {
    if (index < headIndex) return
    if (index === headIndex) {
      const left = Math.max(0, status.total - status.done) * model.unit - unitSeconds
      headSeconds += Math.max(left, model.unit * 0.1)
    } else {
      headSeconds += model.units * model.unit
    }
  })
  let bodySeconds = 0
  if (plan === 'figure') {
    const left = bodyLeft(status)
    bodySeconds = left.generate * ENHANCE_SECONDS.generate +
      left.decompose * ENHANCE_SECONDS.decompose +
      left.fit * ENHANCE_SECONDS.bodyFit
    // The Space decomposes one at a time: the body's wait in the head's queue.
    const headDecomposing = models.some((model, index) =>
      index >= headIndex && (model.key === 'figureDecompose' || model.key === 'headDecompose'))
    if (headDecomposing) headSeconds += left.decompose * ENHANCE_SECONDS.decompose
    const body = status.body
    steps.push({
      key: 'body',
      state: bodyState(status, headIndex >= models.length),
      done: body ? Math.min(body.done, body.total) : 0,
      total: body ? Math.max(body.total, 1) : 1,
      parallel: true,
    })
  }
  const remainingSeconds = finished ? 0 : Math.round(Math.max(headSeconds, bodySeconds))
  const elapsedSeconds = Math.max(0, status.seconds)
  const whole = elapsedSeconds + remainingSeconds
  return {
    steps,
    elapsedSeconds,
    remainingSeconds,
    fraction: finished ? 1 : whole > 0 ? Math.min(0.99, elapsedSeconds / whole) : 0,
  }
}

/** The plan a running job follows, from where it is reported and what the page started. */
export function enhancePlanOf(status: TurnKeyformsStatus, figure: boolean, enhanced: boolean): EnhancePlan | null {
  if (!status.phase) return null
  if (status.phase === 'refit') return 'refit'
  if (status.phase === 'load') return null
  if (status.phase === 'bust') return 'bust'
  return figure && enhanced ? 'figure' : 'figurePlain'
}
