import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

export interface Command {
  /** 唯一 id，用于注销 */
  id: string
  /** 命令名（中文显示） */
  title: string
  /** 分组/分类（可选） */
  category?: string
  /**
   * 快捷键显示文本（可选）。
   *
   * 只放「确实已绑定」的键：全局键由 App.vue 的 onGlobalKeydown 处理，
   * 组件内键由对应组件处理。没有绑定就不要填，否则会误导用户。
   */
  shortcut?: string
  /** 是否在当前上下文可用；不传则始终可用 */
  available?: () => boolean
  /** 执行动作 */
  action: () => void
}

/**
 * 命令注册表（Command Palette / Ctrl+K 数据源）
 *
 * 各组件在 onMounted 时通过 register 注册自己的命令，
 * onUnmounted 时通过 unregister 清理；MainContent/App 层负责全局弹窗 UI。
 *
 * 首版只做字符串过滤（startsWith + includes），不做 fuzzy search。
 */
export const useCommandRegistryStore = defineStore('commandRegistry', () => {
  const commands = ref<Command[]>([])
  const open = ref(false)
  const query = ref('')
  const activeIndex = ref(0)

  function register(cmd: Command) {
    // 同 id 覆盖，避免重复注册
    unregister(cmd.id)
    commands.value.push(cmd)
  }

  function unregister(id: string) {
    const idx = commands.value.findIndex(c => c.id === id)
    if (idx !== -1) commands.value.splice(idx, 1)
  }

  function reset() {
    commands.value = []
  }

  function updateCommandTitle(id: string, newTitle: string) {
    const cmd = commands.value.find(c => c.id === id)
    if (cmd) cmd.title = newTitle
  }

  const filtered = computed(() => {
    const q = query.value.trim().toLowerCase()
    return commands.value
      .filter(c => !c.available || c.available())
      .filter(c => {
        if (!q) return true
        const title = c.title.toLowerCase()
        const cat = c.category?.toLowerCase() ?? ''
        return title.includes(q) || cat.includes(q)
      })
  })

  function openPalette() {
    open.value = true
    query.value = ''
    activeIndex.value = 0
  }

  function closePalette() {
    open.value = false
    query.value = ''
  }

  /** 执行指定命令；点击列表项时必须传被点击项，不能依赖 activeIndex */
  function runCommand(cmd: Command) {
    cmd.action()
    closePalette()
  }

  function runActive() {
    const list = filtered.value
    if (list.length === 0) return
    const cmd = list[activeIndex.value]
    if (cmd) runCommand(cmd)
  }

  /**
   * 按 id 执行命令，供全局快捷键复用同一套「可用性判断 + 执行」逻辑。
   * 返回 false 表示命令不存在或当前不可用（调用方据此决定是否 preventDefault）。
   */
  function execute(id: string): boolean {
    const cmd = commands.value.find(c => c.id === id)
    if (!cmd) return false
    if (cmd.available && !cmd.available()) return false
    runCommand(cmd)
    return true
  }

  function moveActive(dir: -1 | 1) {
    const len = filtered.value.length
    if (len === 0) return
    activeIndex.value = (activeIndex.value + dir + len) % len
  }

  return {
    commands,
    open,
    query,
    activeIndex,
    filtered,
    register,
    unregister,
    reset,
    updateCommandTitle,
    openPalette,
    closePalette,
    runCommand,
    runActive,
    execute,
    moveActive,
  }
})
