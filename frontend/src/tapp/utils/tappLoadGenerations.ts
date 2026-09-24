export type LatestLoad<T> = { stale: false; value: T } | { stale: true }

/**
 * Per-TAPP load generations. Each `tapp:updated` begins a new generation; a
 * resource load that settles after a newer one began is stale and must not
 * reach the window, whether it resolved or failed.
 */
export class TappLoadGenerations {
  private readonly generations = new Map<string, number>()

  /** Start a load that supersedes every earlier one for this TAPP. */
  begin(tappId: string): number {
    const next = this.current(tappId) + 1
    this.generations.set(tappId, next)
    return next
  }

  /** Observe without superseding, for loads that must yield to later updates. */
  current(tappId: string): number {
    return this.generations.get(tappId) ?? 0
  }

  isCurrent(tappId: string, generation: number): boolean {
    return this.current(tappId) === generation
  }

  async settle<T>(tappId: string, generation: number, work: Promise<T>): Promise<LatestLoad<T>> {
    try {
      const value = await work
      return this.isCurrent(tappId, generation) ? { stale: false, value } : { stale: true }
    } catch (error) {
      if (!this.isCurrent(tappId, generation)) return { stale: true }
      throw error
    }
  }
}
