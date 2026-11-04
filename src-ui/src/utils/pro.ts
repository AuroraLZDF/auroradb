import type { Component } from 'vue'

/**
 * 功能界面注册表。
 *
 * 各功能界面统一从 `src/pro` 目录按需加载；这里用 glob 扫描该目录，
 * 目录缺失时得到空表，公开构建无需替换任何文件即可编译。
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
