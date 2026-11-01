/**
 * `.sql` 文件打开相关的纯函数（文件关联 / 拖拽共用）
 */

/** 是否是 SQL 文件（扩展名大小写不敏感） */
export function isSqlFilePath(path: string): boolean {
  return /\.sql$/i.test(path.trim())
}

/** 从完整路径取文件名，兼容 `/` 与 `\` 两种分隔符 */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/)
  return parts[parts.length - 1] || path
}
