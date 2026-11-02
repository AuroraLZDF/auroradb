#!/usr/bin/env node
// 版本号单一来源校验：以 src-tauri/tauri.conf.json 的 version 为基准，
// 校验其余配置文件的版本号是否一致。不一致时以非 0 退出，供构建/发布/CI 阻断。
//
// 用法: node tools/check-version.mjs

import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const BASELINE = 'src-tauri/tauri.conf.json'

function readText(rel) {
  return readFileSync(resolve(ROOT, rel), 'utf8')
}

function readJson(rel) {
  return JSON.parse(readText(rel))
}

function cargoPackageVersion(rel) {
  const text = readText(rel)
  const section = text.match(/^\[package\][ \t]*\r?\n([\s\S]*?)(?=^\[|$(?![\s\S]))/m)
  if (!section) throw new Error(`未找到 [package] 段`)
  const version = section[1].match(/^version\s*=\s*"([^"]+)"/m)
  if (!version) throw new Error(`[package] 段缺少 version`)
  return version[1]
}

function cargoLockVersion(rel, crate) {
  const text = readText(rel)
  for (const block of text.split(/^\[\[package\]\][ \t]*$/m)) {
    const name = block.match(/^name\s*=\s*"([^"]+)"/m)
    if (name && name[1] === crate) {
      const version = block.match(/^version\s*=\s*"([^"]+)"/m)
      if (version) return version[1]
    }
  }
  throw new Error(`未找到 ${crate} 的 package 条目`)
}

function check(label, getVersion) {
  try {
    return { label, version: getVersion() }
  } catch (error) {
    return { label, error: error.message }
  }
}

const baseline = check(BASELINE, () => readJson(BASELINE).version)
if (baseline.error || !baseline.version) {
  console.error(`版本校验失败: 无法读取 ${BASELINE} 的 version`)
  process.exit(1)
}

const expected = baseline.version
const targets = [
  baseline,
  check('src-ui/package.json', () => readJson('src-ui/package.json').version),
  check('src-ui/package-lock.json', () => {
    const lock = readJson('src-ui/package-lock.json')
    const rootPkg = lock.packages?.['']?.version
    if (lock.version !== rootPkg) {
      throw new Error(`顶层 version=${lock.version} 与 packages[""].version=${rootPkg} 不一致`)
    }
    return lock.version
  }),
  check('src-tauri/Cargo.toml', () => cargoPackageVersion('src-tauri/Cargo.toml')),
  check('src-tauri/Cargo.lock', () => cargoLockVersion('src-tauri/Cargo.lock', 'auroradb')),
]

let failed = false
console.log(`版本号单一来源校验（基准 ${BASELINE} = ${expected}）`)
for (const { label, version, error } of targets) {
  if (error) {
    failed = true
    console.log(`  [FAIL] ${label}: ${error}`)
  } else if (version !== expected) {
    failed = true
    console.log(`  [FAIL] ${label}: ${version} (期望 ${expected})`)
  } else {
    console.log(`  [ OK ] ${label}: ${version}`)
  }
}

if (failed) {
  console.error('')
  console.error('版本号不一致：请以 src-tauri/tauri.conf.json 为准，手工同步以下文件：')
  console.error('  src-ui/package.json、src-ui/package-lock.json、src-tauri/Cargo.toml、src-tauri/Cargo.lock')
  process.exit(1)
}
