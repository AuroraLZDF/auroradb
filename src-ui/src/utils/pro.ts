import type { Component } from 'vue'

/**
 * 专业版界面注册表。
 *
 * `src/pro` 只在私有主仓中存在，公开仓导出时会被剔除；这里用 glob 扫描该目录，
 * 目录缺失时得到空表，免费构建无需替换任何文件即可编译。入口仍由 App.vue
 * 通过 license store 统一提示升级。
 */
const modules = import.meta.glob<{ default: Component }>('../pro/*.vue', { eager: true })

function pick(name: string): Component | null {
  return modules[`../pro/${name}.vue`]?.default ?? null
}

export const ProUi = {
  SchemaDiff: pick('SchemaDiff'),
  MonitorPanel: pick('MonitorPanel'),
  BackupDialog: pick('BackupDialog'),
  ErDiagram: pick('ErDiagram'),
  GlobalSearchDialog: pick('GlobalSearchDialog'),
  ExplainTree: pick('ExplainTree'),
}

/** 当前构建是否包含专业版界面（免费构建为 false） */
export const hasProUi = Object.values(ProUi).every((component) => component !== null)
