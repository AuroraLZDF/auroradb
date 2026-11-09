import { expect, test } from '@playwright/test'
import { FIXTURE, getTauriCalls, installTauriMock } from './support/tauri-mock'
import { chooseCustomSelect, connectToMockDatabase, openTableTab } from './support/app'

/**
 * 表格筛选回归：LIKE 必须是「包含」语义。
 *
 * 曾经的缺陷：前端生成的 WHERE 与后端构造的 SQL 都把用户输入原样当模式，
 * MySQL 里 `LIKE 'li'` 等价于 `= 'li'`，于是「筛选后没有数据」。
 * 现在两端都补 `%`（用户自己写了 `%`/`_` 时按原样当模式）。
 */
test.describe('表格筛选', () => {
  test.beforeEach(async ({ page }) => {
    await installTauriMock(page)
    await page.goto('/')
    await connectToMockDatabase(page)
    await openTableTab(page)
  })

  test('LIKE 按包含匹配，WHERE 预览带 % 包裹', async ({ page }) => {
    await page.locator('.toolbar-right > button').filter({ hasText: '筛选' }).click()
    if ((await page.locator('.filter-row').count()) === 0) {
      await page.locator('.filter-actions button').filter({ hasText: '添加条件' }).click()
    }

    const row = page.locator('.filter-row').first()
    const selects = row.locator('.filter-select')
    await chooseCustomSelect(page, selects.nth(0).locator('.select-trigger'), 'name')
    await chooseCustomSelect(page, selects.nth(1).locator('.select-trigger'), 'LIKE（包含）')
    await row.locator('input.filter-value').fill('li')

    await expect(page.locator('.filter-preview code')).toHaveText("WHERE `name` LIKE '%li%'")

    await page.locator('.filter-actions button').filter({ hasText: '应用筛选' }).click()

    const rows = page.locator('.table-container tbody tr')
    await expect(rows).toHaveCount(1)
    await expect(rows.first()).toContainText('Alice')
    await expect(page.locator('.table-footer')).toContainText('共 1 行')

    const calls = await getTauriCalls(page)
    const queries = calls.filter((call) => call.cmd === 'query_table_data')
    const last = queries[queries.length - 1]
    expect(last?.args.request).toMatchObject({
      table: FIXTURE.table,
      filters: [{ column: 'name', operator: 'Like', value: 'li' }],
    })
  })

  test('显式通配符按原样当模式', async ({ page }) => {
    await page.locator('.toolbar-right > button').filter({ hasText: '筛选' }).click()
    if ((await page.locator('.filter-row').count()) === 0) {
      await page.locator('.filter-actions button').filter({ hasText: '添加条件' }).click()
    }

    const row = page.locator('.filter-row').first()
    const selects = row.locator('.filter-select')
    await chooseCustomSelect(page, selects.nth(0).locator('.select-trigger'), 'name')
    await chooseCustomSelect(page, selects.nth(1).locator('.select-trigger'), 'LIKE（包含）')
    await row.locator('input.filter-value').fill('Ali%')

    await expect(page.locator('.filter-preview code')).toHaveText("WHERE `name` LIKE 'Ali%'")

    await page.locator('.filter-actions button').filter({ hasText: '应用筛选' }).click()
    const rows = page.locator('.table-container tbody tr')
    await expect(rows).toHaveCount(1)
    await expect(rows.first()).toContainText('Alice')
  })
})
