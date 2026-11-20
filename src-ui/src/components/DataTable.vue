<template>
  <div class="data-table" @click="hideContextMenu">
    <!-- 工具栏 -->
    <div class="table-toolbar" v-if="currentTab">
      <div class="toolbar-left">
        <div class="toolbar-actions">
          <!-- 仅当加载持续超过阈值才切换为「取消查询」：快速加载时刷新按钮保持不动，避免按钮一闪而过 -->
          <button v-if="!showCancelLoad" @click="refreshData" :title="t('dataTable.refreshHint')" :aria-label="t('dataTable.refreshHint')">
            <RefreshCw :size="14" />
          </button>
          <!-- 加载中（大表 COUNT / 慢查询）允许取消，避免界面一直等待 -->
          <button v-else @click="cancelLoad" class="btn-cancel-query" :title="t('dataTable.cancelLoadHint')">
            <X :size="14" />
            {{ t('dataTable.cancelLoad') }}
          </button>
          <button @click="addRow" :disabled="loading || !canEdit" :title="canEdit ? t('dataTable.addRowHint') : editBlockReason">
            <Plus :size="14" />
            {{ t('dataTable.addRow') }}
          </button>
          <button @click="deleteSelectedRows" :disabled="!hasSelection || loading || !canEdit" :title="canEdit ? t('dataTable.deleteRowsHint') : editBlockReason">
            <Minus :size="14" />
            {{ t('common.delete') }}
          </button>
          <button @click="saveChanges" :disabled="!hasUnsavedChanges || loading || !canEdit || transactionActive" :title="transactionActive ? t('dataTable.transactionBlockingSave') : (canEdit ? t('dataTable.saveHint') : editBlockReason)">
            <Save :size="14" />
            {{ t('common.save') }}
          </button>
          <button @click="autoFitAllColumns" :disabled="loading || !currentTableData" :title="t('dataTable.autoFitAll')" :aria-label="t('dataTable.autoFitAll')">
            <AlignHorizontalDistributeCenter :size="14" />
          </button>
          <!-- 按需列加载：宽表默认只取前 N 列，勾选后重新查询 -->
          <div v-if="allColumnsForTab.length > 0" class="column-selector">
            <button
              ref="columnSelectorBtn"
              @click.stop="toggleColumnSelector"
              :class="{ active: showColumnSelector }"
              :title="t('dataTable.columnsHint')"
            >
              <Columns3 :size="14" />
              {{ t('dataTable.columnsLabel', { selected: selectedColumnNames.length, total: allColumnsForTab.length }) }}
            </button>
            <!-- Teleport 到 body：工具栏 overflow-y: hidden 会裁掉绝对定位的下拉菜单 -->
            <Teleport to="body">
              <div v-if="showColumnSelector" ref="columnSelectorMenu" class="column-selector-menu" :style="columnSelectorMenuStyle" @click.stop>
              <div class="column-selector-header">
                <span>{{ t('dataTable.columnsTitle') }}</span>
                <div class="column-selector-actions">
                  <button @click="selectAllColumns">{{ t('dataTable.selectAllColumns') }}</button>
                  <button @click="resetColumnSelection">{{ t('dataTable.resetColumns') }}</button>
                </div>
              </div>
              <div class="column-selector-list">
                <label v-for="col in allColumnsForTab" :key="col.name" class="column-option">
                  <input
                    type="checkbox"
                    :checked="isColumnSelected(col.name)"
                    :disabled="col.is_primary"
                    :title="col.is_primary ? t('dataTable.columnsPrimaryRequired') : col.name"
                    @change="toggleColumnSelection(col.name)"
                  />
                  <span class="column-option-name" :title="col.name">{{ col.name }}</span>
                  <span class="column-option-type">{{ col.data_type }}</span>
                </label>
              </div>
              <div class="column-selector-footer">
                <span v-if="colSelectionChanged" class="pending-hint">{{ t('dataTable.columnsPendingHint') }}</span>
                <button
                  class="btn-primary"
                  :disabled="!colSelectionChanged || selectedColumnNames.length === 0"
                  @click="applyColumns"
                >{{ t('dataTable.applyColumns') }}</button>
              </div>
              </div>
            </Teleport>
          </div>
          <!-- 固定主键列：把主键（及前置列）冻结在左侧，横向滚动不消失 -->
          <button
            :class="{ active: pkFrozen }"
            :title="t('dataTable.pkPinHint')"
            @click="pkFrozen = !pkFrozen"
          >
            <Pin :size="14" /> {{ t('dataTable.pkPin') }}
          </button>
          <button
            @click="showGenerator = true"
            :disabled="loading || !currentTab"
            :title="t('dataGenerator.button')"
          >
            <Wand2 :size="14" />
            {{ t('dataGenerator.button') }}
          </button>
          <span class="toolbar-divider"></span>
          <!-- 事务控制：收进分段控件，避免三个 ghost 按钮在 2px 间距下粘连 -->
          <div class="txn-group">
            <button
              @click="beginTransaction"
              :disabled="transactionActive || loading || isReadOnlyConnection"
              :class="{ active: transactionActive }"
              :title="t('dataTable.beginTransaction')"
            >BEGIN</button>
            <button
              @click="commitTransaction"
              :disabled="!transactionActive || loading"
              :title="t('dataTable.commitTransaction')"
            >COMMIT</button>
            <button
              @click="rollbackTransaction"
              :disabled="!transactionActive || loading"
              :title="t('dataTable.rollbackTransaction')"
            >ROLLBACK</button>
            <span v-if="transactionActive" class="transaction-indicator" :title="t('dataTable.transactionActiveHint')">
              <Circle :size="10" fill="var(--success-500)" />
            </span>
          </div>
          <span v-if="isReadOnlyConnection" class="readonly-badge" :title="readOnlyHint">
            {{ readOnlyBadgeLabel }}
          </span>
          <span v-else-if="currentTableData && !hasPrimaryKey" class="readonly-badge" :title="t('dataTable.noPrimaryKeyReadonlyHint')">
            {{ t('dataTable.readOnlyNoPrimaryKey') }}
          </span>
        </div>
      </div>
      <div class="toolbar-right">
        <!-- 排序指示：横向滚动表头出视野后仍能看到当前排序（点击清除） -->
        <button
          v-if="sortColumn"
          class="sort-chip"
          :title="t('dataTable.sortChipHint', { column: sortColumn, direction: sortDirection === 'Asc' ? t('dataTable.sortAsc') : t('dataTable.sortDesc') })"
          @click="clearSort"
        >
          <ArrowUpDown :size="12" />
          {{ sortColumn }}{{ sortDirection === 'Asc' ? ' ↑' : ' ↓' }}
          <X :size="10" class="chip-clear" />
        </button>
        <button
          :class="{ active: chartView }"
          :title="t('dataTable.chartToggle')"
          @click="chartView = !chartView"
        >
          <BarChart3 :size="14" />
          {{ t('dataTable.chartToggle') }}
        </button>
        <button
          :class="{ active: showFilterPanel || activeFilters.length > 0 }"
          :title="t('dataTable.filterToggle')"
          @click="showFilterPanel = !showFilterPanel"
        >
          <Filter :size="14" />
          {{ t('dataTable.filterToggle') }}
          <span v-if="activeFilters.length > 0" class="filter-count">{{ activeFilters.length }}</span>
        </button>
        <input
          v-model="searchText"
          type="text"
          :placeholder="t('dataTable.searchPlaceholder')"
          class="search-input"
        />
      </div>
    </div>

    <!-- 过滤栏：列头筛选入口 + 多条件构建器 + 已存过滤器 -->
    <div v-if="showFilterPanel && currentTab" class="filter-panel" @click.stop>
      <div class="filter-panel-header">
        <span class="filter-panel-title">{{ t('dataTable.filterTitle') }}</span>
        <div class="filter-logic">
          <button :class="{ active: filterLogic === 'AND' }" @click="filterLogic = 'AND'">AND</button>
          <button :class="{ active: filterLogic === 'OR' }" @click="filterLogic = 'OR'">OR</button>
        </div>
        <button class="filter-close" @click="showFilterPanel = false" :title="t('common.close')" :aria-label="t('common.close')">
          <X :size="13" />
        </button>
      </div>

      <div class="filter-rows">
        <div v-for="(row, index) in filterRows" :key="row.id" class="filter-row">
          <span v-if="index > 0" class="filter-join">{{ filterLogic }}</span>
          <span v-else class="filter-join">{{ t('dataTable.filterTitle') }}</span>
          <Select
            v-model="row.column"
            class="filter-select"
            size="sm"
            :options="filterColumnChoices"
            :placeholder="t('dataTable.filterColumn')"
          />
          <Select
            v-model="row.operator"
            class="filter-select filter-op"
            size="sm"
            :options="filterOperatorOptions"
          />
          <input
            v-if="!isNullOperator(row.operator)"
            v-model="row.value"
            class="filter-value"
            :placeholder="row.operator === 'Like' ? t('dataTable.filterValueLike') : t('dataTable.filterValue')"
          />
          <button class="filter-remove" @click="removeFilterRow(row.id)" :title="t('common.delete')" :aria-label="t('common.delete')">
            <X :size="12" />
          </button>
        </div>
        <div v-if="filterRows.length === 0" class="filter-empty">{{ t('dataTable.filterAdd') }}</div>
      </div>

      <div class="filter-actions">
        <button @click="addFilterRow()">+ {{ t('dataTable.filterAdd') }}</button>
        <button class="btn-primary" :disabled="filterRows.length === 0" @click="applyFilters">
          {{ t('dataTable.filterApply') }}
        </button>
        <button @click="clearFilters">{{ t('dataTable.filterClear') }}</button>
      </div>

      <div class="filter-saved">
        <Select
          v-model="selectedSavedFilter"
          class="filter-select"
          size="sm"
          :options="savedFilterChoices"
          :placeholder="t('dataTable.filterSavedPlaceholder')"
          @update:model-value="applySavedFilter"
        />
        <input v-model="newFilterName" class="filter-value" :placeholder="t('dataTable.filterSaveName')" />
        <button :disabled="!newFilterName.trim() || filterRows.length === 0" @click="saveCurrentFilter">
          {{ t('dataTable.filterSave') }}
        </button>
        <button :disabled="!selectedSavedFilter" @click="deleteSavedFilter">
          {{ t('dataTable.filterDelete') }}
        </button>
      </div>

      <div v-if="filterWherePreview" class="filter-preview">
        <code>{{ filterWherePreview }}</code>
        <button @click="copyWherePreview">{{ t('dataTable.filterCopyWhere') }}</button>
      </div>
    </div>

    <!-- 数据表格 -->
    <div class="table-container" v-if="currentTableData && !chartView" ref="tableContainer" @scroll="handleScroll">
      <table>
        <thead>
          <tr>
            <th class="row-number-header frozen-row-number" style="left: 0"></th>
            <th
              v-for="(col, index) in currentTableData.columns"
              :key="col.name"
              :class="[
                { sorted: sortColumn === col.name },
                isColumnFrozen(index) ? 'frozen-col' : ''
              ]"
              :style="{
                width: columnWidths[index] + 'px',
                ...(isColumnFrozen(index) ? { left: getFrozenLeftOffset(index) + 'px' } : {})
              }"
              @click="sort(col.name)"
            >
              <div class="th-content">
                <span class="col-name" :title="col.name">{{ col.name }}</span>
                <span class="data-type">{{ col.data_type }}</span>
                <span class="sort-indicator">
                  <component
                    v-if="sortColumn === col.name"
                    :is="sortDirection === 'Asc' ? ChevronUp : ChevronDown"
                    :size="12"
                  />
                </span>
                <button
                  class="th-filter-btn"
                  :aria-label="t('dataTable.filterColumnHint')"
                  :class="{ active: activeFilters.some(f => f.column === col.name) }"
                  :title="t('dataTable.filterColumnHint')"
                  @click.stop="addFilterRow(col.name)"
                >
                  <Filter :size="11" />
                </button>
              </div>
              <div
                class="resize-handle"
                @mousedown.stop="startResize($event, index)"
                @dblclick.stop="autoFitColumn(index)"
              ></div>
            </th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="currentTableData.rows.length === 0" class="empty-row">
            <td :colspan="currentTableData.columns.length + 1" class="empty-cell">
              {{ activeFilters.length > 0 ? t('dataTable.filterNoMatch') : t('dataTable.tableEmpty') }}
            </td>
          </tr>
          <tr
            v-if="virtualEnabled && virtualPaddingTop > 0"
            class="virtual-spacer"
            aria-hidden="true"
          >
            <td
              :colspan="currentTableData.columns.length + 1"
              :style="{ height: virtualPaddingTop + 'px', padding: '0', border: 'none' }"
            ></td>
          </tr>
          <tr
            v-for="{ row, index: rowIndex } in visibleRows"
            :key="rowIndex"
            :class="{ selected: isRowSelected(rowIndex), 'new-row': isNewRow(rowIndex) }"
            @click="selectRow(rowIndex, $event)"
          >
            <td class="row-number frozen-row-number" style="left: 0">
              <span class="row-num">{{ (cursorMode ? 0 : (page - 1) * pageSize) + rowIndex + 1 }}</span>
            </td>
            <td
              v-for="(cell, cellIndex) in row"
              :key="cellIndex"
              :class="[
                {
                  editing: isEditing(rowIndex, cellIndex),
                  modified: isModified(rowIndex, cellIndex),
                  'is-null': cell === null || cell === undefined
                },
                isColumnFrozen(cellIndex) ? 'frozen-col' : ''
              ]"
              :style="{
                width: columnWidths[cellIndex] + 'px',
                ...(isColumnFrozen(cellIndex) ? { left: getFrozenLeftOffset(cellIndex) + 'px' } : {})
              }"
              @click="onCellClick($event, rowIndex, cellIndex, cell)"
              @dblclick="onCellDblClick(rowIndex, cellIndex, cell)"
              @contextmenu.prevent="showContextMenu($event, rowIndex, cellIndex, cell)"
              :title="getCellTitle(cell)"
            >
              <template v-if="isEditing(rowIndex, cellIndex)">
                <!-- 日期时间类型使用自定义编辑器 -->
                <template v-if="isDateTimeType(editingColumnType)">
                  <div class="datetime-editor-wrapper">
                    <!-- 遮罩层：防止点击外部关闭（仅在弹层打开时显示） -->
                    <div v-if="showDateTimePicker" class="datetime-picker-overlay" @click.stop="showDateTimePicker = false"></div>
                    <div class="datetime-editor-container" @click.stop>
                      <!-- 显示当前值的文本框 -->
                      <input
                        v-model="editingValue"
                        class="edit-input datetime-display"
                        readonly
                        @keydown.enter="finishEdit"
                        @keydown.esc="cancelEdit"
                      />
                      <!-- 打开选择器按钮 -->
                      <button class="btn-open-picker" @click="openDateTimePicker($event)" :title="t('dataTable.pickDateTime')" :aria-label="t('dataTable.pickDateTime')">
                        <Calendar :size="14" />
                      </button>
                      <!-- 自定义选择器弹层 -->
                      <Teleport to="body">
                        <div v-if="showDateTimePicker" class="datetime-picker-popup" 
                             :style="pickerPosition" 
                             @click.stop>
                          <!-- 日期选择 -->
                          <div class="picker-section" v-if="!editingColumnType.includes('time') || editingColumnType.includes('datetime') || editingColumnType.includes('timestamp')">
                            <div class="picker-header">
                              <button @click="prevMonth">◀</button>
                              <span>{{ t('dataTable.yearMonth', { year: currentYear, month: currentMonth }) }}</span>
                              <button @click="nextMonth">▶</button>
                            </div>
                            <div class="picker-days">
                              <div class="day-header">{{ t('dataTable.weekdaySun') }}</div>
                              <div class="day-header">{{ t('dataTable.weekdayMon') }}</div>
                              <div class="day-header">{{ t('dataTable.weekdayTue') }}</div>
                              <div class="day-header">{{ t('dataTable.weekdayWed') }}</div>
                              <div class="day-header">{{ t('dataTable.weekdayThu') }}</div>
                              <div class="day-header">{{ t('dataTable.weekdayFri') }}</div>
                              <div class="day-header">{{ t('dataTable.weekdaySat') }}</div>
                              <div v-for="day in calendarDays" :key="day.date" 
                                   class="day-cell"
                                   :class="{ 
                                     'other-month': !day.isCurrentMonth, 
                                     'selected': day.date === selectedDate,
                                     'today': day.isToday 
                                   }"
                                   @click="selectDate(day.date)">
                                {{ day.day }}
                              </div>
                            </div>
                          </div>
                          <!-- 时间选择 -->
                          <div class="picker-section time-section" v-if="editingColumnType.includes('datetime') || editingColumnType.includes('timestamp') || editingColumnType.includes('time')">
                            <div class="time-inputs">
                              <div class="time-field">
                                <label>{{ t('dataTable.hour') }}</label>
                                <input type="number" v-model.number="selectedHour" min="0" max="23" @input="updateTime" />
                              </div>
                              <span class="time-separator">:</span>
                              <div class="time-field">
                                <label>{{ t('dataTable.minute') }}</label>
                                <input type="number" v-model.number="selectedMinute" min="0" max="59" @input="updateTime" />
                              </div>
                              <span class="time-separator">:</span>
                              <div class="time-field">
                                <label>{{ t('dataTable.second') }}</label>
                                <input type="number" v-model.number="selectedSecond" min="0" max="59" @input="updateTime" />
                              </div>
                            </div>
                          </div>
                          <!-- 选择器按钮 -->
                        <div class="picker-actions">
                          <button class="btn-cancel" @click="showDateTimePicker = false">{{ t('common.cancel') }}</button>
                          <button class="btn-confirm" @click="applyDateTime">{{ t('common.confirm') }}</button>
                        </div>
                        </div>
                      </Teleport>
                    </div>
                  </div>
                </template>
                <!-- 其他类型使用普通文本输入 -->
                <input
                  v-else
                  v-model="editingValue"
                  @blur="finishEdit"
                  @keydown.enter="finishEdit"
                  @keydown.esc="cancelEdit"
                  class="edit-input"
                  ref="textInput"
                />
              </template>
              <template v-else>
                <span class="cell-text" :class="getCellClass(cell)">
                  {{ formatCellValue(cell, cellIndex) }}
                </span>
              </template>
            </td>
          </tr>
          <tr
            v-if="virtualEnabled && virtualPaddingBottom > 0"
            class="virtual-spacer"
            aria-hidden="true"
          >
            <td
              :colspan="currentTableData.columns.length + 1"
              :style="{ height: virtualPaddingBottom + 'px', padding: '0', border: 'none' }"
            ></td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- 结果集图表视图：与表格共享同一份 rows（V3-1.1） -->
    <ResultChart
      v-if="currentTableData && chartView"
      :columns="currentTableData.columns"
      :rows="filteredRows"
      @pushdown="copyChartPushdownSql"
    />

    <!-- 分页 / 游标续页 -->
    <PaginationBar
      :table-data="currentTableData"
      :cursor-mode="cursorMode"
      :page="page"
      :page-size="pageSize"
      :total-pages="totalPages"
      :can-load-more="canLoadMore"
      :loading-more="loadingMore"
      :page-size-options="pageSizeOptions"
      @load-more="loadMore"
      @first-page="firstPage"
      @prev-page="prevPage"
      @next-page="nextPage"
      @last-page="lastPage"
      @page-size-change="onPageSizeChange"
    />

    <!-- 加载失败 -->
    <div v-if="!currentTableData && !loading && loadError" class="empty-state error-state">
      {{ t('dataTable.loadFailed', { error: loadError }) }}
    </div>

    <!-- 空状态 -->
    <div v-else-if="!currentTableData && !loading" class="empty-state">
      {{ t('dataTable.selectTableHint') }}
    </div>

    <!-- 加载中 -->
    <div v-if="loading" class="loading-overlay">{{ t('common.loading') }}</div>

    <!-- 右键菜单 -->
    <div v-if="contextMenu.show" class="context-menu" :style="contextMenu.style">
      <div class="menu-item" @click="copyCell">{{ t('common.copy') }}</div>
      <div class="menu-item" @click="copyRow">{{ t('common.copyRow') }}</div>
      <div v-if="hasSelection" class="menu-item" @click="copySelectedRows">{{ t('dataTable.copySelectedRows') }}</div>
      <div class="menu-item" @click="copyColumn">{{ t('common.copyColumn') }}</div>
      <div class="menu-item" @click="copyHeaderRow">{{ t('common.copyHeaderRow') }}</div>
      <div class="menu-item" @click="copyAllRows">{{ t('common.copyAllRows') }}</div>
      <div class="menu-divider"></div>
      <div class="menu-item" @click="autoFitColumnFromMenu">{{ t('dataTable.autoFitColumn') }}</div>
      <div class="menu-item" @click="freezeColumnFromMenu">
        {{ frozenCount > 0 && contextMenu.colIndex < frozenCount ? t('dataTable.unfreezeAll') : t('dataTable.freezeColumn') }}
      </div>
      <div class="menu-divider"></div>
      <div class="menu-item" @click="pasteCell">{{ t('dataTable.paste') }}</div>
      <div class="menu-divider"></div>
      <div class="menu-item" @click="setNull">{{ t('dataTable.setNull') }}</div>
      <div class="menu-item" @click="viewFullContent">{{ t('dataTable.viewFullContent') }}</div>
      <template v-if="currentCellForeignKeys.length > 0 || foreignKeyMenu.keys.length > 0">
        <div class="menu-divider"></div>
        <div
          v-for="fk in currentCellForeignKeys"
          :key="`fk-parent-${fk.name}`"
          class="menu-item"
          @click="openForeignKeyQuery(fk.ref_table, fk.ref_column, t('dataTable.fkOpenReferenced', { table: fk.ref_table }))"
        >{{ t('dataTable.fkQueryReferenced', { table: fk.ref_table }) }}</div>
        <div
          v-for="fk in foreignKeyMenu.keys"
          :key="`fk-child-${fk.name}`"
          class="menu-item"
          @click="openForeignKeyQuery(fk.ref_table, fk.column, t('dataTable.fkOpenReferencing', { table: fk.ref_table }))"
        >{{ t('dataTable.fkQueryReferencing', { table: fk.ref_table }) }}</div>
      </template>
      <div class="menu-divider"></div>
      <div class="menu-item" @click="editCell">{{ t('common.edit') }}</div>
    </div>

    <!-- 单元格查看器（大文本 / JSON / BLOB） -->
    <CellViewer
      :open="cellViewer.show"
      :value="cellViewer.value"
      :column-name="cellViewer.columnName"
      :column-type="cellViewer.columnType"
      :driver="driverKind"
      @close="cellViewer.show = false"
    />

    <!-- 关闭 / 切换确认对话框 -->
    <div v-if="showCloseConfirm" class="modal-overlay">
      <div class="confirm-dialog">
        <h3>{{ pendingSwitchTab ? t('dataTable.confirmSwitchTitle') : t('dataTable.confirmCloseTitle') }}</h3>
        <p>{{ pendingSwitchTab ? t('dataTable.confirmSwitchMessage') : t('dataTable.confirmCloseMessage') }}</p>
        <div class="confirm-actions">
          <button @click="confirmClose(false)" class="btn-secondary">
            {{ pendingSwitchTab ? t('dataTable.discardAndSwitch') : t('dataTable.discardAndClose') }}
          </button>
          <button @click="confirmClose(true)" class="btn-primary">
            {{ pendingSwitchTab ? t('dataTable.saveAndSwitch') : t('dataTable.saveAndClose') }}
          </button>
          <button @click="cancelClose" class="btn-secondary">{{ t('common.cancel') }}</button>
        </div>
      </div>
    </div>

    <!-- 保存预览：确认后在同一事务中执行 -->
    <div v-if="previewDialog.show" class="modal-overlay" @click.self="cancelPreview">
      <div class="dialog save-preview-dialog">
        <h3>{{ t('dataTable.confirmSaveTitle') }}</h3>
        <div class="dialog-content">
          <p class="preview-summary">{{ previewSummary }}</p>
          <p class="preview-hint">{{ t('dataTable.previewHint') }}</p>
          <textarea :value="previewSqlText" readonly rows="14" class="result-textarea"></textarea>
          <p v-if="previewDialog.error" class="preview-error">{{ previewDialog.error }}</p>
        </div>
        <div class="dialog-actions">
          <button @click="cancelPreview" class="btn-secondary" :disabled="previewDialog.saving">{{ t('common.cancel') }}</button>
          <button @click="confirmSave" class="btn-primary" :disabled="previewDialog.saving">
            {{ previewDialog.saving ? t('dataTable.saving') : t('dataTable.saveAndExecute') }}
          </button>
        </div>
      </div>
    </div>

    <!-- 保存成功提示 -->
    <div v-if="saveSuccess.show" class="toast-message" :class="saveSuccess.type">
      {{ saveSuccess.message }}
    </div>

    <!-- 导入数据弹窗 -->
    <ImportDialog ref="importDialogRef" :table="currentTab" @imported="loadData" @toast="showToast" />

    <!-- 导出数据弹窗 -->
    <ExportDialog ref="exportDialogRef" :table="currentTab" @toast="showToast" />

    <!-- 数据生成器（B8） -->
    <DataGeneratorDialog
      v-if="currentTab"
      :open="showGenerator"
      :connection-id="currentTab.connectionId"
      :database="currentTab.database"
      :table="currentTab.tableName"
      :driver="driverKind"
      @close="showGenerator = false"
      @open-sql="onGeneratorOpenSql"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import {
  useDataBrowserStore,
  type TableChange,
} from '@/stores/dataBrowser'
import { useConnectionStore } from '@/stores/connection'
import { useDebounce } from '@/composables/useDebounce'
import { confirmAction } from '@/composables/useConfirm'
import { useUnsavedGuard } from '@/composables/useUnsavedGuard'
import { useVirtualRows } from '@/composables/useVirtualRows'
import { useTableFilters } from '@/composables/useTableFilters'
import {
  useDateTimePicker,
  isDateTimeType,
  formatDateTimeForInput,
  formatDateTimeFromInput,
} from '@/composables/useDateTimePicker'
import { collectTableChanges, formatSqlValue, isBinaryType, isSameValue } from '@/utils/tableChanges'
import { quoteIdentifier, quoteQualifiedIdentifier } from '@/utils/sql'
import { capabilitiesOf, normalizeDriver, type DriverKind } from '@/utils/driver'
import { t } from '@/i18n'
import CellViewer from '@/components/CellViewer.vue'
import {
  RefreshCw,
  Plus,
  Minus,
  Save,
  ChevronUp,
  ChevronDown,
  Circle,
  X,
  AlignHorizontalDistributeCenter
} from '@/components/icons'
import { ArrowUpDown, BarChart3, Calendar, Columns3, Filter, Pin, Wand2 } from 'lucide-vue-next'
import Select from '@/components/ui/Select.vue'
import PaginationBar from '@/components/datatable/PaginationBar.vue'
import DataGeneratorDialog from '@/components/DataGeneratorDialog.vue'
import ImportDialog from '@/components/datatable/ImportDialog.vue'
import ExportDialog from '@/components/datatable/ExportDialog.vue'
import ResultChart from '@/components/ResultChart.vue'
import { globalToast } from '@/composables/useToast'

