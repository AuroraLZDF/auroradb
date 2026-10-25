import { computed, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue'

interface VirtualRowsOptions {
  /** 单行高度（px），用于推算可视区间 */
  rowHeight?: number
  /** 可视区上下额外渲染的行数 */
  overscan?: number
  /** 超过该行数才启用虚拟滚动 */
  threshold?: number
}

/**
 * 固定行高虚拟滚动：只渲染可视区（含 overscan）内的行，
 * 上下用等高 spacer 撑起滚动条，索引仍对应原始行号。
 */
export function useVirtualRows(
  container: Ref<HTMLElement | null>,
  count: () => number,
  options: VirtualRowsOptions = {}
) {
  const rowHeight = ref(options.rowHeight ?? 36)
  const overscan = options.overscan ?? 8
  const threshold = options.threshold ?? 100

  const scrollTop = ref(0)
  const viewportHeight = ref(0)

  const enabled = computed(() => count() > threshold)

  const start = computed(() => {
    const total = count()
    if (!enabled.value || total === 0) return 0
    const first = Math.floor(scrollTop.value / rowHeight.value) - overscan
    return Math.min(Math.max(first, 0), total - 1)
  })

  const end = computed(() => {
    const total = count()
    if (!enabled.value) return total
    const last = Math.ceil((scrollTop.value + viewportHeight.value) / rowHeight.value) + overscan
    return Math.min(Math.max(last, start.value + 1), total)
  })

  const paddingTop = computed(() => (enabled.value ? start.value * rowHeight.value : 0))
  const paddingBottom = computed(() =>
    enabled.value ? Math.max(0, (count() - end.value) * rowHeight.value) : 0
  )

  let frame = 0

  function syncScroll() {
    frame = 0
    const el = container.value
    if (!el) return
    scrollTop.value = el.scrollTop
    if (el.clientHeight > 0) viewportHeight.value = el.clientHeight
  }

  function scheduleSync() {
    if (frame !== 0) return
    frame = requestAnimationFrame(syncScroll)
  }

  function onScroll() {
    scheduleSync()
  }

  // 用真实渲染出的行列校准行高，避免 spacer 与真实行高不一致导致滚动条抖动
  function measureRowHeight() {
    const el = container.value
    if (!el) return
    const row = el.querySelector('tbody tr:not(.virtual-spacer):not(.empty-row)')
    const measured = row?.getBoundingClientRect().height ?? 0
    if (measured > 0 && Math.abs(measured - rowHeight.value) > 0.5) {
      rowHeight.value = measured
    }
  }

  let observer: ResizeObserver | null = null

  function observe(el: HTMLElement | null) {
    observer?.disconnect()
    observer = null
    if (!el) return
    scrollTop.value = el.scrollTop
    viewportHeight.value = el.clientHeight
    if (typeof ResizeObserver !== 'undefined') {
      observer = new ResizeObserver(() => {
        viewportHeight.value = el.clientHeight
      })
      observer.observe(el)
    }
  }

  watch(container, observe, { flush: 'post' })

  // 数据量变化后等 DOM 更新完再同步滚动位置与行高
  watch(
    count,
    () => {
      if (frame !== 0) return
      frame = requestAnimationFrame(() => {
        syncScroll()
        measureRowHeight()
      })
    },
    { flush: 'post' }
  )

  onMounted(() => observe(container.value))

  onBeforeUnmount(() => {
    if (frame !== 0) cancelAnimationFrame(frame)
    observer?.disconnect()
    observer = null
  })

  return {
    enabled,
    rowHeight,
    start,
    end,
    paddingTop,
    paddingBottom,
    onScroll,
    measureRowHeight,
  }
}
