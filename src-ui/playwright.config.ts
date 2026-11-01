import { defineConfig, devices } from '@playwright/test'

/**
 * Playwright E2E 配置。
 *
 * 用例跑在浏览器里访问 Vite dev server，通过 `e2e/support/tauri-mock.ts`
 * 注入伪造的 Tauri IPC，因此无需 Tauri 运行时与真实数据库。
 */
export default defineConfig({
  testDir: './e2e',
  // 用例之间互不影响（每个用例都会重新注入 mock 状态），可以并行
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: process.env.CI
    ? [['list'], ['html', { open: 'never' }]]
    : [['list']],
  timeout: 30_000,
  expect: { timeout: 10_000 },

  use: {
    baseURL: 'http://localhost:1420',
    // 中文文案断言依赖浏览器语言（localStorage 无记录时按 navigator.language 选择）
    locale: 'zh-CN',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    // 无法访问 Playwright CDN 的离线环境，可用该变量指定本机已有的 Chromium
    launchOptions: process.env.AURORADB_E2E_CHROMIUM
      ? { executablePath: process.env.AURORADB_E2E_CHROMIUM }
      : {},
  },

  projects: [
    {
      name: 'chromium',
      // 桌面端布局：侧边栏 + 主内容区 + 信息面板需要足够宽度
      use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } },
    },
  ],

  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:1420',
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    stdout: 'ignore',
    stderr: 'pipe',
  },
})