interface TableTab {
  id: string
  connectionId: string
  database: string
  tableName: string
  name: string
  modified: boolean
  originalData?: unknown
}

interface ModifiedCell {
  rowIndex: number
  colIndex: number
  oldValue: unknown
  newValue: unknown
  oldRowData: unknown[] // 存储原始行数据用于构建 WHERE 子句
}

interface NewRow {
  data: unknown[]
  isNew: true
}

const browserStore = useDataBrowserStore()
const connectionStore = useConnectionStore()

// 表标签真正关闭后通知外层，保证外层 Tab 与内部表状态同步
const emit = defineEmits<{
  (e: 'table-closed', tab: { connectionId: string; database: string; tableName: string }): void
  /** 请求外层新开一个 SQL 标签页（外键引用导航等） */
  (e: 'open-sql', sql: string, title: string, connectionId: string): void
}>()

// 标签页管理
const openTables = ref<TableTab[]>([])
const currentTab = ref<TableTab | null>(null)
const currentTableData = computed(() => browserStore.tableData)
const loading = computed(() => browserStore.loading)

/** 「取消查询」延迟显示阈值：加载持续超过该时长才允许取消 */
const CANCEL_VISIBLE_DELAY = 500
const slowLoading = ref(false)
let slowLoadingTimer: ReturnType<typeof setTimeout> | null = null

watch(loading, (active) => {
  if (slowLoadingTimer) {
    clearTimeout(slowLoadingTimer)
    slowLoadingTimer = null
  }
  if (active) {
    slowLoadingTimer = setTimeout(() => {
      slowLoading.value = true
    }, CANCEL_VISIBLE_DELAY)
  } else {
    slowLoading.value = false
  }
})

/** 快速加载（打开/刷新表格）时保持刷新按钮，仅慢查询才切换为「取消查询」 */
const showCancelLoad = computed(() => loading.value && slowLoading.value)

onUnmounted(() => {
  if (slowLoadingTimer) clearTimeout(slowLoadingTimer)
})
const loadError = ref('')

