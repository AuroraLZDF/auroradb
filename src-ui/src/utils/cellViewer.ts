/**
 * 单元格查看器用的纯工具函数：Base64 解码、图片文件头识别、十六进制视图、
 * JSON 格式化与渲染上限判定。
 *
 * 这里不依赖 Vue 与 Tauri，便于单测覆盖。
 */

/** 图片预览体积上限：超过后只给十六进制，避免解码大图卡住界面 */
export const IMAGE_PREVIEW_MAX_BYTES = 10 * 1024 * 1024

/** 十六进制视图默认展示的字节数（约 256 行） */
export const HEX_PREVIEW_BYTES = 4096

/** 纯文本一次性渲染的字符上限，超出部分只提示不渲染（复制/另存为仍用完整内容） */
export const TEXT_RENDER_MAX_CHARS = 200_000

/** JSON 解析上限：超大文本不尝试解析，直接按原文展示 */
export const JSON_PARSE_MAX_CHARS = 2 * 1024 * 1024

export type ImageKind = 'png' | 'jpeg' | 'gif' | 'webp' | 'bmp'

export interface ImageSignature {
  kind: ImageKind
  mime: string
  extension: string
}

const IMAGE_SIGNATURES: ImageSignature[] = [
  { kind: 'png', mime: 'image/png', extension: 'png' },
  { kind: 'jpeg', mime: 'image/jpeg', extension: 'jpg' },
  { kind: 'gif', mime: 'image/gif', extension: 'gif' },
  { kind: 'webp', mime: 'image/webp', extension: 'webp' },
  { kind: 'bmp', mime: 'image/bmp', extension: 'bmp' },
]

function matches(bytes: Uint8Array, offset: number, signature: number[]): boolean {
  if (bytes.length < offset + signature.length) return false
  return signature.every((value, index) => bytes[offset + index] === value)
}

/** 按文件头识别常见图片格式，识别不出返回 null */
export function sniffImageType(bytes: Uint8Array): ImageSignature | null {
  if (matches(bytes, 0, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) {
    return IMAGE_SIGNATURES[0]!
  }
  if (matches(bytes, 0, [0xff, 0xd8, 0xff])) {
    return IMAGE_SIGNATURES[1]!
  }
  if (matches(bytes, 0, [0x47, 0x49, 0x46, 0x38])) {
    return IMAGE_SIGNATURES[2]!
  }
  // RIFF....WEBP
  if (matches(bytes, 0, [0x52, 0x49, 0x46, 0x46]) && matches(bytes, 8, [0x57, 0x45, 0x42, 0x50])) {
    return IMAGE_SIGNATURES[3]!
  }
  if (matches(bytes, 0, [0x42, 0x4d])) {
    return IMAGE_SIGNATURES[4]!
  }
  return null
}

/** Base64 文本转字节；非法 Base64 返回 null 而不是抛错 */
export function base64ToBytes(base64: string): Uint8Array | null {
  const normalized = base64.replace(/\s+/g, '')
  if (!normalized) return new Uint8Array(0)
  try {
    const binary = atob(normalized)
    const bytes = new Uint8Array(binary.length)
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
    return bytes
  } catch {
    return null
  }
}

function hexByte(value: number): string {
  return value.toString(16).padStart(2, '0')
}

/**
 * 生成十六进制视图：每行 16 字节，左侧偏移、中间十六进制、右侧可打印 ASCII。
 * 仅格式化前 `maxBytes` 字节。
 */
export function toHexDump(bytes: Uint8Array, maxBytes: number = HEX_PREVIEW_BYTES): string {
  const limit = Math.min(bytes.length, Math.max(0, maxBytes))
  const lines: string[] = []
  for (let offset = 0; offset < limit; offset += 16) {
    const chunk = bytes.subarray(offset, Math.min(offset + 16, limit))
    const hex = Array.from(chunk, hexByte)
    const left = hex.slice(0, 8).join(' ').padEnd(23, ' ')
    const right = hex.slice(8).join(' ').padEnd(23, ' ')
    const ascii = Array.from(chunk, byte =>
      byte >= 0x20 && byte <= 0x7e ? String.fromCharCode(byte) : '.'
    ).join('')
    lines.push(`${offset.toString(16).padStart(8, '0')}  ${left} ${right} |${ascii}|`)
  }
  return lines.join('\n')
}

export interface JsonFormatResult {
  /** 是否为合法 JSON（仅在以 { 或 [ 开头时才尝试解析） */
  isJson: boolean
  /** isJson 为 true 时是格式化后的文本，否则原样返回 */
  text: string
  /** 解析出的值，供 JSON 树视图使用 */
  value?: unknown
}

/**
 * 尝试把文本当 JSON 格式化。
 * 非对象/数组、解析失败、超长文本一律视为普通文本并原样返回（回退原文）。
 */
export function formatJsonText(
  text: string,
  options: { indent?: number; maxChars?: number } = {}
): JsonFormatResult {
  const indent = options.indent ?? 2
  const maxChars = options.maxChars ?? JSON_PARSE_MAX_CHARS
  const trimmed = text.trim()
  if (!trimmed || text.length > maxChars) return { isJson: false, text }
  if (!trimmed.startsWith('{') && !trimmed.startsWith('[')) return { isJson: false, text }
  try {
    const value: unknown = JSON.parse(trimmed)
    return { isJson: true, text: JSON.stringify(value, null, indent), value }
  } catch {
    return { isJson: false, text }
  }
}

/** 按字符上限截断文本，返回展示片段与是否被截断 */
export function truncateForRender(
  text: string,
  maxChars: number = TEXT_RENDER_MAX_CHARS
): { text: string; truncated: boolean } {
  if (text.length <= maxChars) return { text, truncated: false }
  return { text: text.slice(0, maxChars), truncated: true }
}

/** 人类可读的体积文案 */
export function formatByteSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
}

/** 生成带扩展名的默认文件名：列名 → 文件名前缀 */
export function defaultFileName(columnName: string, extension: string): string {
  const base = (columnName || 'cell').replace(/[\\/:*?"<>|\s]+/g, '_').slice(0, 60) || 'cell'
  return `${base}.${extension}`
}
