import { defineStore } from 'pinia'

/**
 * ER 模型（V3-3.1 模型设计器）：不连库也能建模，正向设计表/字段/关系。
 * 模型整体就是一份 JSON 定义，持久化在 localStorage——几 KB 的结构无需进后端；
 * 导出/导入模型文件（.json）在 ErModelDesigner.vue 里通过系统对话框完成。
 */

export interface ModelColumn {
  id: string
  name: string
  /** 类型带完整精度，如 bigint / varchar(64) / decimal(10,2) */
  type: string
  pk: boolean
  nullable: boolean
  unique: boolean
  default?: string
  comment?: string
}

export interface ModelTable {
  id: string
  name: string
  /** 画布坐标（手动拖拽定位，随模型一起保存） */
  x: number
  y: number
  comment?: string
  columns: ModelColumn[]
}

export interface ModelRelation {
  id: string
  fromTableId: string
  fromColumnId: string
  toTableId: string
  toColumnId: string
}

export interface ErModel {
  id: string
  name: string
  tables: ModelTable[]
  relations: ModelRelation[]
  updatedAt: number
}

const STORAGE_KEY = 'auroradb.erModels.v1'

function load(): ErModel[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as unknown
    return Array.isArray(parsed) ? (parsed as ErModel[]) : []
  } catch {
    return []
  }
}

function persist(models: ErModel[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(models))
  } catch {
    // 存储满等异常静默失败，不影响编辑中的会话
  }
}

let seq = 0
function newId(prefix: string): string {
  seq += 1
  return `${prefix}-${Date.now()}-${seq}`
}

function touch(model: ErModel, models: ErModel[]) {
  model.updatedAt = Date.now()
  persist(models)
}

/** 导出的模型文件格式：带版本号，导入时按结构校验并重新分配 id */
export interface ErModelFile {
  kind: 'auroradb-er-model'
  version: 1
  name: string
  tables: Array<{
    name: string
    x?: number
    y?: number
    comment?: string
    columns: Array<{
      name: string
      type: string
      pk?: boolean
      nullable?: boolean
      unique?: boolean
      default?: string
      comment?: string
    }>
  }>
  relations: Array<{
    fromTable: string
    fromColumn: string
    toTable: string
    toColumn: string
  }>
}

