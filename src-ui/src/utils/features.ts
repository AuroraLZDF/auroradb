import type { Component } from 'vue'

/**
 * 功能界面注册表。
 *
 * 各功能界面统一放在 `src/features` 目录；这里用 glob 扫描该目录并按需加载。
 * 历史上该目录名为 `src/pro`（商业授权时代的开放核心边界），2026-11-04 起全部功能
 * 免费开源，目录收敛为 `features`，注册表仅保留统一入口的加载职责。
 */
const modules = import.meta.glob<{ default: Component }>('../features/*.vue', { eager: true })

function pick(name: string): Component | null {
  return modules[`../features/${name}.vue`]?.default ?? null
}

export const FeatureUi = {
  SchemaDiff: pick('SchemaDiff'),
  MonitorPanel: pick('MonitorPanel'),
  BackupDialog: pick('BackupDialog'),
  ErDiagram: pick('ErDiagram'),
  GlobalSearchDialog: pick('GlobalSearchDialog'),
  ExplainTree: pick('ExplainTree'),
}
