import { describe, expect, it } from 'vitest'
import { DRIVER_CAPABILITIES, capabilitiesOf, normalizeDriver } from './driver'

describe('driver capabilities', () => {
  it('默认回退到 MySQL 能力（未连接/未指定驱动时保持旧行为）', () => {
    expect(normalizeDriver(undefined)).toBe('mysql')
    expect(normalizeDriver(null)).toBe('mysql')
    expect(capabilitiesOf(undefined)).toBe(DRIVER_CAPABILITIES.mysql)
    expect(capabilitiesOf(null).prompt).toBe('mysql')
  })

  it('MySQL 保持全部能力：可写、零值日期、原生 USE / SHOW CREATE TABLE、全部 Pro 工具', () => {
    const mysql = capabilitiesOf('mysql')
    expect(mysql.editableTableData).toBe(true)
    expect(mysql.zeroDateDefaults).toBe(true)
    expect(mysql.nativeUseStatement).toBe(true)
    expect(mysql.nativeShowCreateTable).toBe(true)
    expect(mysql.formatDdl).toBe(false)
    expect(mysql.tableQualification).toBe('database')
    expect(Object.values(mysql.proTools).every(Boolean)).toBe(true)
  })

  it('PostgreSQL 支持表数据保存与 ER 图，其余 Pro 工具留待 P2', () => {
    const postgres = capabilitiesOf('postgres')
    expect(postgres.editableTableData).toBe(true)
    expect(postgres.zeroDateDefaults).toBe(false)
    expect(postgres.nativeUseStatement).toBe(false)
    expect(postgres.nativeShowCreateTable).toBe(false)
    expect(postgres.formatDdl).toBe(true)
    expect(postgres.requiresDatabase).toBe(false)
    expect(postgres.tableQualification).toBe('schema')
    expect(postgres.proTools.erDiagram).toBe(true)
    expect(postgres.proTools.monitor).toBe(false)
    expect(postgres.proTools.backup).toBe(false)
    expect(postgres.proTools.schemaDiff).toBe(false)
    expect(postgres.proTools.globalSearch).toBe(false)
  })

  it('每个驱动都有独立配置对象与默认端口', () => {
    const drivers = Object.keys(DRIVER_CAPABILITIES)
    expect(new Set(drivers.map(driver => DRIVER_CAPABILITIES[driver as keyof typeof DRIVER_CAPABILITIES])).size).toBe(drivers.length)
    expect(DRIVER_CAPABILITIES.mysql.defaultPort).toBe(3306)
    expect(DRIVER_CAPABILITIES.postgres.defaultPort).toBe(5432)
  })
})