// 分页和排序
const page = computed(() => browserStore.page)
const pageSize = computed(() => browserStore.pageSize)
const sortColumn = ref('')
const sortDirection = ref<'Asc' | 'Desc'>('Asc')

// 列宽管理
const defaultColumnWidth = 150

// 根据数据类型获取列宽
function getColumnWidthByType(dataType: string): number {
  const type = dataType.toLowerCase()
  // ID/主键列
  if (type.includes('int') || type.includes('bigint') || type.includes('serial')) {
    return 80
  }
  // 小整数
  if (type.includes('tinyint') || type.includes('smallint') || type.includes('mediumint')) {
    return 60
  }
  // 布尔值
  if (type.includes('bool')) {
    return 60
  }
  // 日期时间
  if (type.includes('datetime') || type.includes('timestamp')) {
    return 160
  }
  // 日期
  if (type.includes('date')) {
    return 100
  }
  // 时间
  if (type.includes('time')) {
    return 80
  }
  // 小数
  if (type.includes('decimal') || type.includes('numeric') || type.includes('float') || type.includes('double')) {
    return 100
  }
  // 枚举/集合
  if (type.includes('enum') || type.includes('set')) {
    return 120
  }
  // JSON
  if (type.includes('json')) {
    return 200
  }
  // 文本/大字段
  if (type.includes('text') || type.includes('blob')) {
    return 250
  }
  // 字符串
  if (type.includes('varchar') || type.includes('char')) {
    return 150
  }
  // 二进制
  if (type.includes('binary') || type.includes('varbinary')) {
    return 150
  }
  // 默认
  return 120
}

const columnWidths = ref<number[]>([])

// 编辑状态
const editingCell = ref<{ row: number; col: number } | null>(null)
const editingValue = ref('')
const editingColumnType = ref('') // 当前编辑列的数据类型

// 自定义日期时间选择器（状态与交互逻辑见 useDateTimePicker）
const {
  showDateTimePicker,
  pickerPosition,
  currentYear,
  currentMonth,
  selectedDate,
  selectedHour,
  selectedMinute,
  selectedSecond,
  calendarDays,
  initDateTimePicker,
  openDateTimePicker,
  prevMonth,
  nextMonth,
  selectDate,
  updateTime,
  applyDateTime,
} = useDateTimePicker({
  editingValue,
  editingColumnType,
  finishEdit,
})

const modifiedCells = ref<Map<string, ModifiedCell>>(new Map())
const newRows = ref<NewRow[]>([])
interface DeletedRow {
  index: number
  rowData: unknown[]
}
const deletedRows = ref<DeletedRow[]>([])

// 行选择
const selectedRows = ref<Set<number>>(new Set())

// 搜索
const searchText = ref('')
// 当前生效的搜索关键字（由后端执行，用于分页/排序复用，并避免重复请求）
const activeSearch = ref('')
const { debouncedValue: debouncedSearchText, flush: flushSearch } = useDebounce(searchText, 300)

// 右键菜单
const contextMenu = ref({
  show: false,
  style: { top: '0px', left: '0px' },
  rowIndex: -1,
  colIndex: -1,
  cellValue: null as unknown
})

// 外键引用导航（B2）：右键某列时异步查询有哪些子表引用它
interface ForeignKeyMeta {
  name: string
  column: string
  ref_table: string
  ref_column: string
}
const foreignKeyMenu = ref<{ loading: boolean; keys: ForeignKeyMeta[] }>({ loading: false, keys: [] })
let foreignKeyRequestSeq = 0

// 单元格查看器状态（记录列名/类型，数据刷新后仍可正确展示）
const cellViewer = ref({
  show: false,
  value: null as unknown,
  columnName: '',
  columnType: ''
})

// 关闭确认
const showCloseConfirm = ref(false)
const tabToClose = ref<TableTab | null>(null)
// 有未保存修改时待切换的目标表（与 tabToClose 互斥，复用同一个确认框）
const pendingSwitchTab = ref<TableTab | null>(null)

// 保存提示
const saveSuccess = ref({
  show: false,
  message: '',
  type: 'success'
})

// 保存预览（SQL 预览 + 确认后在同一事务中执行）
const previewDialog = ref({
  show: false,
  changes: [] as TableChange[],
  saving: false,
  error: ''
})
// 从「保存并关闭」进入预览时为 true，保存成功后继续关闭标签页
const pendingCloseAfterSave = ref(false)

// 事务状态
const transactionActive = ref(false)

// 取消查询会把事务连接一并丢弃（后端自动回滚），这里同步复位，避免界面仍显示「事务活跃」
watch(
  () => connectionStore.transactionResetSeq,
  () => {
    if (transactionActive.value) {
      transactionActive.value = false
      showToast(t('dataTable.transactionRolledBack'))
    }
  }
)

// 导入导出弹窗（组件自身持有状态，父组件只负责触发打开与转发 toast）
const importDialogRef = ref<InstanceType<typeof ImportDialog> | null>(null)
const exportDialogRef = ref<InstanceType<typeof ExportDialog> | null>(null)

function showImportDialog() {
  importDialogRef.value?.open()
}

function showExportDialog() {
  exportDialogRef.value?.open()
}

// 每页条数选项
const pageSizeOptions = computed(() =>
  [50, 100, 500, 1000].map(size => ({
    label: t('dataTable.rowsPerPage', { count: size }),
    value: String(size),
  }))
)

// ========== 宽表按需列加载 ==========
interface TableColumnMeta {
  name: string
  data_type: string
  is_primary: boolean
}

/** 默认展示的列数上限；超过则只取前 N 列 + 主键列，避免宽表一次拉取 200+ 列 */
const DEFAULT_VISIBLE_COLUMNS = 30

/** 每张表的结构列元数据（按 tab id 缓存，只取一次） */
const tableColumns = ref<Record<string, TableColumnMeta[]>>({})
/** 每张表的外键（按 tab id 缓存），用于「查它引用的行」 */
const tableForeignKeys = ref<Record<string, ForeignKeyMeta[]>>({})
/** 每张表当前勾选要查询的列 */
const selectedColumns = ref<Record<string, string[]>>({})
const showColumnSelector = ref(false)

/** 结果集图表视图开关：打开后以图表展示当前结果集（V3-1.1） */
const chartView = ref(false)

// 列选择菜单 Teleport 到 body 后按按钮位置固定定位，不受工具栏 overflow 裁剪
const columnSelectorBtn = ref<HTMLElement | null>(null)
const columnSelectorMenu = ref<HTMLElement | null>(null)
const columnSelectorMenuStyle = ref<Record<string, string>>({})

/** 打开列选择菜单：按按钮位置计算，靠近窗口底部时向上展开 */
function toggleColumnSelector() {
  showColumnSelector.value = !showColumnSelector.value
  if (!showColumnSelector.value || !columnSelectorBtn.value) return
  const rect = columnSelectorBtn.value.getBoundingClientRect()
  const top = rect.bottom + 4
  // 360 为菜单 max-height，底部空间不足时改为向上展开
  if (top + 360 > window.innerHeight && rect.top - 4 - 360 > 0) {
    columnSelectorMenuStyle.value = {
      position: 'fixed',
      left: `${Math.round(rect.left)}px`,
      bottom: `${Math.round(window.innerHeight - rect.top + 4)}px`,
    }
  } else {
    columnSelectorMenuStyle.value = {
      position: 'fixed',
      left: `${Math.round(rect.left)}px`,
      top: `${Math.round(top)}px`,
    }
  }
}

/** 点击菜单与触发按钮之外的区域时收起菜单 */
function handleColumnSelectorDocClick(e: MouseEvent) {
  if (!showColumnSelector.value) return
  const target = e.target as Node
  if (columnSelectorMenu.value?.contains(target)) return
  if (columnSelectorBtn.value?.contains(target)) return
  showColumnSelector.value = false
}
/** 是否处于游标续页模式（首页返回 next_cursor 时开启） */
const cursorMode = ref(false)
const loadingMore = computed(() => browserStore.loadingMore)

const currentTableKey = computed(() => currentTab.value?.id ?? '')
const allColumnsForTab = computed(() => tableColumns.value[currentTableKey.value] ?? [])
const selectedColumnNames = computed(() => selectedColumns.value[currentTableKey.value] ?? [])
const loadedColumnNames = computed(() => currentTableData.value?.columns.map(col => col.name) ?? [])

/** 勾选结果与当前已加载列是否不同（用于「应用」按钮的可用状态） */
const colSelectionChanged = computed(() => {
  const selected = selectedColumnNames.value
  const loaded = loadedColumnNames.value
  if (selected.length === 0 || loaded.length === 0) return false
  if (selected.length !== loaded.length) return true
  const loadedSet = new Set(loaded)
  return selected.some(name => !loadedSet.has(name))
})

/** 触底加载更多：仅在游标模式下、还有下一页且没有本地新增行时可用 */
const canLoadMore = computed(() =>
  cursorMode.value &&
  currentTableData.value?.next_cursor != null &&
  !loadingMore.value &&
  !loading.value &&
  newRows.value.length === 0
)

/** 默认列选择：列数不超过上限时全选，否则取前 N 列并强制保留主键列 */
function defaultColumnSelection(columns: TableColumnMeta[]): string[] {
  if (columns.length <= DEFAULT_VISIBLE_COLUMNS) return columns.map(col => col.name)
  // 保持表结构顺序：前 N 列 + 位置靠后的主键列
  return columns
    .filter((col, index) => index < DEFAULT_VISIBLE_COLUMNS || col.is_primary)
    .map(col => col.name)
}

/** 首次打开某张表时取列元数据，失败则回退为 SELECT *（不影响数据浏览） */
async function ensureTableColumns(tab: TableTab) {
  if (tableColumns.value[tab.id]) return
  try {
    const structure = await browserStore.getTableStructure(tab.connectionId, tab.database, tab.tableName)
    const columns: TableColumnMeta[] = structure.columns.map(col => ({
      name: col.name,
      data_type: col.data_type,
      is_primary: col.is_primary,
    }))
    tableColumns.value = { ...tableColumns.value, [tab.id]: columns }
    tableForeignKeys.value = { ...tableForeignKeys.value, [tab.id]: structure.foreign_keys ?? [] }
    selectedColumns.value = { ...selectedColumns.value, [tab.id]: defaultColumnSelection(columns) }
  } catch (e) {
    console.error('Failed to load table columns:', e)
  }
}

function isColumnSelected(name: string): boolean {
  return selectedColumnNames.value.includes(name)
}

function toggleColumnSelection(name: string) {
  const column = allColumnsForTab.value.find(col => col.name === name)
  // 主键列用于游标翻页与行定位，不允许取消
  if (!column || column.is_primary) return
  const selectedSet = new Set(selectedColumnNames.value)
  if (selectedSet.has(name)) {
    selectedSet.delete(name)
  } else {
    selectedSet.add(name)
  }
  // 按表结构顺序回填，保证查询列顺序稳定
  const next = allColumnsForTab.value.map(col => col.name).filter(colName => selectedSet.has(colName))
  selectedColumns.value = { ...selectedColumns.value, [currentTableKey.value]: next }
}

function selectAllColumns() {
  selectedColumns.value = {
    ...selectedColumns.value,
    [currentTableKey.value]: allColumnsForTab.value.map(col => col.name),
  }
}

function resetColumnSelection() {
  selectedColumns.value = {
    ...selectedColumns.value,
    [currentTableKey.value]: defaultColumnSelection(allColumnsForTab.value),
  }
}

/** 应用列选择：回到第一页重新查询 */
function applyColumns() {
  showColumnSelector.value = false
  browserStore.setPage(1)
  cursorMode.value = false
  loadData()
}

// 导入 / 导出格式选项已移入 ImportDialog / ExportDialog 组件

function onPageSizeChange(value: string) {
  browserStore.setPageSize(Number(value))
  reload()
}

// 计算属性
const totalPages = computed(() => {
  if (!currentTableData.value) return 1
  return Math.ceil(currentTableData.value.total_count / pageSize.value)
})

const hasUnsavedChanges = computed(() => {
  return currentTab.value?.modified ||
         modifiedCells.value.size > 0 ||
         newRows.value.length > 0 ||
         deletedRows.value.length > 0
})

// 关闭窗口/刷新前的兜底提示，避免直接丢失未保存的单元格修改
useUnsavedGuard('data-table', hasUnsavedChanges)

const hasSelection = computed(() => selectedRows.value.size > 0)

// 没有主键就无法安全定位行，此时禁止编辑（避免全列匹配误伤）
const hasPrimaryKey = computed(() => (currentTableData.value?.columns ?? []).some(col => col.is_primary))

// 驱动能力集中判断：写路径是否可用由能力表决定（PG P1 已支持保存表数据；
// 后续 sqlite 等驱动在能力表里把 editableTableData 置 false 即可禁用编辑）
const driverKind = computed<DriverKind>(() => normalizeDriver(connectionStore.currentConnection?.driver))
const driverCaps = computed(() => capabilitiesOf(driverKind.value))

// ========== 过滤栏（列头筛选 + 多条件构建器 + 已存过滤器）：逻辑见 useTableFilters ==========
const {
  showFilterPanel,
  filterRows,
  filterLogic,
  activeFilters,
  activeFilterLogic,
  selectedSavedFilter,
  newFilterName,
  filterOperatorOptions,
  filterColumnChoices,
  savedFilterChoices,
  isNullOperator,
  toFilterConditions,
  addFilterRow,
  removeFilterRow,
  filterWherePreview,
  copyWherePreview,
  applyFilters,
  clearFilters,
  saveCurrentFilter,
  applySavedFilter,
  deleteSavedFilter,
  resetFilters,
} = useTableFilters({
  currentTab,
  allColumnsForTab,
  driverKind,
  setPage: (p: number) => browserStore.setPage(p),
  loadData,
  hasUnsavedChanges: () => hasUnsavedChanges.value,
  showToast,
})


// 只读连接同样禁止写入（后端也会兜底拦截）
const isReadOnlyConnection = computed(
  () => !driverCaps.value.editableTableData ||
    connectionStore.currentConnection?.safety_mode === 'read-only'
)

const readOnlyHint = computed(() => t('dataTable.readOnlyConnectionHint'))

const readOnlyBadgeLabel = computed(() => t('dataTable.readOnlyConnection'))

// 编辑拦截原因：只读连接优先提示，其次是无主键表
const editBlockReason = computed(() => {
  if (isReadOnlyConnection.value) return readOnlyHint.value
  if (!hasPrimaryKey.value) return t('dataTable.noPrimaryKeyEditReason')
  return ''
})

const canEdit = computed(() => editBlockReason.value === '')

const isAllSelected = computed(() => {
  if (!currentTableData.value?.rows.length) return false
  return selectedRows.value.size === currentTableData.value.rows.length
})

