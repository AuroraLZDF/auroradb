import { ref, computed, onMounted, onUnmounted } from 'vue'

// 断点定义
const breakpoints = {
  '2xs': 480,
  'xs': 640,
  'sm': 768,
  'md': 1024,
  'lg': 1280,
  'xl': 1536
}

type Breakpoint = keyof typeof breakpoints

export function useBreakpoints() {
  const width = ref(window.innerWidth)
  const height = ref(window.innerHeight)

  // 当前断点
  const current = computed<Breakpoint>(() => {
    if (width.value >= breakpoints.xl) return 'xl'
    if (width.value >= breakpoints.lg) return 'lg'
    if (width.value >= breakpoints.md) return 'md'
    if (width.value >= breakpoints.sm) return 'sm'
    if (width.value >= breakpoints.xs) return 'xs'
    return '2xs'
  })

  // 断点判断
  const is2xs = computed(() => width.value < breakpoints.xs)
  const isXs = computed(() => width.value < breakpoints.sm)
  const isSm = computed(() => width.value < breakpoints.md)
  const isMd = computed(() => width.value < breakpoints.lg)
  const isLg = computed(() => width.value < breakpoints.xl)
  const isXl = computed(() => width.value >= breakpoints.xl)

  // 设备类型判断
  const isMobile = computed(() => width.value < breakpoints.sm)  // < 768px
  const isTablet = computed(() => width.value >= breakpoints.sm && width.value < breakpoints.lg)  // 768-1280px
  const isDesktop = computed(() => width.value >= breakpoints.lg)  // >= 1280px

  // 小屏判断（用于抽屉式面板）
  const isSmallScreen = computed(() => width.value < breakpoints.md)  // < 1024px

  // 监听窗口变化
  let resizeTimer: ReturnType<typeof setTimeout> | null = null

  function updateSize() {
    width.value = window.innerWidth
    height.value = window.innerHeight
  }

  function handleResize() {
    if (resizeTimer) clearTimeout(resizeTimer)
    resizeTimer = setTimeout(updateSize, 100)
  }

  onMounted(() => {
    window.addEventListener('resize', handleResize)
    updateSize()
  })

  onUnmounted(() => {
    window.removeEventListener('resize', handleResize)
    if (resizeTimer) clearTimeout(resizeTimer)
  })

  return {
    width,
    height,
    current,
    is2xs,
    isXs,
    isSm,
    isMd,
    isLg,
    isXl,
    isMobile,
    isTablet,
    isDesktop,
    isSmallScreen
  }
}

// 导出断点值供 CSS 使用
export { breakpoints }
