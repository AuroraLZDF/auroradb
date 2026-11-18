import { defineStore } from 'pinia'
import type { ChartConfig } from '@/utils/chart'

/**
 * 仪表盘（V3-1.3）：命名保存/打开/删除，与连接 + 库绑定；
 * 卡片 = 聚合查询 SQL + 字段映射，打开时执行取数渲染。
 * 定义持久化在 localStorage——定义只是几 KB 的配置，无需进后端。
 */
export interface DashboardCard {
  id: string
  title: string
  /** 聚合查询 SQL（SELECT 维度, AGG(度量)... GROUP BY ...），可含 :name 具名参数 */
  sql: string
  config: ChartConfig
  /** 自动刷新间隔秒数，0 或缺省为关闭（V3-1.4） */
  refresh_sec?: number
  /** 网格跨列数 1~4，缺省 1（V3-1.5） */
  w?: number
  /** 网格跨行数 1~3，缺省 1（V3-1.5） */
  h?: number
}

export interface Dashboard {
  id: string
  name: string
  /** 绑定的连接 id + 库名，卡片 SQL 在此执行 */
  connectionId: string
  database: string
  cards: DashboardCard[]
}

const STORAGE_KEY = 'auroradb.dashboards.v1'
const RECENT_KEY = 'auroradb.dashboards.recent'

function load(): Dashboard[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as unknown
    return Array.isArray(parsed) ? (parsed as Dashboard[]) : []
  } catch {
    return []
  }
}

function save(dashboards: Dashboard[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(dashboards))
  } catch {
    // 存储满等异常静默失败，不影响功能
  }
}

let cardSeq = 0
function newCardId(): string {
  cardSeq += 1
  return `card-${Date.now()}-${cardSeq}`
}

export const useDashboardStore = defineStore('dashboard', {
  state: () => ({
    dashboards: load() as Dashboard[],
    /** 最近使用的仪表盘 id，用于「存入仪表盘」与恢复打开 */
    recentId: localStorage.getItem(RECENT_KEY) ?? '',
    currentId: '',
  }),

  getters: {
    current: (state) => state.dashboards.find((d) => d.id === state.currentId) ?? null,
    recent: (state) => state.dashboards.find((d) => d.id === state.recentId) ?? null,
  },

  actions: {
    persist() {
      save(this.dashboards)
    },

    setCurrent(id: string) {
      this.currentId = id
      if (id) {
        this.recentId = id
        try { localStorage.setItem(RECENT_KEY, id) } catch { /* 忽略 */ }
      }
    },

    /** 命名新建并设为当前 */
    create(name: string, connectionId: string, database: string): Dashboard {
      const dashboard: Dashboard = {
        id: `dash-${Date.now()}`,
        name: name.trim() || 'Dashboard',
        connectionId,
        database,
        cards: [],
      }
      this.dashboards.push(dashboard)
      this.setCurrent(dashboard.id)
      this.persist()
      return dashboard
    },

    /** 确保存在可用的仪表盘：有最近使用的用之，否则按连接+库名新建 */
    ensureFor(connectionId: string, database: string): Dashboard | null {
      const recent = this.recent
      if (recent) return recent
      return this.create('Dashboard', connectionId, database)
    },

    rename(id: string, name: string) {
      const d = this.dashboards.find((x) => x.id === id)
      if (d && name.trim()) {
        d.name = name.trim()
        this.persist()
      }
    },

    remove(id: string) {
      this.dashboards = this.dashboards.filter((d) => d.id !== id)
      if (this.currentId === id) this.currentId = ''
      if (this.recentId === id) this.recentId = this.dashboards[0]?.id ?? ''
      this.persist()
    },

    addCard(dashboardId: string, card: Omit<DashboardCard, 'id'>): DashboardCard | null {
      const d = this.dashboards.find((x) => x.id === dashboardId)
      if (!d) return null
      const full: DashboardCard = { ...card, id: newCardId() }
      d.cards.push(full)
      this.persist()
      return full
    },

    updateCard(dashboardId: string, cardId: string, patch: Partial<Omit<DashboardCard, 'id'>>) {
      const d = this.dashboards.find((x) => x.id === dashboardId)
      const card = d?.cards.find((c) => c.id === cardId)
      if (card) {
        Object.assign(card, patch)
        this.persist()
      }
    },

    removeCard(dashboardId: string, cardId: string) {
      const d = this.dashboards.find((x) => x.id === dashboardId)
      if (d) {
        d.cards = d.cards.filter((c) => c.id !== cardId)
        this.persist()
      }
    },

    /** 拖拽换位：把 fromId 的卡片移到 toId 卡片当前位置（V3-1.5） */
    reorderCard(dashboardId: string, fromId: string, toId: string) {
      const d = this.dashboards.find((x) => x.id === dashboardId)
      if (!d || fromId === toId) return
      const from = d.cards.findIndex((c) => c.id === fromId)
      const to = d.cards.findIndex((c) => c.id === toId)
      if (from < 0 || to < 0) return
      const [moved] = d.cards.splice(from, 1)
      if (moved) {
        d.cards.splice(to, 0, moved)
        this.persist()
      }
    },

    resizeCard(dashboardId: string, cardId: string, w: number, h: number) {
      const card = this.dashboards.find((x) => x.id === dashboardId)?.cards.find((c) => c.id === cardId)
      if (card) {
        card.w = w
        card.h = h
        this.persist()
      }
    },
  },
})