// 过滤后的行（搜索已下沉到后端，这里不再做客户端过滤，
// 否则会与后端的 LIKE 语义不一致，且只能覆盖当前页）
const filteredRows = computed(() => currentTableData.value?.rows ?? [])

// 大表虚拟滚动：只渲染可视区内的行，行索引仍对应 filteredRows
const tableContainer = ref<HTMLElement | null>(null)
const {
  enabled: virtualEnabled,
  start: virtualStart,
  end: virtualEnd,
  paddingTop: virtualPaddingTop,
  paddingBottom: virtualPaddingBottom,
  onScroll: handleVirtualScroll,
} = useVirtualRows(tableContainer, () => filteredRows.value.length, {
  rowHeight: 28,
  threshold: 100,
})

const visibleRows = computed(() => {
  const rows = filteredRows.value
  if (!virtualEnabled.value) {
    return rows.map((row, index) => ({ row, index }))
  }
  return rows
    .slice(virtualStart.value, virtualEnd.value)
    .map((row, offset) => ({ row, index: virtualStart.value + offset }))
})

// 监听表数据变化，初始化列宽
watch(() => currentTableData.value?.columns, (columns) => {
  if (columns && columnWidths.value.length !== columns.length) {
    columnWidths.value = columns.map(col => getColumnWidthByType(col.data_type))
  }
}, { immediate: true })

// 键盘快捷键
onMounted(() => {
  document.addEventListener('keydown', handleKeyDown)
  document.addEventListener('click', handleColumnSelectorDocClick)

  // v-show 切走再切回时，display:none 期间的 scrollTop 已被浏览器清零；
  // 用 IntersectionObserver 感知表格重新可见，从记忆恢复上次浏览位置
  const el = tableContainer.value
  if (el && typeof IntersectionObserver !== 'undefined') {
    tableViewObserver = new IntersectionObserver((entries) => {
      if (entries.some(entry => entry.isIntersecting)) {
        restoreSavedScroll(scrollMemoryKey(
          browserStore.currentDatabase ?? '',
          browserStore.currentTable ?? '',
          connectionStore.currentConnection?.id ?? ''
        ))
      }
    }, { threshold: 0 })
    tableViewObserver.observe(el)
  }
})

onUnmounted(() => {
  document.removeEventListener('keydown', handleKeyDown)
  document.removeEventListener('click', handleColumnSelectorDocClick)
  // 拖拽列宽中卸载时清理全局监听
  document.removeEventListener('mousemove', handleResize)
  document.removeEventListener('mouseup', stopResize)
  clearPendingEdit()
  if (toastTimer) clearTimeout(toastTimer)
  tableViewObserver?.disconnect()
  tableViewObserver = null
})

function handleKeyDown(e: KeyboardEvent) {
  // 未打开表或当前视图被 v-show 隐藏时，不响应全局快捷键，
  // 否则在 SQL 编辑器里按 Delete/F5/Ctrl+S 会误操作数据表
  if (!isTableViewActive()) return

  // 检查当前焦点是否在数据表格区域内
  const target = e.target as HTMLElement | null
  const isInDataTable = target?.closest('.data-table') !== null
  if (!isInDataTable) return

  // 正在单元格/输入框中编辑文本时，Delete 属于文本编辑操作，不能删除整行
  const isEditingText = target?.closest('input, textarea, [contenteditable="true"]') !== null

  // 快捷键同时支持 Ctrl（Windows/Linux）与 Cmd（macOS）
  const modKey = e.ctrlKey || e.metaKey

  // Ctrl/Cmd+S 保存
  if (modKey && e.key === 's') {
    e.preventDefault()
    if (hasUnsavedChanges.value) saveChanges()
  }
  // Ctrl/Cmd+N 新增行
  if (modKey && e.key === 'n') {
    e.preventDefault()
    addRow()
  }
  // Delete 删除选中行
  if (e.key === 'Delete' && !isEditingText && hasSelection.value) {
    deleteSelectedRows()
  }
  // F5 刷新
  if (e.key === 'F5') {
    e.preventDefault()
    refreshData()
  }
  // Ctrl/Cmd+A 全选 - 只在焦点在数据表格内且不在编辑文本时生效
  if (modKey && e.key === 'a' && !isEditingText) {
    e.preventDefault()
    toggleSelectAll()
  }
}

// 当前表视图是否真正对用户可见（有打开的表，且没有被 v-show 隐藏）
function isTableViewActive(): boolean {
  if (!currentTab.value) return false
  const el = tableContainer.value
  return !!el && el.offsetParent !== null
}


// 打开新表
function openTable(connectionId: string, database: string, tableName: string) {
  const tabId = `${connectionId}.${database}.${tableName}`
  const existingTab = openTables.value.find(t => t.id === tabId)

  if (existingTab) {
    // 已是当前表则不重复切换/加载，避免切回同一表时重复请求
    if (currentTab.value?.id !== existingTab.id) {
      switchTab(existingTab)
    }
    return
  }

  const newTab: TableTab = {
    id: tabId,
    connectionId,
    database,
    tableName,
    name: `${database}.${tableName}`,
    modified: false
  }

  openTables.value.push(newTab)
  switchTab(newTab)
}

// 切换标签页：当前表有未保存修改时先确认，避免静默丢弃
async function switchTab(tab: TableTab) {
  const previousTab = currentTab.value
  if (previousTab && previousTab.id !== tab.id && hasUnsavedChanges.value) {
    tabToClose.value = null
    pendingSwitchTab.value = tab
    showCloseConfirm.value = true
    return
  }

  await activateTab(tab)
}

// 真正切换到目标表并加载数据
async function activateTab(tab: TableTab) {
  const previousTab = currentTab.value
  currentTab.value = tab
  browserStore.currentDatabase = tab.database
  browserStore.currentTable = tab.tableName

  // 切换到对应连接
  connectionStore.switchConnection(tab.connectionId)

  // 切换表即丢弃上一张表的未保存状态，同步复位脏标记，避免切回后显示「假脏」
  if (previousTab && previousTab.id !== tab.id) {
    previousTab.modified = false
  }

  // 清除修改状态
  modifiedCells.value.clear()
  newRows.value = []
  deletedRows.value = []
  selectedRows.value.clear()
  searchText.value = ''
  activeSearch.value = ''
  flushSearch()
  resetFilters()
  
  // 重置排序状态，避免使用之前表的排序列
  sortColumn.value = ''
  sortDirection.value = 'Asc'
  showColumnSelector.value = false
  cursorMode.value = false
  // 切换表从第一页开始，避免沿用上一张表的页码（游标模式依赖首页语义）
  browserStore.setPage(1)

  // 先取列元数据（宽表按需加载的前提），再查询数据
  await ensureTableColumns(tab)
  // 等待期间可能已切换到别的表，过期的激活不再发请求
  if (currentTab.value?.id !== tab.id) return
  await loadData()

  // 恢复该表上次的浏览位置（关闭重开/切回不再回到顶部）
  await nextTick()
  if (currentTab.value?.id !== tab.id) return
  restoreSavedScroll(scrollMemoryKey(tab.database, tab.tableName, tab.connectionId))
}

// 关闭标签页：立即关闭返回 true；有未保存修改时弹确认框并返回 false，由确认结果收尾
function closeTab(tab: TableTab): boolean {
  if (tab.modified || hasUnsavedChanges.value) {
    pendingSwitchTab.value = null
    tabToClose.value = tab
    showCloseConfirm.value = true
    return false
  }

  doCloseTab(tab)
  return true
}

async function confirmClose(save: boolean) {
  const switching = pendingSwitchTab.value

  if (save && hasUnsavedChanges.value) {
    // 有真实变更时走保存预览，保存成功后由 confirmSave 继续关闭标签页 / 切换目标表
    if (collectChanges().length > 0) {
      saveChanges()
      return
    }
    // 只有脏标记、没有实际变更时直接丢弃并继续
  }

  if (switching) {
    pendingSwitchTab.value = null
    showCloseConfirm.value = false
    await activateTab(switching)
    return
  }

  closeAfterSave()
}

// 完成「保存并关闭」：关闭最初请求关闭的标签页
function closeAfterSave() {
  if (tabToClose.value) {
    doCloseTab(tabToClose.value)
    tabToClose.value = null
  }
  showCloseConfirm.value = false
}

function cancelClose() {
  showCloseConfirm.value = false
  tabToClose.value = null
  pendingSwitchTab.value = null
}

function doCloseTab(tab: TableTab) {
  const index = openTables.value.findIndex(t => t.id === tab.id)
  if (index === -1) return
  openTables.value.splice(index, 1)

  // 清理该表遗留的编辑记录，避免重新打开同一张表时脏状态复活
  modifiedCells.value.forEach((_, key) => {
    if (key.startsWith(`${tab.id}.`)) modifiedCells.value.delete(key)
  })

  if (currentTab.value?.id === tab.id) {
    currentTab.value = openTables.value[0] || null
    if (currentTab.value) {
      switchTab(currentTab.value)
    } else {
      browserStore.tableData = null
    }
  }

  emit('table-closed', {
    connectionId: tab.connectionId,
    database: tab.database,
    tableName: tab.tableName
  })
}

// 关闭指定连接的所有表格
function closeConnectionTables(connectionId: string) {
  const tabsToClose = openTables.value.filter(t => t.connectionId === connectionId)
  tabsToClose.forEach(tab => {
    // 清除相关修改记录
    modifiedCells.value.forEach((_, key) => {
      if (key.startsWith(tab.id)) {
        modifiedCells.value.delete(key)
      }
    })
  })
  // 从 openTables 中移除
  openTables.value = openTables.value.filter(t => t.connectionId !== connectionId)

  // 如果当前标签页被关闭，切换到其他标签页
  if (currentTab.value?.connectionId === connectionId) {
    currentTab.value = openTables.value[0] || null
    if (currentTab.value) {
      switchTab(currentTab.value)
    } else {
      browserStore.tableData = null
    }
  }
}

// 根据 ID 关闭表（供外部调用）：返回是否已立即关闭
function closeTableById(tabId: string): boolean {
  const tab = openTables.value.find(t => t.id === tabId)
  if (!tab) return true
  return closeTab(tab)
}

// 加载数据
async function loadData() {
  const conn = connectionStore.currentConnection
  if (!conn || !browserStore.currentDatabase || !browserStore.currentTable) return

  loadError.value = ''
  try {
    const result = await browserStore.loadTableData(conn.id, browserStore.currentDatabase, browserStore.currentTable, {
      page: page.value,
      pageSize: pageSize.value,
      sortColumn: sortColumn.value || undefined,
      sortDirection: sortDirection.value?.toLowerCase() as 'asc' | 'desc' | undefined,
      search: activeSearch.value || undefined,
      filters: toFilterConditions(activeFilters.value),
      filterLogic: activeFilterLogic.value === 'OR' ? 'or' : 'and',
      columns: selectedColumnNames.value.length > 0 ? selectedColumnNames.value : undefined,
    })
    // 首页返回游标说明后续翻页可走 keyset，切换为「加载更多」模式
    if (result) cursorMode.value = result.next_cursor != null
  } catch (e) {
    const message = String(e)
    loadError.value = message
    console.error('Failed to load table data:', e)
    showToast(t('dataTable.loadFailed', { error: message }), 'error')
  }
}

// 触底加载更多（游标续页），追加到当前结果末尾
async function loadMore() {
  const conn = connectionStore.currentConnection
  const tab = currentTab.value
  if (!conn || !tab || !canLoadMore.value) return

  try {
    await browserStore.loadMoreTableData(conn.id, tab.database, tab.tableName, {
      pageSize: pageSize.value,
      sortColumn: sortColumn.value || undefined,
      sortDirection: sortDirection.value?.toLowerCase() as 'asc' | 'desc' | undefined,
      search: activeSearch.value || undefined,
      filters: toFilterConditions(activeFilters.value),
      filterLogic: activeFilterLogic.value === 'OR' ? 'or' : 'and',
      columns: selectedColumnNames.value.length > 0 ? selectedColumnNames.value : undefined,
    })
  } catch (e) {
    showToast(t('dataTable.loadMoreFailed', { error: String(e) }), 'error')
  }
}

// 刷新
async function refreshData() {
  if (hasUnsavedChanges.value) {
    const confirmed = await confirmAction({
      message: t('dataTable.refreshConfirm'),
      confirmText: t('common.confirm'),
    })
    if (!confirmed) return
  }
  await loadData()
}

// 取消正在进行的表数据加载（大表 COUNT / 慢查询）
async function cancelLoad() {
  const conn = connectionStore.currentConnection
  if (!conn) return
  try {
    await connectionStore.cancelQuery(conn.id)
  } catch (e) {
    showToast(t('dataTable.cancelLoadFailed', { error: String(e) }), 'error')
  }
}

// 搜索：关键字变化后回到第一页，由后端做整表匹配
watch(debouncedSearchText, async (value) => {
  const keyword = value.trim()
  if (!currentTab.value || keyword === activeSearch.value) return

  if (hasUnsavedChanges.value) {
    const confirmed = await confirmAction({
      message: t('dataTable.searchDiscardConfirm'),
      confirmText: t('common.confirm'),
    })
    if (!confirmed) {
      searchText.value = activeSearch.value
      return
    }
  }

  activeSearch.value = keyword
  browserStore.setPage(1)
  await loadData()
})

// 排序：首次点击降序、再次点击切换升/降。
// 首次点击用降序是因为表格默认按主键升序展示，升序结果与默认视图一致，看起来像「点了没反应」；
// 同时回到第一页并滚回顶部，避免停在第 N 页看排序结果误判为失效。
async function sort(column: string) {
  if (sortColumn.value === column) {
    sortDirection.value = sortDirection.value === 'Asc' ? 'Desc' : 'Asc'
  } else {
    sortColumn.value = column
    sortDirection.value = 'Desc'
  }
  browserStore.setPage(1)
  await loadData()
  scrollTableToTop()
}

// 清除排序：恢复默认（主键）顺序
async function clearSort() {
  if (!sortColumn.value) return
  sortColumn.value = ''
  sortDirection.value = 'Asc'
  browserStore.setPage(1)
  await loadData()
  scrollTableToTop()
}

// 滚动记忆：按「连接:库:表」记住浏览位置，关闭/切换后重开时恢复（不再回到顶部）。
// 表默认按主键序展示，位置语义稳定；排序被复位后仍按默认序恢复，偏差可接受。
const tableScrollMemory = new Map<string, number>()

function scrollMemoryKey(database: string, table: string, connectionId: string): string {
  return `${connectionId}:${database}:${table}`
}

function scrollTableToTop() {
  const el = tableContainer.value
  if (el) el.scrollTop = 0
}

