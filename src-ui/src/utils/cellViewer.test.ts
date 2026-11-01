import { describe, expect, it } from 'vitest'
import {
  base64ToBytes,
  defaultFileName,
  formatByteSize,
  formatJsonText,
  sniffImageType,
  toHexDump,
  truncateForRender,
} from './cellViewer'

describe('sniffImageType', () => {
  it('识别 PNG / JPEG / GIF / WebP / BMP 文件头', () => {
    expect(sniffImageType(new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))?.kind)
      .toBe('png')
    expect(sniffImageType(new Uint8Array([0xff, 0xd8, 0xff, 0xe0]))?.kind).toBe('jpeg')
    expect(sniffImageType(new Uint8Array([0x47, 0x49, 0x46, 0x38, 0x39, 0x61]))?.kind).toBe('gif')
    expect(sniffImageType(new Uint8Array([
      0x52, 0x49, 0x46, 0x46, 0x00, 0x00, 0x00, 0x00,
      0x57, 0x45, 0x42, 0x50,
    ]))?.kind).toBe('webp')
    expect(sniffImageType(new Uint8Array([0x42, 0x4d, 0x00]))?.kind).toBe('bmp')
  })

  it('非图片内容返回 null', () => {
    expect(sniffImageType(new TextEncoder().encode('hello'))).toBeNull()
    expect(sniffImageType(new Uint8Array(0))).toBeNull()
  })

  it('文件头不足时不会越界读', () => {
    expect(sniffImageType(new Uint8Array([0x89, 0x50]))).toBeNull()
  })
})

describe('base64ToBytes', () => {
  it('解码 Base64 文本', () => {
    expect(Array.from(base64ToBytes('AQID') ?? [])).toEqual([1, 2, 3])
  })

  it('空串返回空字节，非法 Base64 返回 null', () => {
    expect(base64ToBytes('')?.length).toBe(0)
    expect(base64ToBytes('!!!not-base64!!!')).toBeNull()
  })

  it('忽略首尾换行与空白', () => {
    expect(Array.from(base64ToBytes(' AQID\n') ?? [])).toEqual([1, 2, 3])
  })
})

describe('toHexDump', () => {
  it('偏移、十六进制与可打印字符对齐输出', () => {
    const dump = toHexDump(new Uint8Array([0x41, 0x42, 0x00, 0x7f]))
    expect(dump.startsWith('00000000  41 42 00 7f')).toBe(true)
    // 0x00 与 0x7f 不可打印，用点号代替
    expect(dump.endsWith('|AB..|')).toBe(true)
  })

  it('按上限截断', () => {
    const bytes = new Uint8Array(64).fill(0x41)
    const dump = toHexDump(bytes, 16)
    expect(dump.split('\n')).toHaveLength(1)
  })
})

describe('formatJsonText', () => {
  it('合法 JSON 按缩进格式化', () => {
    const result = formatJsonText('{"a":1,"b":[2,3]}')
    expect(result.isJson).toBe(true)
    expect(result.text).toBe('{\n  "a": 1,\n  "b": [\n    2,\n    3\n  ]\n}')
    expect(result.value).toEqual({ a: 1, b: [2, 3] })
  })

  it('非法 JSON 回退原文', () => {
    const raw = '{"a":1,'
    const result = formatJsonText(raw)
    expect(result.isJson).toBe(false)
    expect(result.text).toBe(raw)
  })

  it('普通文本不当作 JSON 处理', () => {
    const raw = 'hello world'
    expect(formatJsonText(raw)).toEqual({ isJson: false, text: raw })
  })

  it('超过 maxChars 时跳过解析', () => {
    const raw = '{"a":1}'
    const result = formatJsonText(raw, { maxChars: 3 })
    expect(result.isJson).toBe(false)
    expect(result.text).toBe(raw)
  })
})

describe('truncateForRender', () => {
  it('未超限时原样返回', () => {
    expect(truncateForRender('abc', 10)).toEqual({ text: 'abc', truncated: false })
  })

  it('超限时截断并标记', () => {
    expect(truncateForRender('abcdef', 3)).toEqual({ text: 'abc', truncated: true })
  })
})

describe('formatByteSize', () => {
  it('按 B / KB / MB 展示', () => {
    expect(formatByteSize(512)).toBe('512 B')
    expect(formatByteSize(2048)).toBe('2.0 KB')
    expect(formatByteSize(5 * 1024 * 1024)).toBe('5.00 MB')
  })
})

describe('defaultFileName', () => {
  it('清理非法字符并追加扩展名', () => {
    expect(defaultFileName('avatar', 'png')).toBe('avatar.png')
    expect(defaultFileName('a/b:c d', 'bin')).toBe('a_b_c_d.bin')
    expect(defaultFileName('', 'txt')).toBe('cell.txt')
  })
})
