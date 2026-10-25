import { ref, computed } from 'vue'
import { defineStore } from 'pinia'

type Theme = 'light' | 'dark' | 'auto'

export const useThemeStore = defineStore('theme', () => {
  // 检测系统偏好
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches

  // 从 localStorage 读取保存的主题设置
  const savedTheme = localStorage.getItem('app-theme') as Theme | null

  // 初始化：如果没有保存过，根据系统偏好；如果有保存，使用保存的值
  const initialTheme = savedTheme === 'dark' || savedTheme === 'light'
    ? savedTheme
    : (prefersDark ? 'dark' : 'light')

  const theme = ref<Theme>(initialTheme)

  // 计算实际应用的主题
  const effectiveTheme = computed(() => theme.value)

  // 应用主题到文档
  function applyTheme() {
    const html = document.documentElement
    html.classList.remove('light', 'dark')
    html.classList.add(theme.value)
    html.setAttribute('data-theme', theme.value)
  }

  // 设置主题
  function setTheme(newTheme: Theme) {
    theme.value = newTheme
    localStorage.setItem('app-theme', newTheme)
    applyTheme()
  }

  // 切换主题（只在 light 和 dark 之间切换）
  function toggleTheme() {
    const nextTheme = theme.value === 'dark' ? 'light' : 'dark'
    setTheme(nextTheme)
  }

  // 初始化
  function init() {
    applyTheme()
  }

  return {
    theme,
    effectiveTheme,
    setTheme,
    toggleTheme,
    init,
  }
})