/** 从记忆恢复指定表的滚动位置（钳制在当前可滚动范围内） */
function restoreSavedScroll(key: string) {
  const saved = tableScrollMemory.get(key)
  const el = tableContainer.value
  if (!el || !saved || Math.abs(el.scrollTop - saved) < 1) return
  el.scrollTop = Math.min(saved, Math.max(0, el.scrollHeight - el.clientHeight))
}

/** 表格容器可见性观察器：v-show 重新显示时恢复浏览位置（见 onMounted） */
let tableViewObserver: IntersectionObserver | null = null

// 分页
function firstPage() {
  browserStore.setPage(1)
  loadData()
}

function prevPage() {
  if (page.value > 1) {
    browserStore.setPage(page.value - 1)
    loadData()
  }
}

function nextPage() {
  if (page.value < totalPages.value) {
    browserStore.setPage(page.value + 1)
    loadData()
  }
}

function lastPage() {
  browserStore.setPage(totalPages.value)
  loadData()
}

function reload() {
  browserStore.setPage(1)
  loadData()
}

// 行选择
function selectRow(rowIndex: number, event: MouseEvent) {
  if (event.shiftKey) {
    // Shift 多选
    const lastSelected = Array.from(selectedRows.value).pop() || 0
    const start = Math.min(lastSelected, rowIndex)
    const end = Math.max(lastSelected, rowIndex)
    for (let i = start; i <= end; i++) {
      selectedRows.value.add(i)
    }
  } else if (event.ctrlKey || event.metaKey) {
    // Ctrl 切换选择
    if (selectedRows.value.has(rowIndex)) {
      selectedRows.value.delete(rowIndex)
    } else {
      selectedRows.value.add(rowIndex)
    }
  } else {
    // 单选
    selectedRows.value.clear()
    selectedRows.value.add(rowIndex)
  }
}

function isRowSelected(rowIndex: number): boolean {
  return selectedRows.value.has(rowIndex)
}

function toggleSelectAll() {
  if (isAllSelected.value) {
    selectedRows.value.clear()
  } else {
    currentTableData.value?.rows.forEach((_, index) => {
      selectedRows.value.add(index)
    })
  }
}

