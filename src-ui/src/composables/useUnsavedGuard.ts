import { onUnmounted, watch, type Ref } from 'vue'

/** 记录各来源（数据表编辑、表设计器等）是否存在未保存内容 */
const dirtySources = new Set<string>()

/** 标记某个来源是否存在未保存内容 */
export function setUnsavedSource(key: string, dirty: boolean) {
  if (dirty) dirtySources.add(key)
  else dirtySources.delete(key)
}

/** 是否存在任何未保存内容，供关闭窗口前的兜底提示使用 */
export function hasUnsavedSources(): boolean {
  return dirtySources.size > 0
}

/** 组件内注册未保存状态，组件卸载时自动清除 */
export function useUnsavedGuard(key: string, dirty: Ref<boolean>) {
  watch(dirty, value => setUnsavedSource(key, value), { immediate: true })
  onUnmounted(() => setUnsavedSource(key, false))
}
