/** 评论接口没有单独的审核状态。公开博客只显示 `is_public` 的评论。 */

export function publicThread<T extends { is_public?: boolean }>(rows: readonly T[]): T[] {
  return rows.filter((row) => row.is_public === true)
}