// 根据数据类型生成默认值
function getDefaultValue(dataType: string, isNullable: boolean): unknown {
  if (isNullable) return null

  const type = dataType.toLowerCase()

  // 数字类型
  if (type.includes('int') || type.includes('decimal') || type.includes('float') || type.includes('double') || type.includes('numeric') || type.includes('real')) {
    return 0
  }

  // 日期时间类型：MySQL 用零值日期，PG 没有零值日期（0000-00-00 非法），
  // 改用最小合法值，避免新增行时直接写入非法字符串。
  // 注意 timestamp 也包含 'time'，必须先于 time 判断。
  const zeroDates = driverCaps.value.zeroDateDefaults
  if (type.includes('datetime') || type.includes('timestamp')) {
    return zeroDates ? '0000-00-00 00:00:00' : '1970-01-01 00:00:00'
  }
  if (type.includes('date')) return zeroDates ? '0000-00-00' : '1970-01-01'
  if (type.includes('time')) return '00:00:00'
  if (type.includes('year')) return 0

  // 布尔类型
  if (type.includes('bool') || type.includes('bit(1)')) return 0

  // 二进制类型
  if (type.includes('binary') || type.includes('blob')) return ''

  // JSON 类型
  if (type.includes('json')) return '{}'

  // 枚举和集合类型 - 返回第一个值
  if (type.includes('enum')) {
    const match = type.match(/enum\s*\(([^)]+)\)/i)
    if (match && match[1]) {
      const values = match[1].split(',').map(v => v.trim().replace(/['"]/g, ''))
      return values[0] || ''
    }
  }
  if (type.includes('set')) {
    return ''
  }

  // 字符串类型（默认）
  return ''
}

// 新增行
function addRow() {
  if (!canEdit.value) {
    showToast(editBlockReason.value, 'error')
    return
  }
  // 仅加载部分列时，INSERT 会缺少未加载列，先要求勾选全部列
  if (allColumnsForTab.value.length > 0 && selectedColumnNames.value.length < allColumnsForTab.value.length) {
    showToast(t('dataTable.partialColumnsEditHint'), 'error')
    return
  }
  const emptyRow = currentTableData.value?.columns.map(col => {
    return getDefaultValue(col.data_type, col.is_nullable)
  }) || []
  newRows.value.push({ data: emptyRow, isNew: true })
  if (currentTableData.value) {
    currentTableData.value.rows.push(emptyRow)
  }
  if (currentTab.value) {
    currentTab.value.modified = true
  }
}

function isNewRow(rowIndex: number): boolean {
  const dataRowsCount = currentTableData.value?.rows.length || 0
  return rowIndex >= dataRowsCount - newRows.value.length
}

// 删除行
function deleteSelectedRows() {
  if (!canEdit.value) {
    showToast(editBlockReason.value, 'error')
    return
  }
  const indices = Array.from(selectedRows.value).sort((a, b) => b - a)
  indices.forEach(index => {
    // 保存原始行数据到 deletedRows
    if (currentTableData.value) {
      const rowData = currentTableData.value.rows[index]
      if (rowData) {
        deletedRows.value.push({ index, rowData: [...rowData] })
      }
      // 从显示中移除
      currentTableData.value.rows.splice(index, 1)
    }
  })
  selectedRows.value.clear()
  if (currentTab.value) {
    currentTab.value.modified = true
  }
}

// 编辑功能
function startEdit(rowIndex: number, colIndex: number, value: unknown) {
  if (!canEdit.value) {
    showToast(editBlockReason.value, 'error')
    return
  }
  editingCell.value = { row: rowIndex, col: colIndex }

  // 获取列的数据类型
  const col = currentTableData.value?.columns[colIndex]
  const colType = col?.data_type?.toLowerCase() || ''
  editingColumnType.value = colType

  // 根据类型格式化值
  let strValue = value === null ? '' : String(value)
  if (strValue && isDateTimeType(colType)) {
    strValue = formatDateTimeForInput(strValue, colType)
  }
  editingValue.value = strValue

  // 初始化日期时间选择器状态
  if (strValue && isDateTimeType(colType)) {
    initDateTimePicker(strValue, colType)
  }

  nextTick(() => {
    // 日期时间类型不自动聚焦，让用户手动点击选择
    if (!isDateTimeType(colType)) {
      const input = document.querySelector('.edit-input') as HTMLInputElement
      if (input) {
        input.focus()
        input.select()
      }
    }
  })
}

// 日期时间类型判断/格式化工具已移入 useDateTimePicker.ts

function clearEditingState() {
  editingCell.value = null
  editingColumnType.value = ''
  showDateTimePicker.value = false
}

/**
 * 写入单元格并记录变更，所有改动入口（编辑、粘贴、置空）共用：
 * - 与原值等价时不写回、不记录，避免生成 `SET col = 原值` 的空更新
 * - 改回原始值时撤销该单元格的记录，而不是留下一条无意义的更新
 */
function commitCellValue(rowIndex: number, colIndex: number, newValue: unknown) {
  const tab = currentTab.value
  const tableData = currentTableData.value
  if (!tab || !tableData || !tableData.rows[rowIndex]) return

  const cellKey = `${tab.id}.${rowIndex}.${colIndex}`
  const existing = modifiedCells.value.get(cellKey)
  const oldValue = existing ? existing.oldValue : tableData.rows[rowIndex]![colIndex]

  if (isSameValue(oldValue, newValue)) {
    if (existing) {
      modifiedCells.value.delete(cellKey)
      tableData.rows[rowIndex]![colIndex] = oldValue
      if (modifiedCells.value.size === 0 && newRows.value.length === 0 && deletedRows.value.length === 0) {
        tab.modified = false
      }
    }
    return
  }

  // 首次改动时深拷贝整行快照（必须在写回新值之前），否则修改主键列时 WHERE 会用新值定位；
  // 已有记录时沿用最初的快照，避免二次修改把主键的当前值当成原始值
  const oldRowData = existing?.oldRowData
    ? existing.oldRowData
    : JSON.parse(JSON.stringify(tableData.rows[rowIndex]))

  tableData.rows[rowIndex]![colIndex] = newValue
  modifiedCells.value.set(cellKey, {
    rowIndex,
    colIndex,
    oldValue,
    newValue,
    oldRowData
  })
  tab.modified = true
}

function finishEdit() {
  if (!editingCell.value || !currentTab.value) return

  const { row, col } = editingCell.value
  let newValue = editingValue.value === '' ? null : editingValue.value

  // 处理日期时间类型格式转换
  if (newValue && isDateTimeType(editingColumnType.value)) {
    newValue = formatDateTimeFromInput(newValue as string, editingColumnType.value)
  }

  commitCellValue(row, col, newValue)
  clearEditingState()
}

function cancelEdit() {
  clearEditingState()
}

function isEditing(row: number, col: number): boolean {
  return editingCell.value?.row === row && editingCell.value?.col === col
}

function isModified(row: number, col: number): boolean {
  if (!currentTab.value) return false
  const cellKey = `${currentTab.value.id}.${row}.${col}`
  return modifiedCells.value.has(cellKey)
}

// 右键菜单
function showContextMenu(event: MouseEvent, rowIndex: number, colIndex: number, cell: unknown) {
  contextMenu.value = {
    show: true,
    style: {
      top: `${event.clientY}px`,
      left: `${event.clientX}px`
    },
    rowIndex,
    colIndex,
    cellValue: cell
  }
  void loadReferencingForeignKeys(colIndex)
}

function hideContextMenu() {
  contextMenu.value.show = false
  foreignKeyMenu.value = { loading: false, keys: [] }
  // 点击表格其他区域时同时收起列选择器
  showColumnSelector.value = false
}

/** 外键导航用的表名：MySQL 带库名前缀，PG 按 schema.table 分段加引号 */
function qualifiedTableName(table: string): string {
  const driver = driverKind.value
  if (driverCaps.value.tableQualification === 'schema') {
    return quoteQualifiedIdentifier(table, driver)
  }
  const database = currentTab.value?.database
  return database
    ? `${quoteIdentifier(database, driver)}.${quoteIdentifier(table, driver)}`
    : quoteIdentifier(table, driver)
}

/** 按图表映射与方言生成聚合 SQL；超限时下推复制用 200 行上限，存入仪表盘用 1000 */
function buildChartAggregateSql(cfg: { dimension: string; measures: string[]; agg: string; group: string }, limit: number): string {
  const driver = driverKind.value
  const dim = quoteIdentifier(cfg.dimension, driver)
  const selectParts = [dim]
  for (const m of cfg.measures) {
    const qm = quoteIdentifier(m, driver)
    selectParts.push(`${cfg.agg.toUpperCase()}(${qm}) AS ${qm}`)
  }
  const groupParts = [dim]
  if (cfg.group) {
    const qg = quoteIdentifier(cfg.group, driver)
    selectParts.push(qg)
    groupParts.push(qg)
  }
  return `SELECT ${selectParts.join(', ')}\nFROM ${qualifiedTableName(currentTab.value?.tableName ?? '')}\nGROUP BY ${groupParts.join(', ')}\nORDER BY ${cfg.group ? '1, ' : ''}2 DESC\nLIMIT ${limit};`
}

/** 图表原始数据超限时，复制聚合下推 SQL 到剪贴板（V3-1.2） */
async function copyChartPushdownSql(cfg: { dimension: string; measures: string[]; agg: string; group: string }) {
  const sql = buildChartAggregateSql(cfg, 200)
  try {
    await navigator.clipboard.writeText(sql)
    globalToast.success(t('dataTable.chartPushdownCopied'))
  } catch {
    // 剪贴板不可用时静默失败
  }
}

/** 查询引用了当前单元格所在列的外键（子表），供右键菜单展示 */
async function loadReferencingForeignKeys(colIndex: number) {
  const tab = currentTab.value
  const column = currentTableData.value?.columns[colIndex]
  if (!tab || !column) return
  const seq = ++foreignKeyRequestSeq
  foreignKeyMenu.value = { loading: true, keys: [] }
  try {
    const keys = await invoke<ForeignKeyMeta[]>('get_referencing_foreign_keys', {
      connectionId: tab.connectionId,
      database: tab.database,
      table: tab.tableName,
      column: column.name
    })
    if (seq === foreignKeyRequestSeq) foreignKeyMenu.value = { loading: false, keys }
  } catch (e) {
    console.error('Failed to load referencing foreign keys:', e)
    if (seq === foreignKeyRequestSeq) foreignKeyMenu.value = { loading: false, keys: [] }
  }
}

/** 当前单元格所在列指向其它表的外键（「查它引用的行」） */
const currentCellForeignKeys = computed<ForeignKeyMeta[]>(() => {
  const tab = currentTab.value
  const column = currentTableData.value?.columns[contextMenu.value.colIndex]
  if (!tab || !column) return []
  return (tableForeignKeys.value[tab.id] ?? []).filter(fk => fk.column === column.name)
})

/** 用当前单元格的值生成一条只读查询，并在新 SQL 标签页打开 */
function openForeignKeyQuery(targetTable: string, targetColumn: string, title: string) {
  const value = contextMenu.value.cellValue
  if (value === null || value === undefined) {
    showToast(t('dataTable.fkNullValue'))
    return
  }
  const driver = driverKind.value
  const literal = formatSqlValue(value, { driver })
  const sql = `SELECT *\nFROM ${qualifiedTableName(targetTable)}\nWHERE ${quoteIdentifier(targetColumn, driver)} = ${literal}\nLIMIT 200;`
  emit('open-sql', sql, title, currentTab.value?.connectionId ?? '')
  hideContextMenu()
}

/** 数据生成器弹窗开关（B8） */
const showGenerator = ref(false)

/** 生成的数据脚本交给外层在新 SQL 标签页打开 */
function onGeneratorOpenSql(sql: string, title: string) {
  emit('open-sql', sql, title, currentTab.value?.connectionId ?? '')
}

function copyCell() {
  const value = contextMenu.value.cellValue
  navigator.clipboard.writeText(value === null ? 'NULL' : String(value))
  hideContextMenu()
  showToast(t('dataTable.copiedToClipboard'))
}

function copyRow() {
  const { rowIndex } = contextMenu.value
  const row = currentTableData.value?.rows[rowIndex]
  const columns = currentTableData.value?.columns
  if (!row || !columns) return
  const tsv = row.map((cell: unknown) => cell === null ? 'NULL' : String(cell).replace(/\t|\n/g, ' ')).join('\t')
  navigator.clipboard.writeText(tsv)
  hideContextMenu()
  showToast(t('dataTable.copiedToClipboard'))
}

function copyColumn() {
  const { colIndex } = contextMenu.value
  const rows = currentTableData.value?.rows
  if (!rows) return
  const values = rows.map((row: unknown[]) => {
    const cell = row[colIndex]
    return cell === null ? 'NULL' : String(cell)
  })
  navigator.clipboard.writeText(values.join('\n'))
  hideContextMenu()
  showToast(t('dataTable.copiedToClipboard'))
}

function copyHeaderRow() {
  const columns = currentTableData.value?.columns
  if (!columns) return
  navigator.clipboard.writeText(columns.map(c => c.name).join('\t'))
  hideContextMenu()
  showToast(t('dataTable.copiedToClipboard'))
}

function copyAllRows() {
  const rows = currentTableData.value?.rows
  const columns = currentTableData.value?.columns
  if (!rows || !columns) return
  const lines = [columns.map(c => c.name).join('\t')]
  for (const row of rows) {
    lines.push(row.map((cell: unknown) => cell === null ? 'NULL' : String(cell).replace(/\t|\n/g, ' ')).join('\t'))
  }
  navigator.clipboard.writeText(lines.join('\n'))
  hideContextMenu()
  showToast(t('dataTable.copiedToClipboard'))
}

/** 复制所有选中行（TSV），按选中顺序排序 */
function copySelectedRows() {
  const selectedIndices = Array.from(selectedRows.value).sort((a, b) => a - b)
  const rows = filteredRows.value
  if (!rows || selectedIndices.length === 0) return
  const lines = selectedIndices.map(idx => {
    const row = rows[idx]
    if (!row) return ''
    return row.map((cell: unknown) => cell === null ? 'NULL' : String(cell).replace(/\t|\n/g, ' ')).join('\t')
  })
  navigator.clipboard.writeText(lines.join('\n'))
  hideContextMenu()
  showToast(t('dataTable.copiedToClipboard'))
}

async function pasteCell() {
  if (!canEdit.value) {
    hideContextMenu()
    showToast(editBlockReason.value, 'error')
    return
  }
  try {
    const text = await navigator.clipboard.readText()
    const { rowIndex, colIndex } = contextMenu.value
    commitCellValue(rowIndex, colIndex, text)
    hideContextMenu()
  } catch {
    showToast(t('dataTable.pasteFailed'), 'error')
  }
}

function setNull() {
  if (!canEdit.value) {
    hideContextMenu()
    showToast(editBlockReason.value, 'error')
    return
  }
  const { rowIndex, colIndex } = contextMenu.value
  commitCellValue(rowIndex, colIndex, null)
  hideContextMenu()
}

function viewFullContent() {
  openCellViewer(contextMenu.value.rowIndex, contextMenu.value.colIndex, contextMenu.value.cellValue)
}

function editCell() {
  const { rowIndex, colIndex, cellValue } = contextMenu.value
  startEdit(rowIndex, colIndex, cellValue)
  hideContextMenu()
}

/** 大文本 / JSON / BLOB 才有必要进独立查看器 */
const VIEWER_TEXT_THRESHOLD = 200

function isViewerWorthyCell(cell: unknown, colIndex: number): boolean {
  if (cell === null || cell === undefined) return false
  const type = (currentTableData.value?.columns[colIndex]?.data_type ?? '').toLowerCase()
  if (isBinaryType(type) || type.includes('json')) return true
  return String(cell).length > VIEWER_TEXT_THRESHOLD
}

function openCellViewer(rowIndex: number, colIndex: number, cell: unknown) {
  const column = currentTableData.value?.columns[colIndex]
  cellViewer.value = {
    show: true,
    value: cell,
    columnName: column?.name ?? '',
    columnType: column?.data_type ?? ''
  }
  hideContextMenu()
}

// 查看器优先的单元格延迟进入编辑，为双击打开查看器留出判定窗口
let pendingEditTimer: ReturnType<typeof setTimeout> | null = null

function clearPendingEdit() {
  if (!pendingEditTimer) return
  clearTimeout(pendingEditTimer)
  pendingEditTimer = null
}

function onCellClick(event: MouseEvent, rowIndex: number, colIndex: number, cell: unknown) {
  // 双击的第二次点击不再重复进入编辑，避免重置正在编辑的值
  if (event.detail > 1) return
  clearPendingEdit()
  if (!isViewerWorthyCell(cell, colIndex)) {
    startEdit(rowIndex, colIndex, cell)
    return
  }
  pendingEditTimer = setTimeout(() => {
    pendingEditTimer = null
    startEdit(rowIndex, colIndex, cell)
  }, 250)
}

function onCellDblClick(rowIndex: number, colIndex: number, cell: unknown) {
  clearPendingEdit()
  if (isViewerWorthyCell(cell, colIndex)) {
    openCellViewer(rowIndex, colIndex, cell)
    return
  }
  // 普通单元格保持原有行为：双击不改变编辑状态
  if (!isEditing(rowIndex, colIndex)) startEdit(rowIndex, colIndex, cell)
}

// 汇总当前所有未保存的修改（SQL 由前端生成，后端在同一事务中执行）
function collectChanges(): TableChange[] {
  const tab = currentTab.value
  const columns = currentTableData.value?.columns
  if (!tab || !columns) return []

  return collectTableChanges({
    table: tab.tableName,
    columns,
    edits: modifiedCells.value.values(),
    inserts: newRows.value.map(row => row.data),
    deletes: deletedRows.value.map(row => row.rowData),
    driver: driverKind.value
  })
}

function changeKindLabel(kind: TableChange['kind']): string {
  if (kind === 'update') return t('dataTable.changeUpdate')
  if (kind === 'insert') return t('dataTable.changeInsert')
  return t('dataTable.changeDelete')
}

// 预览弹窗：摘要与逐条 SQL
const previewSummary = computed(() => {
  const counts: Record<TableChange['kind'], number> = { update: 0, insert: 0, delete: 0 }
  for (const change of previewDialog.value.changes) counts[change.kind] += 1
  const total = previewDialog.value.changes.length
  return t('dataTable.previewSummary', {
    total,
    update: counts.update,
    insert: counts.insert,
    delete: counts.delete
  })
})

const previewSqlText = computed(() =>
  previewDialog.value.changes
    .map((change, index) => `-- ${index + 1}. ${changeKindLabel(change.kind)}\n${change.sql};`)
    .join('\n\n')
)

// 保存：先收集变更并打开 SQL 预览，确认后由后端在同一事务中执行
function saveChanges(): boolean {
  if (!currentTab.value) return false
  if (!canEdit.value) {
    showToast(editBlockReason.value, 'error')
    return false
  }
  if (transactionActive.value) {
    showToast(t('dataTable.transactionBlockingSaveMessage'), 'error')
    return false
  }

  const changes = collectChanges()
  if (changes.length === 0) {
    showToast(t('dataTable.noChangesToSave'), 'error')
    return false
  }

  previewDialog.value = { show: true, changes, saving: false, error: '' }
  return true
}

function cancelPreview() {
  if (previewDialog.value.saving) return
  previewDialog.value = { show: false, changes: [], saving: false, error: '' }
  pendingCloseAfterSave.value = false
  pendingSwitchTab.value = null
}

async function confirmSave() {
  const conn = connectionStore.currentConnection
  const tab = currentTab.value
  if (!conn || !tab || previewDialog.value.saving) return

  previewDialog.value.saving = true
  previewDialog.value.error = ''
  try {
    const result = await browserStore.applyTableChanges(conn.id, tab.database, previewDialog.value.changes)

    modifiedCells.value.clear()
    newRows.value = []
    deletedRows.value = []
    tab.modified = false
    previewDialog.value = { show: false, changes: [], saving: false, error: '' }

    showToast(result.unchanged > 0
      ? t('dataTable.saveSuccessWithUnchanged', { applied: result.applied, unchanged: result.unchanged })
      : t('dataTable.saveSuccess', { count: result.applied }))

    const nextTab = pendingSwitchTab.value
    pendingSwitchTab.value = null

    if (nextTab) {
      // 「保存并切换」：保存成功后直接切到目标表，由 activateTab 加载数据，避免重复刷新当前表
      showCloseConfirm.value = false
      await activateTab(nextTab)
    } else {
      await loadData()

      if (pendingCloseAfterSave.value) {
        pendingCloseAfterSave.value = false
        closeAfterSave()
      }
    }
  } catch (e) {
    console.error('Save failed:', e)
    previewDialog.value.error = String(e)
  } finally {
    previewDialog.value.saving = false
  }
}

// 工具函数
function formatCellValue(value: unknown, colIndex: number): string {
  if (value === null || value === undefined) return 'NULL'
  const str = String(value)
  const maxChars = Math.floor((columnWidths.value[colIndex] || defaultColumnWidth) / 7)
  if (str.length > maxChars) {
    return str.substring(0, maxChars - 3) + '...'
  }
  return str
}

function getCellTitle(value: unknown): string {
  if (value === null || value === undefined) return 'NULL'
  return String(value)
}

function getCellClass(value: unknown): string {
  if (value === null || value === undefined) return 'type-null'
  if (typeof value === 'number') return 'type-number'
  if (typeof value === 'boolean') return 'type-boolean'
  return 'type-string'
}

let toastTimer: ReturnType<typeof setTimeout> | null = null

function showToast(message: string, type: 'success' | 'error' = 'success') {
  saveSuccess.value = { show: true, message, type }
  if (toastTimer) clearTimeout(toastTimer)
  toastTimer = setTimeout(() => {
    saveSuccess.value.show = false
    toastTimer = null
  }, 3000)
}

// 列宽调整
let resizingCol = -1
let startX = 0
let startWidth = 0

function startResize(event: MouseEvent, colIndex: number) {
  resizingCol = colIndex
  startX = event.clientX
  startWidth = columnWidths.value[colIndex] ?? 100

  document.addEventListener('mousemove', handleResize)
  document.addEventListener('mouseup', stopResize)
}

function handleResize(event: MouseEvent) {
  if (resizingCol === -1) return
  const delta = event.clientX - startX
  columnWidths.value[resizingCol] = Math.max(80, startWidth + delta)
}

function stopResize() {
  resizingCol = -1
  document.removeEventListener('mousemove', handleResize)
  document.removeEventListener('mouseup', stopResize)
}

// ========== 列宽自动适应 ==========
/** 测量字符串像素宽度，使用共享的隐藏 canvas context */
let measureCanvas: CanvasRenderingContext2D | null = null
function getMeasureContext(): CanvasRenderingContext2D | null {
  if (measureCanvas) return measureCanvas
  try {
    const canvas = document.createElement('canvas')
    measureCanvas = canvas.getContext('2d')
    return measureCanvas
  } catch {
    return null
  }
}

function measureTextWidth(text: string): number {
  const ctx = getMeasureContext()
  if (!ctx) return text.length * 7 // 兜底：每字符 ~7px

  // 尝试从现有单元格或表头取字体样式
  const tableEl = tableContainer.value
  let fontFamily = 'system-ui, sans-serif'
  let fontSize = 12
  if (tableEl) {
    const firstCell = tableEl.querySelector('td, th') as HTMLElement | null
    if (firstCell) {
      const cs = getComputedStyle(firstCell)
      fontFamily = cs.fontFamily || fontFamily
      fontSize = parseFloat(cs.fontSize) || fontSize
    }
  }
  ctx.font = `${fontSize}px ${fontFamily}`
  // 加 2px 字符间距补偿，避免 "刚好装不下"
  return ctx.measureText(text).width + 2
}

/** 自动适应单列：测量表头 + 当前页面所有可见数据行的最宽内容 */
function autoFitColumn(colIndex: number) {
  const columns = currentTableData.value?.columns
  const rows = filteredRows.value
  if (!columns) return
  const col = columns[colIndex]
  if (!col) return

  const padding = 24 // 左右 padding 合计
  const maxWidth = 500
  const minWidth = 60

  // 表头宽度（列名 + 数据类型）
  const headerText = `${col.name}  ${col.data_type}`
  let maxPx = measureTextWidth(headerText)

  // 数据行：测所有 visibleRows（虚拟滚动可见行 + 抽样）
  const rowLimit = Math.min(rows.length, visibleRows.value.length + 50) // 虚拟可见 + 抽样 50 行
  for (let i = 0; i < rowLimit; i++) {
    const cell = rows[i]?.[colIndex]
    const text = cell === null || cell === undefined ? 'NULL' : String(cell)
    maxPx = Math.max(maxPx, measureTextWidth(text))
  }

  const newWidth = Math.max(minWidth, Math.min(maxWidth, Math.ceil(maxPx + padding)))
  columnWidths.value[colIndex] = newWidth
}

/** 对所有列执行 auto-fit */
function autoFitAllColumns() {
  const columns = currentTableData.value?.columns
  if (!columns) return
  columns.forEach((_, idx) => autoFitColumn(idx))
}

/** 右键菜单里的自动适应（使用右键点击的那列） */
function autoFitColumnFromMenu() {
  const { colIndex } = contextMenu.value
  if (colIndex >= 0) autoFitColumn(colIndex)
  hideContextMenu()
}

// ========== 冻结列 ==========
const ROW_NUMBER_WIDTH = 40
/** 冻结数据列数（不包含行号列） */
const frozenCount = ref(0)

/** 每个冻结数据列的 left offset（累加前面所有列宽 + 行号列） */
const frozenLeftOffsets = computed<number[]>(() => {
  const cols = currentTableData.value?.columns ?? []
  const offsets: number[] = []
  let acc = ROW_NUMBER_WIDTH
  for (let i = 0; i < frozenCount.value && i < cols.length; i++) {
    offsets.push(acc)
    acc += columnWidths.value[i] ?? defaultColumnWidth
  }
  return offsets
})

function isColumnFrozen(colIndex: number): boolean {
  return colIndex < frozenCount.value
}

function getFrozenLeftOffset(colIndex: number): number {
  return frozenLeftOffsets.value[colIndex] ?? 0
}

function toggleFreezeBeforeCol(colIndex: number) {
  // 如果这列已冻结（是当前最后一列），取消；否则冻结到这列
  if (frozenCount.value === colIndex + 1) {
    frozenCount.value = 0
  } else {
    frozenCount.value = colIndex + 1
  }
}

/** 右键菜单里的冻结/取消冻结 */
function freezeColumnFromMenu() {
  const { colIndex } = contextMenu.value
  if (colIndex >= 0) toggleFreezeBeforeCol(colIndex)
  hideContextMenu()
}

// ========== 固定主键列 ==========
// 全局开关（localStorage 持久化）：开启时把主键列冻结在左侧，横向滚动始终可见。
// sticky 冻结要求前缀列，主键不在首列时连同其前面的列一起冻结。
const PK_PIN_STORAGE_KEY = 'auroradb.dataTable.pinPk'
const pkFrozen = ref(localStorage.getItem(PK_PIN_STORAGE_KEY) === '1')

function applyPkFrozen() {
  if (!pkFrozen.value) {
    frozenCount.value = 0
    return
  }
  const cols = currentTableData.value?.columns ?? []
  const lastPkIndex = cols.reduce((last, col, index) => (col.is_primary ? index : last), -1)
  frozenCount.value = lastPkIndex >= 0 ? lastPkIndex + 1 : 0
}

watch(pkFrozen, () => {
  localStorage.setItem(PK_PIN_STORAGE_KEY, pkFrozen.value ? '1' : '0')
  applyPkFrozen()
})

// 换表 / 列元数据按需加载完成后按新表的主键重新应用
watch(() => currentTableData.value?.columns, () => {
  if (pkFrozen.value) applyPkFrozen()
})

function handleScroll() {
  hideContextMenu()
  handleVirtualScroll()

  // 触底预取：滚动到接近底部时自动续页（距底部 120px 内触发）
  const el = tableContainer.value
  // 记录浏览位置，供关闭重开/切回时恢复
  if (el) {
    tableScrollMemory.set(scrollMemoryKey(browserStore.currentDatabase ?? '', browserStore.currentTable ?? '', connectionStore.currentConnection?.id ?? ''), el.scrollTop)
  }
  if (el && canLoadMore.value) {
    const nearBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 120
    if (nearBottom) loadMore()
  }
}

// 事务管理
async function beginTransaction() {
  const conn = connectionStore.currentConnection
  if (!conn) return

  try {
    await browserStore.beginTransaction(conn.id)
    transactionActive.value = true
    showToast(t('dataTable.transactionStarted'))
  } catch (e: unknown) {
    showToast(t('dataTable.beginTransactionFailed', { error: String(e) }), 'error')
  }
}

async function commitTransaction() {
  const conn = connectionStore.currentConnection
  if (!conn) return

  try {
    await browserStore.commitTransaction(conn.id)
    transactionActive.value = false
    showToast(t('dataTable.transactionCommitted'))
    await loadData()
  } catch (e: unknown) {
    showToast(t('dataTable.commitTransactionFailed', { error: String(e) }), 'error')
  }
}

async function rollbackTransaction() {
  const conn = connectionStore.currentConnection
  if (!conn) return

  try {
    await browserStore.rollbackTransaction(conn.id)
    transactionActive.value = false
    showToast(t('dataTable.transactionRolledBack'))
    await loadData()
  } catch (e: unknown) {
    showToast(t('dataTable.rollbackTransactionFailed', { error: String(e) }), 'error')
  }
}

// 导入导出逻辑已移入 ImportDialog / ExportDialog 组件

// 暴露方法
defineExpose({
  openTable,
  switchToTable: switchTab,
  closeTableById,
  closeConnectionTables,
  showImportDialog,
  showExportDialog,
  refreshData
})
</script>

<style scoped>
.data-table {
  flex: 1;
  display: flex;
  flex-direction: column;
  background: var(--canvas);
  overflow: hidden;
  position: relative;
}

/* 工具栏 */
.table-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  height: var(--toolbar-height);
  padding: 0 var(--space-2);
  background: var(--canvas);
  border-bottom: 1px solid var(--hairline);
  flex-shrink: 0;
  gap: var(--space-2);
  overflow: hidden;
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex: 1;
  min-width: 0;
  overflow-x: auto;
  overflow-y: hidden;
  scrollbar-width: none;
}

.toolbar-left::-webkit-scrollbar {
  display: none;
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-shrink: 0;
}

.toolbar-actions {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
}

.toolbar-actions button {
  height: var(--control-height-sm);
  padding: 0 5px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  white-space: nowrap;
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
  transition: var(--transition-colors);
}

.toolbar-actions button:hover:not(:disabled) {
  background: var(--hover-fill);
  color: var(--text-primary);
}

/* 工具栏内所有按钮：单行不换行、不压缩（溢出交给 .toolbar-left 横向滚动） */
.table-toolbar button,
.table-toolbar .toolbar-btn {
  white-space: nowrap;
  flex-shrink: 0;
}

/* 右侧筛选按钮（与左侧操作按钮同一 ghost 语言） */
.toolbar-right > button {
  height: var(--control-height-sm);
  padding: 0 6px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  display: flex;
  align-items: center;
  gap: 4px;
  transition: var(--transition-colors);
}

/* 排序指示 chip：横向滚动表头出视野后仍能看到当前排序 */
.sort-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  max-width: 220px;
  height: 24px;
  padding: 0 8px;
  border: 1px solid var(--brand-500);
  border-radius: 12px;
  background: transparent;
  color: var(--brand-500);
  font-size: var(--text-xs);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
}

