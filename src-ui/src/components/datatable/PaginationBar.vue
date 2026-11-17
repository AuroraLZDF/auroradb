<template>
  <div v-if="tableData" class="table-footer">
    <div class="footer-left">
      <!-- 行区间只在这里出现一次（原工具栏也显示一份，属于重复信息） -->
      <span v-if="tableData" class="row-info">
        {{ t('dataTable.rowRange', {
          start: cursorMode ? 1 : (page - 1) * pageSize + 1,
          end: cursorMode ? tableData.rows.length : Math.min(page * pageSize, tableData.total_count),
          total: tableData.total_count
        }) }}
      </span>
      <!-- 大表使用游标翻页：只追加「加载更多」，不再展示页码 -->
      <div v-if="cursorMode" class="pagination load-more-bar">
        <span class="loaded-rows">
          {{ t('dataTable.loadedRows', { loaded: tableData.rows.length, total: tableData.total_count }) }}
        </span>
        <button
          v-if="tableData.next_cursor != null"
          @click="$emit('load-more')"
          :disabled="!canLoadMore"
        >{{ loadingMore ? t('dataTable.loadingMore') : t('dataTable.loadMore') }}</button>
        <span v-else class="all-loaded">{{ t('dataTable.allLoaded') }}</span>
      </div>
      <div v-else class="pagination">
        <button @click="$emit('first-page')" :disabled="page <= 1">{{ t('dataTable.firstPage') }}</button>
        <button @click="$emit('prev-page')" :disabled="page <= 1">{{ t('dataTable.prevPage') }}</button>
        <span>{{ t('dataTable.pageIndicator', { page, total: totalPages }) }}</span>
        <button @click="$emit('next-page')" :disabled="page >= totalPages">{{ t('dataTable.nextPage') }}</button>
        <button @click="$emit('last-page')" :disabled="page >= totalPages">{{ t('dataTable.lastPage') }}</button>
      </div>
    </div>
    <div class="footer-right">
      <div class="page-size">
        <Select
          :model-value="String(pageSize)"
          :options="pageSizeOptions"
          size="sm"
          @update:model-value="$emit('page-size-change', $event)"
        />
      </div>
      <div class="execution-time" v-if="tableData">
        {{ t('dataTable.executionTime', { ms: tableData.execution_time_ms }) }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { t } from '@/i18n'
import Select from '@/components/ui/Select.vue'

export interface PaginationTableData {
  rows: unknown[][]
  total_count: number
  next_cursor?: unknown
  execution_time_ms: number
}

defineProps<{
  tableData: PaginationTableData | null
  cursorMode: boolean
  page: number
  pageSize: number
  totalPages: number
  canLoadMore: boolean
  loadingMore: boolean
  pageSizeOptions: { label: string; value: string }[]
}>()

defineEmits<{
  (e: 'load-more'): void
  (e: 'first-page'): void
  (e: 'prev-page'): void
  (e: 'next-page'): void
  (e: 'last-page'): void
  (e: 'page-size-change', value: string): void
}>()
</script>

<style scoped>
.table-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: var(--space-3);
  height: var(--toolbar-height);
  padding: 0 var(--space-2) 0 var(--space-3);
  background: var(--canvas);
  border-top: 1px solid var(--hairline);
  flex-shrink: 0;
  overflow: hidden;
}

.row-info {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  flex-shrink: 0;
}

/* 窄窗口时行数/分页只横向滚动，不允许换行（换行会撑破 32px 的脚注） */
.footer-left,
.footer-right {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  min-width: 0;
  white-space: nowrap;
}

.footer-left {
  flex: 1 1 auto;
  overflow-x: auto;
  scrollbar-width: none;
}

.footer-left::-webkit-scrollbar {
  display: none;
}

.footer-left > * {
  flex-shrink: 0;
}

.footer-right {
  flex-shrink: 0;
}

.pagination {
  display: flex;
  gap: var(--space-1);
  align-items: center;
  flex-shrink: 0;
  white-space: nowrap;
}

.pagination button {
  height: 22px;
  padding: 0 var(--space-2);
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  transition: var(--transition-colors);
}

.pagination button:hover:not(:disabled) {
  background: var(--hover-fill);
  color: var(--text-primary);
}

.pagination button:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* 分页条数选择：外层定宽，触发器与分页按钮同高 */
.page-size {
  width: 120px;
  flex-shrink: 0;
}

.page-size :deep(.custom-select .select-trigger) {
  height: 22px;
  padding: 0 var(--space-2);
  border-color: transparent;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-xs);
}

.page-size :deep(.custom-select .select-trigger:hover) {
  background: var(--hover-fill);
}

/* 游标续页 */
.load-more-bar {
  gap: 12px;
}

.loaded-rows {
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.all-loaded {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}
</style>