export const useErModelStore = defineStore('erModel', {
  state: () => ({
    models: load() as ErModel[],
    currentId: '',
  }),

  getters: {
    current: (state) => state.models.find((m) => m.id === state.currentId) ?? null,
  },

  actions: {
    setCurrent(id: string) {
      this.currentId = id
    },

    create(name: string): ErModel {
      const model: ErModel = {
        id: newId('erm'),
        name: name.trim() || 'Model',
        tables: [],
        relations: [],
        updatedAt: Date.now(),
      }
      this.models.push(model)
      this.currentId = model.id
      persist(this.models)
      return model
    },

    rename(id: string, name: string) {
      const model = this.models.find((m) => m.id === id)
      if (model && name.trim()) {
        model.name = name.trim()
        touch(model, this.models)
      }
    },

    remove(id: string) {
      this.models = this.models.filter((m) => m.id !== id)
      if (this.currentId === id) this.currentId = this.models[0]?.id ?? ''
      persist(this.models)
    },

    // ========== 表 ==========
    addTable(modelId: string, name: string, x: number, y: number): ModelTable | null {
      const model = this.models.find((m) => m.id === modelId)
      if (!model) return null
      const table: ModelTable = {
        id: newId('t'),
        name: name.trim() || 'new_table',
        x: Math.round(x),
        y: Math.round(y),
        columns: [{
          id: newId('c'),
          name: 'id',
          type: 'bigint',
          pk: true,
          nullable: false,
          unique: false,
        }],
      }
      model.tables.push(table)
      touch(model, this.models)
      return table
    },

    renameTable(modelId: string, tableId: string, name: string) {
      const model = this.models.find((m) => m.id === modelId)
      const table = model?.tables.find((t) => t.id === tableId)
      if (model && table && name.trim()) {
        table.name = name.trim()
        touch(model, this.models)
      }
    },

    setTableComment(modelId: string, tableId: string, comment: string) {
      const model = this.models.find((m) => m.id === modelId)
      const table = model?.tables.find((t) => t.id === tableId)
      if (model && table) {
        table.comment = comment.trim() || undefined
        touch(model, this.models)
      }
    },

    moveTable(modelId: string, tableId: string, x: number, y: number) {
      const model = this.models.find((m) => m.id === modelId)
      const table = model?.tables.find((t) => t.id === tableId)
      if (model && table) {
        table.x = Math.round(x)
        table.y = Math.round(y)
        persist(this.models)
      }
    },

    removeTable(modelId: string, tableId: string) {
      const model = this.models.find((m) => m.id === modelId)
      if (!model) return
      model.tables = model.tables.filter((t) => t.id !== tableId)
      // 连带清理指向该表的关系
      model.relations = model.relations.filter(
        (r) => r.fromTableId !== tableId && r.toTableId !== tableId
      )
      touch(model, this.models)
    },

    // ========== 字段 ==========
    addColumn(modelId: string, tableId: string, col: Omit<ModelColumn, 'id'>): ModelColumn | null {
      const model = this.models.find((m) => m.id === modelId)
      const table = model?.tables.find((t) => t.id === tableId)
      if (!model || !table) return null
      const full: ModelColumn = { ...col, id: newId('c') }
      table.columns.push(full)
      touch(model, this.models)
      return full
    },

    updateColumn(modelId: string, tableId: string, columnId: string, patch: Partial<Omit<ModelColumn, 'id'>>) {
      const model = this.models.find((m) => m.id === modelId)
      const col = model?.tables.find((t) => t.id === tableId)?.columns.find((c) => c.id === columnId)
      if (model && col) {
        Object.assign(col, patch)
        touch(model, this.models)
      }
    },

    removeColumn(modelId: string, tableId: string, columnId: string) {
      const model = this.models.find((m) => m.id === modelId)
      const table = model?.tables.find((t) => t.id === tableId)
      if (!model || !table) return
      table.columns = table.columns.filter((c) => c.id !== columnId)
      // 连带清理指向该字段的关系
      model.relations = model.relations.filter(
        (r) =>
          !((r.fromTableId === tableId && r.fromColumnId === columnId) ||
            (r.toTableId === tableId && r.toColumnId === columnId))
      )
      touch(model, this.models)
    },

    // ========== 关系 ==========
    addRelation(modelId: string, rel: Omit<ModelRelation, 'id'>): ModelRelation | null {
      const model = this.models.find((m) => m.id === modelId)
      if (!model) return null
      // 端点必须真实存在，且不允许完全重复
      const exists = (tableId: string, colId: string) =>
        model.tables.find((t) => t.id === tableId)?.columns.some((c) => c.id === colId)
      if (!exists(rel.fromTableId, rel.fromColumnId) || !exists(rel.toTableId, rel.toColumnId)) return null
      const duplicated = model.relations.some(
        (r) =>
          r.fromTableId === rel.fromTableId && r.fromColumnId === rel.fromColumnId &&
          r.toTableId === rel.toTableId && r.toColumnId === rel.toColumnId
      )
      if (duplicated) return null
      const full: ModelRelation = { ...rel, id: newId('rel') }
      model.relations.push(full)
      touch(model, this.models)
      return full
    },

    removeRelation(modelId: string, relationId: string) {
      const model = this.models.find((m) => m.id === modelId)
      if (!model) return
      model.relations = model.relations.filter((r) => r.id !== relationId)
      touch(model, this.models)
    },

    // ========== 导入 / 导出 ==========
    toFile(modelId: string): string {
      const model = this.models.find((m) => m.id === modelId)
      if (!model) return ''
      const file: ErModelFile = {
        kind: 'auroradb-er-model',
        version: 1,
        name: model.name,
        tables: model.tables.map((t) => ({
          name: t.name,
          x: t.x,
          y: t.y,
          comment: t.comment,
          columns: t.columns.map((c) => ({
            name: c.name,
            type: c.type,
            pk: c.pk,
            nullable: c.nullable,
            unique: c.unique,
            default: c.default,
            comment: c.comment,
          })),
        })),
        relations: model.relations.flatMap((r) => {
          const fromT = model.tables.find((t) => t.id === r.fromTableId)
          const toT = model.tables.find((t) => t.id === r.toTableId)
          const fromC = fromT?.columns.find((c) => c.id === r.fromColumnId)
          const toC = toT?.columns.find((c) => c.id === r.toColumnId)
          if (!fromT || !toT || !fromC || !toC) return []
          return [{
            fromTable: fromT.name,
            fromColumn: fromC.name,
            toTable: toT.name,
            toColumn: toC.name,
          }]
        }),
      }
      return JSON.stringify(file, null, 2)
    },

    /** 从模型文件 JSON 导入为一个新模型，返回错误文案（空串 = 成功） */
    importFromJson(json: string, fallbackName: string): string {
      let data: unknown
      try {
        data = JSON.parse(json)
      } catch {
        return '不是合法的 JSON 文件'
      }
      const file = data as ErModelFile
      if (!file || typeof file !== 'object' || !Array.isArray(file.tables)) {
        return '缺少 tables 字段，不是 AuroraDB 模型文件'
      }
      const model = this.create(file.name?.trim() || fallbackName)
      const tableIds = new Map<string, string>()
      const colIds = new Map<string, string>()
      for (const t of file.tables) {
        if (!t?.name?.trim()) continue
        const table = this.addTable(model.id, t.name.trim(), t.x ?? 0, t.y ?? 0)
        if (!table) continue
        tableIds.set(t.name, table.id)
        table.comment = t.comment?.trim() || undefined
        const first = table.columns[0]!
        if (t.columns?.length) {
          // 换掉建表时自带的 id 列，按文件内容重建
          table.columns = []
          for (const c of t.columns) {
            if (!c?.name?.trim()) continue
            const col = this.addColumn(model.id, table.id, {
              name: c.name.trim(),
              type: c.type?.trim() || 'varchar(255)',
              pk: !!c.pk,
              nullable: c.nullable !== false,
              unique: !!c.unique,
              default: c.default,
              comment: c.comment,
            })
            if (col) colIds.set(`${t.name}.${c.name.trim()}`, col.id)
          }
        } else {
          colIds.set(`${t.name}.${first.name}`, first.id)
        }
      }
      for (const r of file.relations ?? []) {
        const fromT = tableIds.get(r.fromTable)
        const toT = tableIds.get(r.toTable)
        const fromC = colIds.get(`${r.fromTable}.${r.fromColumn}`)
        const toC = colIds.get(`${r.toTable}.${r.toColumn}`)
        if (fromT && toT && fromC && toC) {
          this.addRelation(model.id, {
            fromTableId: fromT,
            fromColumnId: fromC,
            toTableId: toT,
            toColumnId: toC,
          })
        }
      }
      persist(this.models)
      return ''
    },
  },
})