.sort-chip:hover {
  background: var(--brand-500);
  color: var(--text-on-brand, #fff);
}

.chip-clear {
  flex-shrink: 0;
  opacity: 0.7;
}

.toolbar-right > button:hover {
  background: var(--hover-fill);
  color: var(--text-primary);
}

.toolbar-right > button.active {
  background: var(--selected-fill);
  color: var(--text-primary);
}

.filter-count {
  min-width: 14px;
  height: 14px;
  padding: 0 3px;
  border-radius: var(--radius-xs);
  background: var(--brand-500);
  color: #fff;
  font-size: 9px;
  line-height: 14px;
  text-align: center;
}

.toolbar-actions button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.toolbar-actions button.btn-cancel-query {
  color: var(--error-500);
}

.toolbar-actions button.btn-cancel-query:hover {
  background: rgba(239, 68, 68, 0.12);
}

.search-input {
  height: var(--control-height-sm);
  padding: 0 var(--space-2);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  background: var(--raised);
  color: var(--text-primary);
  width: 132px;
  transition: var(--transition-colors);
}

.search-input:focus {
  border-color: var(--brand-500);
  background: var(--canvas);
}

.search-input:focus {
  outline: none;
  background: var(--canvas);
  border-color: var(--brand-500);
}

/* 表格容器 */
.table-container {
  flex: 1;
  overflow: auto;
  position: relative;
}

table {
  border-collapse: separate;
  border-spacing: 0;
  font-size: var(--text-sm);
  table-layout: fixed;
  width: max-content;
  min-width: 100%;
}

/* 行号列 */
.row-number-header,
.row-number {
  width: 32px !important;
  min-width: 24px !important;
  max-width: 44px !important;
  text-align: center;
  border-right: 1px solid var(--hairline);
  padding: 0 var(--space-2) !important;
}

.row-number-header {
  background: var(--surface-default);
}

.row-number {
  background: var(--canvas);
  text-align: center;
  vertical-align: middle;
}

.row-num {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

th, td {
  padding: 0 var(--space-2);
  text-align: left;
  border-bottom: 1px solid var(--hairline);
  /* 列分隔线：只留 hairline 一级，保证多列时不至于把字段粘在一起 */
  border-right: 1px solid var(--hairline);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  position: relative;
}

/* 最后一列不再画右边线，避免和容器边缘叠成双线 */
th:last-child,
td:last-child {
  border-right: none;
}

td {
  height: var(--row-height);
  line-height: calc(var(--row-height) - 1px);
}

th {
  background: var(--surface-default);
  font-weight: var(--font-semibold);
  cursor: pointer;
  user-select: none;
  position: sticky;
  top: 0;
  z-index: 1;
  padding-top: 3px;
  padding-bottom: 3px;
}

/* ========== 冻结列样式 ========== */
.frozen-row-number,
.frozen-col {
  position: sticky !important;
  /* 底色交给各元素既有规则（表头 surface-default、行号 canvas、tbody 冻结列 canvas），
     不再 inherit：行本身无背景时 inherit 等于透明，横向滚动时下层列会穿透 */
}

/* thead 里的冻结列：同时 top + left 双向 sticky，层级要盖过普通表头(z-index:1)和普通冻结 td(z-index:2) */
thead .frozen-row-number,
thead .frozen-col {
  z-index: 3;
}

/* 普通冻结单元格：高于默认 td、低于表头冻结；不透明底色阻止横向滚动时下层列穿透 */
.frozen-row-number,
tbody .frozen-col {
  z-index: 2;
}

tbody .frozen-col {
  background-color: var(--canvas);
}

/* 冻结列右边缘阴影，水平滚动时视觉分隔更清晰 */
.frozen-row-number,
.frozen-col {
  box-shadow: 4px 0 6px -4px rgba(0, 0, 0, 0.35);
}

/* 行状态叠加：hover/选中/新增/删除都是半透明色，用 background-image 叠在不透明底色上，
   不能写成 background-color（会把底色顶掉，重新出现穿透） */
tbody tr:hover .frozen-col {
  background-image: linear-gradient(var(--hover-fill), var(--hover-fill));
}

tbody tr.selected .frozen-col {
  background-image: linear-gradient(var(--selected-fill), var(--selected-fill));
}

tbody tr.new-row .frozen-col {
  background-image: linear-gradient(rgba(16, 185, 129, 0.08), rgba(16, 185, 129, 0.08));
}

tbody tr.deleted-row .frozen-col {
  background-image: linear-gradient(rgba(239, 68, 68, 0.08), rgba(239, 68, 68, 0.08));
}

/* 冻结列里的已修改单元格：底色保持不透明，修改高亮用叠加 */
tbody td.modified.frozen-col {
  background-image: linear-gradient(rgba(245, 158, 11, 0.08), rgba(245, 158, 11, 0.08));
}

.th-content {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 1px;
  /* 右侧留出排序箭头 / 筛选按钮的位置，它们改为绝对定位，避免把表头堆成 4 行 */
  padding-right: 30px;
  overflow: hidden;
}

.col-name {
  font-size: var(--text-xs);
  line-height: 16px;
  font-weight: var(--font-medium);
  overflow: hidden;
  text-overflow: ellipsis;
}

.data-type {
  font-size: 10px;
  line-height: 13px;
  color: var(--text-tertiary);
  font-weight: var(--font-normal);
}

.sort-indicator {
  position: absolute;
  top: 2px;
  right: 15px;
  display: flex;
  align-items: center;
  color: var(--brand-500);
  pointer-events: none;
}

.resize-handle {
  position: absolute;
  right: 0;
  top: 0;
  bottom: 0;
  width: 4px;
  cursor: col-resize;
  background: transparent;
  transition: background var(--duration-fast);
}

.resize-handle:hover {
  background: var(--brand-500);
}

/* 数据行 */
tr:hover {
  background: var(--hover-fill);
}

tr.selected {
  background: var(--selected-fill) !important;
}

tr.new-row {
  background: rgba(16, 185, 129, 0.08);
}

tr.deleted-row {
  background: rgba(239, 68, 68, 0.08);
  opacity: 0.5;
}

/* 虚拟滚动占位行：只用于撑起滚动高度 */
.virtual-spacer:hover {
  background: transparent;
}

.virtual-spacer td {
  cursor: default;
}

td {
  cursor: pointer;
  position: relative;
}

td.modified {
  background: rgba(245, 158, 11, 0.08);
}

td.is-null {
  color: var(--text-tertiary);
  font-style: italic;
}

.cell-text {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
}

.type-null {
  color: var(--text-tertiary);
  font-style: italic;
}

.type-number {
  color: var(--brand-500);
}

.type-boolean {
  color: var(--brand-500);
}

.type-string {
  color: var(--text-primary);
}

/* 编辑输入框 */
.edit-input {
  width: 100%;
  height: 100%;
  border: 1px solid var(--brand-500);
  padding: var(--space-2);
  font-size: var(--text-sm);
  outline: none;
  background: var(--canvas);
  box-sizing: border-box;
  color: var(--text-primary);
  box-shadow: 0 0 0 3px rgba(76, 154, 255, 0.2), 0 4px 12px rgba(0, 0, 0, 0.15);
  transition: all 0.15s ease;
}

.edit-input:focus {
  border-color: var(--brand-600);
  box-shadow: 0 0 0 4px rgba(76, 154, 255, 0.25), 0 6px 16px rgba(0, 0, 0, 0.2);
}

/* 编辑器包装器 */
.datetime-editor-wrapper {
  position: relative;
}

/* 遮罩层：防止点击外部关闭编辑器（仅在弹层打开时显示） */
.datetime-picker-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 9999;
  background: rgba(0, 0, 0, 0.1);
}

/* 自定义日期时间编辑器容器 */
.datetime-editor-container {
  position: relative;
  z-index: 1000;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 2px;
  background: var(--canvas);
  border: 1px solid var(--brand-500);
  box-shadow: 0 0 0 3px rgba(76, 154, 255, 0.2), 0 4px 12px rgba(0, 0, 0, 0.15);
}

.datetime-editor-container .edit-input {
  flex: 1;
  min-width: 0;
  padding: 4px 6px;
  font-size: var(--text-sm);
  border: none;
  box-shadow: none;
  background: transparent;
}

.datetime-editor-container .edit-input:focus {
  box-shadow: none;
  border: none;
  outline: none;
}

/* 打开选择器按钮 */
.btn-open-picker {
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-secondary);
  transition: all 0.15s ease;
  padding: 0;
}

.btn-open-picker:hover {
  color: var(--brand-500);
  background: var(--brand-100);
}

/* 确认/取消按钮 */
.datetime-actions {
  display: flex;
  gap: 2px;
  padding: 0 2px;
}

.datetime-actions button {
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.15s ease;
  padding: 0;
}

.datetime-actions .btn-confirm {
  color: var(--success-500);
}

.datetime-actions .btn-confirm:hover {
  background: var(--success-100);
}

.datetime-actions .btn-cancel {
  color: var(--error-500);
}

.datetime-actions .btn-cancel:hover {
  background: rgba(239, 68, 68, 0.12);
}

/* 自定义日期时间选择器弹层 */
.datetime-picker-popup {
  position: fixed;
  background: var(--canvas);
  border: 1px solid var(--hairline);
  border-radius: 8px;
  box-shadow: 0 8px 30px rgba(0, 0, 0, 0.2);
  z-index: 10000;
  min-width: 240px;
  padding: 8px;
}

/* 日历选择器头部 */
.picker-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 6px;
  padding: 0 2px;
}

.picker-header span {
  font-weight: 600;
  font-size: 13px;
  color: var(--text-primary);
}

.picker-header button {
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-secondary);
  border-radius: 4px;
  transition: all 0.15s ease;
  font-size: 11px;
}

.picker-header button:hover {
  background: var(--brand-100);
  color: var(--brand-500);
}

/* 日历格子 */
.picker-days {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 1px;
}

.day-header {
  text-align: center;
  padding: 3px 1px;
  font-size: 11px;
  color: var(--text-tertiary);
  font-weight: 500;
}

.day-cell {
  text-align: center;
  padding: 5px 1px;
  font-size: 12px;
  cursor: pointer;
  border-radius: 3px;
  transition: all 0.15s ease;
}

.day-cell:hover {
  background: var(--brand-100);
}

.day-cell.other-month {
  color: var(--text-tertiary);
}

.day-cell.selected {
  background: var(--brand-500);
  color: white;
}

