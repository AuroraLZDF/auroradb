/**
 * Tab 与连接绑定的纯函数工具。
 *
 * 背景：SQL / 控制台 Tab 各自绑定打开时的连接。多连接同时在线时，
 * 激活某个 Tab 必须把全局当前连接切回它自己绑定的连接，否则 header、
 * 元数据、对象树以及后续执行都可能落在错误的连接（库）上。
 */

/** 参与连接解析的最小 Tab 形状：只需要可能带 connectionId（null 视同未绑定） */
export interface TabConnectionRef {
  connectionId?: string | null
}

/**
 * 解析 Tab 最终使用的连接 id：Tab 自身绑定的连接优先，
 * 绑定为空（旧草稿 / 未绑定的 Tab）时回退全局当前连接。
 * 两者都为空返回空串，表示当前没有可用连接。
 */
export function resolveTabConnectionId(
  tab: TabConnectionRef | null | undefined,
  currentId: string | null | undefined,
): string {
  return tab?.connectionId?.trim() || currentId?.trim() || ''
}

/** 从一组 tab id 中解析指定前缀的最大数字后缀；不符合 `前缀 + 数字` 的 id 直接忽略 */
export function maxTabIndex(prefix: string, existingIds: Iterable<string>): number {
  let max = 0
  for (const id of existingIds) {
    if (!id.startsWith(prefix)) continue
    const suffix = id.slice(prefix.length)
    if (!/^\d+$/.test(suffix)) continue
    max = Math.max(max, Number(suffix))
  }
  return max
}

/**
 * 恢复草稿（或任意路径新增 Tab）后计数器应推进到的值：已用最大序号 +1。
 * 保证之后新建的 Tab 不会与恢复的 id 撞号。
 */
export function nextTabCounter(prefix: string, existingIds: Iterable<string>): number {
  return maxTabIndex(prefix, existingIds) + 1
}

/**
 * 分配下一个可用的 tab id：前缀 + 已用最大序号 +1。
 * 按现有 id 的最大序号递增，返回值一定不与 existingIds 冲突。
 */
export function nextSqlTabId(prefix: string, existingIds: Iterable<string>): string {
  return `${prefix}${nextTabCounter(prefix, existingIds)}`
}
