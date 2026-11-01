import { expect, type Locator, type Page } from '@playwright/test'
import { FIXTURE, type MockFixture } from './tauri-mock'

/**
 * 面向真实 UI 的操作封装：只通过用户可见的按钮/菜单/单元格交互，
 * 不直接读写 store，保证用例覆盖的是组装后的界面。
 */

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/** 按表单标签定位输入框（`:text-is` 精确匹配，避免「数据库」命中「数据库类型」） */
function fieldInput(page: Page, label: string): Locator {
  return page.locator(`.form-group:has(> label:text-is("${label}")) input`).first()
}

/** 通过顶部「新建连接」弹窗连接到 mock 数据库 */
export async function connectToMockDatabase(
  page: Page,
  fixture: MockFixture = FIXTURE,
): Promise<void> {
  await page.locator('button[title="新建连接"]').click()

  const form = page.locator('.connection-form')
  await expect(form).toBeVisible()

  await fieldInput(page, '连接名称').fill(fixture.connectionName)
  await fieldInput(page, '主机').fill(fixture.host)
  await fieldInput(page, '端口').fill(String(fixture.port))
  await fieldInput(page, '用户名').fill(fixture.username)
  await fieldInput(page, '密码').fill(fixture.password)
  await fieldInput(page, '数据库').fill(fixture.database)

  await form.locator('button[type="submit"]').click()

  // 弹窗关闭 + 左侧出现活跃连接，说明 connect 命令已完成
  await expect(page.locator('.modal-connection')).toHaveCount(0)
  await expect(page.locator('.sidebar-left .connection-item:not(.saved)').first()).toContainText(
    fixture.connectionName,
  )
}

/** 展开数据库并展开「表」分类 */
export async function openDatabase(page: Page, database: string = FIXTURE.database): Promise<void> {
  const connection = page.locator('.sidebar-left .connection-item:not(.saved)').first()

  // 连接成功后默认已展开；若被手动收起则补一次点击
  const databaseItem = page.locator('.database-item:not(.new-database)').first()
  if (!(await databaseItem.isVisible())) {
    await connection.locator('.expand-icon').click()
  }

  const db = page
    .locator('.database-item:not(.new-database)')
    .filter({ has: page.locator('.database-name', { hasText: database }) })
    .first()
  await db.locator('.database-header').dblclick()
  await expect(db.locator('.object-categories')).toBeVisible()

  await db.locator('.category-header').filter({ hasText: '表' }).click()
  await expect(db.locator('.object-item').filter({ hasText: FIXTURE.table })).toBeVisible()
}

/** 双击侧栏表名打开数据表格，并等待数据加载完成 */
export async function openTableTab(page: Page, table: string = FIXTURE.table): Promise<void> {
  await openDatabase(page)

  await page
    .locator('.sidebar-left .object-item')
    .filter({ hasText: table })
    .first()
    .dblclick()

  await expect(page.locator('.tab-bar .tab-title').filter({ hasText: table }).first()).toBeVisible()
  await expect(page.locator('.table-container')).toBeVisible()
  await expect(page.locator('.table-container tbody tr').first()).toContainText('Alice')
}

/** 新建查询 Tab，输入 SQL 并执行 */
export async function runQuery(page: Page, sql: string): Promise<void> {
  await page.locator('button[title="新建查询"]').click()

  const editor = page.locator('.sql-view textarea.sql-textarea')
  await expect(editor).toBeVisible()
  await editor.fill(sql)
  await page.locator('.sql-view .btn-execute').click()

  await expect(page.locator('.result-panel .result-header')).toContainText('执行成功')
}

/** 单击单元格进入编辑，改值后回车，再走「保存 → 预览确认」流程 */
export async function editCellAndSave(page: Page, from: string, to: string): Promise<void> {
  const cell = page
    .locator('.table-container tbody td')
    .filter({ hasText: new RegExp(`^${escapeRegExp(from)}$`) })
    .first()
  await cell.click()

  const input = page.locator('input.edit-input')
  await expect(input).toBeVisible()
  await input.fill(to)
  await input.press('Enter')

  await page.locator('.toolbar-actions button').filter({ hasText: '保存' }).click()
  await page.locator('.save-preview-dialog .btn-primary').click()
  await expect(page.locator('.data-table .toast-message')).toContainText('保存成功')
}

/** 右键侧栏表名 → 导出数据 → 确认导出 */
export async function exportTable(page: Page, table: string = FIXTURE.table): Promise<void> {
  await page
    .locator('.sidebar-left .object-item')
    .filter({ hasText: table })
    .first()
    .click({ button: 'right' })

  const menu = page.locator('.context-menu:visible')
  await expect(menu).toBeVisible()
  await menu.locator('.menu-item').filter({ hasText: '导出数据' }).click()

  const dialog = page.locator('.export-dialog')
  await expect(dialog).toBeVisible()
  await dialog.locator('.btn-primary').click()
  await expect(page.locator('.data-table .toast-message')).toContainText('文件已保存到')
}
