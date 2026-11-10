import { expect, test } from '@playwright/test'
import { FIXTURE, getTauriCalls, installTauriMock } from './support/tauri-mock'
import {
  connectToMockDatabase,
  editCellAndSave,
  exportTable,
  openTableTab,
  runQuery,
} from './support/app'

/**
 * V2-7.1 端到端主流程：连接 → 查询 → 编辑保存 → 导出。
 *
 * 后端由 `support/tauri-mock.ts` 在页面里伪造，因此不需要 Tauri 运行时与真实数据库。
 */
test.describe('连接 → 查询 → 编辑保存 → 导出', () => {
  test.beforeEach(async ({ page }) => {
    await installTauriMock(page)
    await page.goto('/')
    // 未连接时展示欢迎页，确认前端已完成挂载
    await expect(page.locator('.welcome h2')).toContainText('欢迎使用 AuroraDB')
    await connectToMockDatabase(page)
  })

  test('执行查询并展示结果集', async ({ page }) => {
    await runQuery(page, 'SELECT * FROM users')

    const header = page.locator('.result-panel .result-header')
    await expect(header).toContainText('执行成功')
    // 行数统计在结果脚注（c309b40 起从 header 移入 .result-footer）
    const footer = page.locator('.result-panel .result-footer')
    await expect(footer).toContainText('共 2 行')

    const rows = page.locator('.result-table tbody tr')
    await expect(rows).toHaveCount(2)
    await expect(rows.first()).toContainText('Alice')

    const calls = await getTauriCalls(page)
    const executeCalls = calls.filter((call) => call.cmd === 'execute_sql')
    expect(executeCalls).toHaveLength(1)
    expect(executeCalls[0]?.args).toMatchObject({
      database: FIXTURE.database,
      sql: 'SELECT * FROM users',
    })
  })

  test('编辑单元格并通过预览弹窗保存', async ({ page }) => {
    await openTableTab(page)
    await editCellAndSave(page, 'Alice', 'Alicia')

    // 保存成功后表格会重新拉取数据，新值应来自后端（mock 已把 UPDATE 回写内存）
    await expect(page.locator('.table-container')).toContainText('Alicia')

    const calls = await getTauriCalls(page)
    const applyCalls = calls.filter((call) => call.cmd === 'apply_table_changes')
    expect(applyCalls).toHaveLength(1)

    const changes = (applyCalls[0]?.args.changes ?? []) as Array<{ kind: string; sql: string }>
    expect(changes).toHaveLength(1)
    expect(changes[0]?.kind).toBe('update')
    expect(changes[0]?.sql).toContain('Alicia')
  })

  test('右键导出表数据', async ({ page }) => {
    await openTableTab(page)
    await exportTable(page)

    const calls = await getTauriCalls(page)
    const exportCalls = calls.filter((call) => call.cmd === 'export_table_to_file')
    expect(exportCalls).toHaveLength(1)
    expect(exportCalls[0]?.args).toMatchObject({
      database: FIXTURE.database,
      table: FIXTURE.table,
      format: 'csv',
    })
  })
})
