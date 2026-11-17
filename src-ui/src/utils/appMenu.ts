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
 * 加速键说明：原生菜单加速键由操作系统菜单层拦截（macOS NSMenu / Windows / GTK），
 * 不会透传到 WebKit，因此与 App.vue 的 webview 全局快捷键不会双重触发。
 */
export async function buildAppMenu(): Promise<void> {
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
  // Linux 上 predefined Quit 不生效，补一个自定义退出项（Ctrl+Q）
  if (navigator.userAgent.includes('Linux')) {
    await fileMenu.append(await MenuItem.new({
      id: 'quit',
      text: t('app.menuQuit'),
      accelerator: 'CmdOrCtrl+Q',
      action: () => dispatch('quit'),
    }))
  }

  // 编辑：标准文本操作（Linux 下 WebKitGTK 无系统默认编辑菜单，必须显式提供）
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
  // macOS 菜单挂在应用上，Windows/Linux 菜单挂在窗口上
  const isMac = navigator.userAgent.includes('Mac')
  if (isMac) {
    await menu.setAsAppMenu()
  } else {
    await menu.setAsWindowMenu()
  }
}
