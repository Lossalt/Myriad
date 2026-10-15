export function workbenchPendingReviews<T extends { status: string }>(
  rows: readonly T[],
): T[] {
  return rows.filter((row) => row.status === 'pending')
}