.day-cell.today {
  font-weight: 600;
  border: 1px solid var(--brand-500);
}

.day-cell.today.selected {
  border-color: white;
}

/* 时间选择器 */
.time-section {
  margin-top: 8px;
  padding-top: 8px;
  border-top: 1px solid var(--hairline);
}

.time-inputs {
  display: flex;
  align-items: flex-end;
  justify-content: center;
  gap: 2px;
}

.time-field {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}

.time-field label {
  font-size: 11px;
  color: var(--text-tertiary);
}

.time-field input {
  width: 40px;
  height: 26px;
  text-align: center;
  border: 1px solid var(--hairline);
  border-radius: 4px;
  font-size: 13px;
  padding: 0;
}

.time-field input:focus {
  border-color: var(--brand-500);
  outline: none;
}

.time-separator {
  font-size: var(--text-lg);
  color: var(--text-secondary);
  padding-bottom: 4px;
}

/* 选择器底部按钮 */
.picker-actions {
  display: flex;
  gap: 8px;
  margin-top: 8px;
  padding-top: 8px;
  border-top: 1px solid var(--hairline);
  justify-content: flex-end;
}

.picker-actions button {
  padding: 5px 14px;
  border: 1px solid var(--hairline);
  border-radius: 4px;
  background: var(--canvas);
  cursor: pointer;
  font-size: 12px;
  transition: all 0.15s ease;
}

.picker-actions .btn-cancel {
  color: var(--text-secondary);
  border-color: var(--hairline);
}

.picker-actions .btn-cancel:hover {
  background: var(--gray-100);
}

.picker-actions .btn-confirm {
  background: var(--brand-500);
  color: white;
  border-color: var(--brand-500);
}

.picker-actions .btn-confirm:hover {
  background: var(--brand-600);
}

td.editing {
  padding: 2px;
  background: var(--hover-fill);
}

td.editing::before {
  content: '';
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  border: 2px solid var(--brand-500);
  pointer-events: none;
  z-index: 1;
}

/* 右键菜单 */
.context-menu {
  position: fixed;
  background: var(--overlay);
  border: 1px solid var(--hairline-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  z-index: 1000;
  min-width: 120px;
  padding: var(--space-1);
}

.menu-item {
  padding: 0 var(--space-2);
  height: var(--control-height-sm);
  display: flex;
  align-items: center;
  cursor: pointer;
  font-size: var(--text-xs);
  color: var(--text-primary);
  border-radius: var(--radius-sm);
}

.menu-item:hover {
  background: var(--hover-fill);
}

.menu-divider {
  height: 1px;
  background: var(--hairline);
  margin: var(--space-1) 0;
}

/* 空状态 */
.empty-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}

.empty-state.error-state {
  color: var(--error-500);
}

/* 空表提示 */
.empty-row {
  background: transparent;
}

.empty-cell {
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  padding: var(--space-5) 0 !important;
}

.loading-overlay {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  /* 使用更柔和的遮罩层，保留内容可见性 */
  background: rgba(var(--bg-primary-rgb, 30, 30, 30), 0.35);
  backdrop-filter: blur(1px);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  color: var(--text-secondary);
  z-index: 10;
  /* 淡入动画 */
  animation: fade-in 0.3s ease-out;
}

/* 查看完整内容弹窗 */
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  /* 弹窗遮罩层可以稍深一些，但也不要太深 */
  background: rgba(var(--bg-primary-rgb, 30, 30, 30), 0.45);
  backdrop-filter: blur(2px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  /* 淡入动画 */
  animation: fade-in 0.3s ease-out;
}

/* 淡入淡出动画 */
@keyframes fade-in {
  from {
    opacity: 0;
    backdrop-filter: blur(0px);
  }
  to {
    opacity: 1;
    backdrop-filter: blur(1px);
  }
}

@keyframes fade-out {
  from {
    opacity: 1;
  }
  to {
    opacity: 0;
  }
}

.dialog-actions {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
}

/* 确认对话框 */
.confirm-dialog {
  background: var(--canvas);
  padding: 24px;
  border-radius: 8px;
  box-shadow: var(--shadow-lg);
  min-width: 300px;
}

.confirm-dialog h3 {
  margin: 0 0 12px 0;
  font-size: 16px;
  color: var(--text-primary);
}

.confirm-dialog p {
  margin: 0 0 20px 0;
  color: var(--text-secondary);
}

.confirm-actions {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
}

.btn-primary {
  background: var(--success-500);
  color: white;
  border: none;
  padding: 8px 16px;
  border-radius: 4px;
  cursor: pointer;
}

.btn-secondary {
  background: var(--panel);
  border: 1px solid var(--hairline);
  color: var(--text-primary);
  padding: 8px 16px;
  border-radius: 4px;
  cursor: pointer;
}

/* 工具栏样式 */
.toolbar-divider {
  width: 1px;
  height: 14px;
  background: var(--hairline);
  margin: 0 2px;
}

/* 事务按钮分段控件：三个裸 ghost 按钮在 2px 间距下会读成一串，这里收成一个整体 */
.txn-group {
  display: flex;
  align-items: center;
  gap: 2px;
  height: var(--control-height-sm);
  padding: 0 2px;
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  flex-shrink: 0;
}

.txn-group button {
  height: calc(var(--control-height-sm) - 4px);
  padding: 0 6px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  border-radius: var(--radius-xs);
  cursor: pointer;
  font-size: 10px;
  font-weight: var(--font-medium);
  letter-spacing: 0.2px;
  white-space: nowrap;
  flex-shrink: 0;
  transition: var(--transition-colors);
}

.txn-group button:hover:not(:disabled) {
  background: var(--hover-fill);
  color: var(--text-primary);
}

.txn-group button.active {
  background: var(--selected-fill);
  color: var(--text-primary);
}

.txn-group button:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.toolbar-actions button.active {
  background: var(--selected-fill);
  color: var(--text-primary);
}

/* 列选择器（宽表按需加载） */
.column-selector {
  position: relative;
}

/* 列选择菜单：Teleport 到 body，定位（fixed + top/left/bottom）由内联样式按按钮位置计算 */
.column-selector-menu {
  z-index: 1000;
  width: 280px;
  max-height: 360px;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.18);
  cursor: default;
}

.column-selector-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border-default);
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.column-selector-actions {
  display: flex;
  gap: 6px;
}

.column-selector-actions button {
  padding: 2px 6px;
  border: 1px solid var(--border-default);
  background: transparent;
  color: var(--text-secondary);
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  cursor: pointer;
}

.column-selector-actions button:hover {
  color: var(--text-primary);
  border-color: var(--text-tertiary);
}

.column-selector-list {
  overflow-y: auto;
  padding: 4px 0;
}

.column-option {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  font-size: var(--text-xs);
  cursor: pointer;
}

.column-option:hover {
  background: var(--bg-secondary);
}

.column-option-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-primary);
}

.column-option-type {
  color: var(--text-tertiary);
}

.column-selector-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 10px;
  border-top: 1px solid var(--border-default);
}

.pending-hint {
  color: var(--warning-500);
  font-size: var(--text-xs);
}

.transaction-indicator {
  color: var(--success-500);
  font-size: 16px;
  animation: pulse 1.5s infinite;
}

/* 无主键只读徽标 */
.readonly-badge {
  display: inline-flex;
  align-items: center;
  padding: 0 6px;
  border-radius: var(--radius-xs);
  font-size: 10px;
  line-height: 16px;
  color: var(--text-tertiary);
  background: var(--hover-fill);
  white-space: nowrap;
}

/* 保存预览弹窗 */
.save-preview-dialog {
  max-width: 760px;
}

.preview-summary {
  margin: 0 0 6px 0;
  font-size: 13px;
  color: var(--text-primary);
}

.preview-hint {
  margin: 0 0 12px 0;
  font-size: 12px;
  color: var(--text-secondary);
}

.save-preview-dialog .result-textarea {
  width: 100%;
  min-height: 220px;
  resize: vertical;
  padding: 10px;
  font-family: monospace;
  font-size: 12px;
  line-height: 1.5;
  border: 1px solid var(--hairline);
  border-radius: 4px;
  background: var(--panel);
  color: var(--text-primary);
  box-sizing: border-box;
}

.preview-error {
  margin: 12px 0 0 0;
  font-size: 12px;
  color: var(--error-500);
  white-space: pre-wrap;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}

/* 导入导出弹窗 */
.dialog {
  background: var(--canvas);
  padding: 24px;
  border-radius: 8px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.2);
  min-width: 400px;
  max-width: 600px;
  width: 90vw;
}

.dialog h3 {
  margin: 0 0 20px 0;
  font-size: 16px;
  border-bottom: 1px solid var(--hairline);
  padding-bottom: 12px;
  color: var(--text-primary);
}

.dialog-content {
  margin-bottom: 20px;
}

.form-group {
  margin-bottom: 16px;
}

.form-group label {
  display: block;
  margin-bottom: 6px;
  font-size: 13px;
  color: var(--text-secondary);
}

.form-group textarea {
  width: 100%;
  padding: 8px 12px;
  border: 1px solid var(--hairline);
  border-radius: 4px;
  font-size: 13px;
  font-family: inherit;
  background: var(--canvas);
  color: var(--text-primary);
}

/* 自定义下拉框：触发器与对话框内输入框保持相同的外观 */
.form-group :deep(.select-trigger) {
  width: 100%;
  height: 38px;
  min-height: 38px;
  padding: 0 12px;
  border-color: var(--hairline);
  border-radius: 4px;
  background: var(--canvas);
  color: var(--text-primary);
  font-size: 13px;
}

.form-group textarea {
  resize: vertical;
  min-height: 120px;
  font-family: monospace;
}

.form-group textarea.result-textarea {
  background: var(--panel);
}

.import-dialog,
.export-dialog {
  max-height: 80vh;
  overflow-y: auto;
}

/* 文件选择区域 */
.file-select-area {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.file-select-area .btn-secondary {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 10px 16px;
}

.btn-icon {
  font-size: 16px;
}

.file-info {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--panel);
  border-radius: 4px;
  border: 1px solid var(--hairline);
}

.file-name {
  font-weight: 500;
  color: var(--text-primary);
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-size {
  color: var(--text-secondary);
  font-size: 12px;
}

.file-hint {
  color: var(--text-secondary);
  font-size: 13px;
  padding: 8px 0;
}

.file-preview {
  background: var(--panel);
  border: 1px solid var(--hairline);
  border-radius: 4px;
  padding: 12px;
  max-height: 150px;
  overflow-y: auto;
}

.file-preview pre {
  margin: 0;
  font-family: 'JetBrains Mono', 'Fira Code', monospace;
  font-size: 12px;
  line-height: 1.4;
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-all;
}

/* Toast 提示 */
.toast-message {
  position: fixed;
  top: 50px;
  right: 20px;
  padding: 12px 24px;
  border-radius: 4px;
  color: white;
  font-size: 14px;
  z-index: 1001;
  animation: slideIn 0.3s ease;
}

.toast-message.success {
  background: var(--success-500);
}

.toast-message.error {
  background: var(--error-500);
}

@keyframes slideIn {
  from {
    transform: translateX(100%);
    opacity: 0;
  }
  to {
    transform: translateX(0);
    opacity: 1;
  }
}

/* 过滤栏 */
.filter-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 14px;
  height: 14px;
  padding: 0 3px;
  border-radius: var(--radius-xs);
  background: var(--brand-500);
  color: #fff;
  font-size: 9px;
  line-height: 1;
}

.filter-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--raised);
  border-bottom: 1px solid var(--hairline);
  flex-shrink: 0;
}

.filter-panel-header {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.filter-panel-title {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.filter-logic {
  display: flex;
  gap: 2px;
  padding: 2px;
  border: none;
  border-radius: var(--radius-sm);
  background: var(--canvas);
  overflow: hidden;
}

.filter-logic button {
  padding: 0 var(--space-2);
  height: 20px;
  border: none;
  border-radius: var(--radius-xs);
  background: transparent;
  color: var(--text-tertiary);
  font-size: 10px;
  cursor: pointer;
}

.filter-logic button.active {
  background: var(--selected-fill);
  color: var(--text-primary);
}

.filter-close {
  margin-left: auto;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}

.filter-rows {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.filter-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.filter-join {
  min-width: 44px;
  font-size: 10px;
  color: var(--text-tertiary);
  text-transform: uppercase;
}

.filter-value {
  height: var(--control-height-sm);
  padding: 0 var(--space-2);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: var(--raised);
  color: var(--text-primary);
  font-size: var(--text-xs);
}

.filter-value:focus {
  outline: none;
  border-color: var(--brand-500);
  background: var(--canvas);
}

/* 下拉用 CustomSelect（DOM 弹层）：原生 <select> 的展开列表由浏览器/系统绘制，
   暗色下会渲染成白底，无法用 CSS 覆盖 —— 这也是换成组件的原因 */
.filter-select {
  /* CustomSelect 自身是 width:100%，这里要收回成内容宽度 */
  width: auto;
  flex: 0 0 auto;
  min-width: 130px;
}

.filter-op {
  min-width: 90px;
}

.filter-select :deep(.select-trigger),
.filter-select :deep(.select-trigger.size-sm) {
  height: var(--control-height-sm);
  min-height: var(--control-height-sm);
  padding: 0 var(--space-2);
  border-color: transparent;
  border-radius: var(--radius-sm);
  background: var(--raised);
  font-size: var(--text-xs);
}

.filter-select :deep(.select-trigger:hover:not(.disabled)) {
  background: var(--hover-fill);
  border-color: transparent;
}

.filter-select :deep(.select-trigger.open) {
  background: var(--canvas);
  border-color: var(--brand-500);
  box-shadow: none;
}

.filter-value {
  flex: 1;
  min-width: 120px;
}

.filter-remove {
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}

.filter-remove:hover {
  color: var(--error-500);
}

.filter-empty {
  font-size: 12px;
  color: var(--text-secondary);
}

.filter-actions,
.filter-saved {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.filter-actions button,
.filter-saved button,
.filter-preview button {
  padding: 3px 10px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-primary);
  font-size: 12px;
  cursor: pointer;
}

.filter-actions button:disabled,
.filter-saved button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.filter-preview {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 8px;
  border: 1px dashed var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-hover);
}

.filter-preview code {
  flex: 1;
  font-size: 12px;
  color: var(--text-primary);
  word-break: break-all;
}

.th-filter-btn {
  position: absolute;
  right: 0;
  top: 50%;
  transform: translateY(-50%);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0 2px;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  opacity: 0;
  cursor: pointer;
  transition: opacity var(--duration-fast);
}

th:hover .th-filter-btn {
  opacity: 0.55;
}

.th-filter-btn:hover,
.th-filter-btn.active {
  opacity: 1;
  color: var(--text-primary);
}
</style>
