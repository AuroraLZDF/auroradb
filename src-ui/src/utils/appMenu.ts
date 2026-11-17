import { Menu, MenuItem, PredefinedMenuItem, Submenu } from '@tauri-apps/api/menu'
import { t } from '@/i18n'

/** 菜单动作分发回调，由 App.vue 注册（转发到 commandRegistry / UI 状态） */
type MenuActionHandler = (id: string) => void
let menuActionHandler: MenuActionHandler | null = null

export function setMenuActionHandler(handler: MenuActionHandler) {
  menuActionHandler = handler
}

function dispatch(id: string) {
  menuActionHandler?.(id)
}

/** 菜单句柄：菜单实体在 Rust 侧存活，前端必须持有引用防止句柄被回收 */
let appMenu: Menu | null = null

/**
 * 构建原生应用菜单（文件/编辑/视图/帮助）。
 * 前端构建以便复用 i18n 文案；语言切换时重新调用本函数即可整体重建。
 *
 * 平台差异：macOS 的菜单显示在系统顶部菜单栏，不占窗口空间，保留原生菜单；
 * Linux/Windows 的原生菜单会在窗口内顶部额外占一行，与应用内导航栏重复，
 * 因此不启用——相关入口已由应用内头部（新建查询按钮 / 工具 / 设置下拉）提供，
 * 编辑剪贴板快捷键由 WebKitGTK/WebView 在输入框内原生处理，不依赖菜单。
 *
 * 加速键说明：原生菜单加速键由操作系统菜单层拦截（macOS NSMenu），
 * 不会透传到 WebKit，因此与 App.vue 的 webview 全局快捷键不会双重触发。
 */
export async function buildAppMenu(): Promise<void> {
  if (!navigator.userAgent.includes('Mac')) return

  // 文件：新建入口 + 退出
  const fileMenu = await Submenu.new({ id: 'menu-file', text: t('app.menuFile') })
  await fileMenu.append(await MenuItem.new({
    id: 'new-query',
    text: t('app.menuNewQuery'),
    accelerator: 'CmdOrCtrl+N',
    action: () => dispatch('new-query'),
  }))
  await fileMenu.append(await MenuItem.new({
    id: 'new-console',
    text: t('app.menuNewConsole'),
    accelerator: 'CmdOrCtrl+Shift+T',
    action: () => dispatch('new-console'),
  }))
  await fileMenu.append(await PredefinedMenuItem.new({ item: 'Separator' }))
  await fileMenu.append(await PredefinedMenuItem.new({ item: 'Quit' }))

  // 编辑：标准文本操作（macOS 系统惯例，由系统实现剪贴板行为）
  const editMenu = await Submenu.new({ id: 'menu-edit', text: t('app.menuEdit') })
  await editMenu.append(await PredefinedMenuItem.new({ item: 'Undo' }))
  await editMenu.append(await PredefinedMenuItem.new({ item: 'Redo' }))
  await editMenu.append(await PredefinedMenuItem.new({ item: 'Separator' }))
  await editMenu.append(await PredefinedMenuItem.new({ item: 'Cut' }))
  await editMenu.append(await PredefinedMenuItem.new({ item: 'Copy' }))
  await editMenu.append(await PredefinedMenuItem.new({ item: 'Paste' }))
  await editMenu.append(await PredefinedMenuItem.new({ item: 'SelectAll' }))

  // 视图：命令面板 / 主题 / 全屏
  const viewMenu = await Submenu.new({ id: 'menu-view', text: t('app.menuView') })
  await viewMenu.append(await MenuItem.new({
    id: 'command-palette',
    text: t('app.menuCommandPalette'),
    accelerator: 'CmdOrCtrl+K',
    action: () => dispatch('command-palette'),
  }))
  await viewMenu.append(await MenuItem.new({
    id: 'toggle-theme',
    text: t('app.menuToggleTheme'),
    accelerator: 'CmdOrCtrl+Shift+L',
    action: () => dispatch('toggle-theme'),
  }))
  await viewMenu.append(await PredefinedMenuItem.new({ item: 'Separator' }))
  await viewMenu.append(await PredefinedMenuItem.new({ item: 'Fullscreen' }))

  // 帮助：关于
  const helpMenu = await Submenu.new({ id: 'menu-help', text: t('app.menuHelp') })
  await helpMenu.append(await MenuItem.new({
    id: 'about',
    text: t('app.menuAbout'),
    action: () => dispatch('about'),
  }))

  const menu = await Menu.new({ items: [fileMenu, editMenu, viewMenu, helpMenu] })
  appMenu = menu
  // 仅 macOS 走到这里：菜单挂在应用上（系统顶部菜单栏）
  await menu.setAsAppMenu()
}
