<template>
  <div class="data-table" @click="hideContextMenu">
    <!-- 工具栏 -->
    <div class="table-toolbar" v-if="currentTab">
      <div class="toolbar-left">
        <span class="table-info">{{ currentTab?.name }}</span>
        <div class="toolbar-actions">
          <button v-if="!loading" @click="refreshData" :title="t('dataTable.refreshHint')">
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
          <button @click="autoFitAllColumns" :disabled="loading || !currentTableData" :title="t('dataTable.autoFitAll')">
            <AlignHorizontalDistributeCenter :size="14" />
          </button>
          <!-- 按需列加载：宽表默认只取前 N 列，勾选后重新查询 -->
          <div v-if="allColumnsForTab.length > 0" class="column-selector">
            <button
              @click.stop="showColumnSelector = !showColumnSelector"
              :class="{ active: showColumnSelector }"
              :title="t('dataTable.columnsHint')"
            >
              <Columns3 :size="14" />
              {{ t('dataTable.columnsLabel', { selected: selectedColumnNames.length, total: allColumnsForTab.length }) }}
            </button>
            <div v-if="showColumnSelector" class="column-selector-menu" @click.stop>
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
          </div>
          <button
            @click="showGenerator = true"
            :disabled="loading || !currentTab"
            :title="t('dataGenerator.button')"
          >
            <Wand2 :size="14" />
            {{ t('dataGenerator.button') }}
          </button>
          <span class="toolbar-divider"></span>
          <!-- 事务控制按钮 -->
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
          <span v-if="isReadOnlyConnection" class="readonly-badge" :title="readOnlyHint">
            {{ readOnlyBadgeLabel }}
          </span>
          <span v-else-if="currentTableData && !hasPrimaryKey" class="readonly-badge" :title="t('dataTable.noPrimaryKeyReadonlyHint')">
            {{ t('dataTable.readOnlyNoPrimaryKey') }}
          </span>
        </div>
      </div>
      <div class="toolbar-right">
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
        <span v-if="currentTableData" class="row-info">
          {{ t('dataTable.rowRange', {
            start: cursorMode ? 1 : (page - 1) * pageSize + 1,
            end: cursorMode ? currentTableData.rows.length : Math.min(page * pageSize, currentTableData.total_count),
            total: currentTableData.total_count
          }) }}
        </span>
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
        <button class="filter-close" @click="showFilterPanel = false" :title="t('common.close')">
          <X :size="13" />
        </button>
      </div>

      <div class="filter-rows">
        <div v-for="(row, index) in filterRows" :key="row.id" class="filter-row">
          <span v-if="index > 0" class="filter-join">{{ filterLogic }}</span>
          <span v-else class="filter-join">{{ t('dataTable.filterTitle') }}</span>
          <select v-model="row.column" class="filter-select">
            <option value="" disabled>{{ t('dataTable.filterColumn') }}</option>
            <option v-for="name in filterColumnOptions" :key="name" :value="name">{{ name }}</option>
          </select>
          <select v-model="row.operator" class="filter-select filter-op">
            <option v-for="op in filterOperatorOptions" :key="op.value" :value="op.value">{{ op.label }}</option>
          </select>
          <input
            v-if="!isNullOperator(row.operator)"
            v-model="row.value"
            class="filter-value"
            :placeholder="t('dataTable.filterValue')"
          />
          <button class="filter-remove" @click="removeFilterRow(row.id)" :title="t('common.delete')">
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
        <select v-model="selectedSavedFilter" class="filter-select" @change="applySavedFilter(selectedSavedFilter)">
          <option value="">{{ t('dataTable.filterSavedPlaceholder') }}</option>
          <option v-for="item in savedFilters" :key="item.name" :value="item.name">{{ item.name }}</option>
        </select>
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
    <div class="table-container" v-if="currentTableData" ref="tableContainer" @scroll="handleScroll">
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
              {{ t('dataTable.tableEmpty') }}
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
                      <button class="btn-open-picker" @click="openDateTimePicker($event)" :title="t('dataTable.pickDateTime')">
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

    <!-- 分页 / 游标续页 -->
    <div v-if="currentTableData" class="table-footer">
      <!-- 大表使用游标翻页：只追加「加载更多」，不再展示页码 -->
      <div v-if="cursorMode" class="pagination load-more-bar">
        <span class="loaded-rows">
          {{ t('dataTable.loadedRows', { loaded: currentTableData.rows.length, total: currentTableData.total_count }) }}
        </span>
        <button
          v-if="currentTableData.next_cursor != null"
          @click="loadMore"
          :disabled="!canLoadMore"
        >{{ loadingMore ? t('dataTable.loadingMore') : t('dataTable.loadMore') }}</button>
        <span v-else class="all-loaded">{{ t('dataTable.allLoaded') }}</span>
      </div>
      <div v-else class="pagination">
        <button @click="firstPage" :disabled="page <= 1">{{ t('dataTable.firstPage') }}</button>
        <button @click="prevPage" :disabled="page <= 1">{{ t('dataTable.prevPage') }}</button>
        <span>{{ t('dataTable.pageIndicator', { page, total: totalPages }) }}</span>
        <button @click="nextPage" :disabled="page >= totalPages">{{ t('dataTable.nextPage') }}</button>
        <button @click="lastPage" :disabled="page >= totalPages">{{ t('dataTable.lastPage') }}</button>
      </div>
      <div class="page-size">
        <Select
          :model-value="String(pageSize)"
          :options="pageSizeOptions"
          size="sm"
          @update:model-value="onPageSizeChange"
        />
      </div>
      <div class="execution-time" v-if="currentTableData">
        {{ t('dataTable.executionTime', { ms: currentTableData.execution_time_ms }) }}
      </div>
    </div>

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
      :driver="isPostgresConnection ? 'postgres' : 'mysql'"
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
    <div v-if="importDialog.show" class="modal-overlay" @click.self="importDialog.show = false">
      <div class="dialog import-dialog">
        <h3>{{ t('dataTable.importTitle') }}</h3>
        <div class="dialog-content">
          <div class="form-group">
            <label>{{ t('dataTable.format') }}</label>
            <Select
              :model-value="importDialog.format"
              :options="importFormatOptions"
              @update:model-value="onImportFormatChange"
            />
          </div>
          <div class="form-group">
            <label>{{ t('dataTable.file') }}</label>
            <div class="file-select-area">
              <button @click="selectImportFile" :disabled="importDialog.loading" class="btn-secondary">
                <Folder class="btn-icon" :size="16" /> {{ t('dataTable.selectFile') }}
              </button>
              <div v-if="importDialog.filePath" class="file-info">
                <span class="file-name">{{ importDialog.fileName }}</span>
                <span class="file-size">({{ formatFileSize(importDialog.fileSize) }})</span>
              </div>
              <div v-else class="file-hint">
                {{ t('dataTable.fileHint') }}
              </div>
            </div>
          </div>
          <div class="form-group" v-if="importDialog.data">
            <label>{{ t('dataTable.previewFirst500') }}</label>
            <div class="file-preview">
              <pre>{{ importDialog.data.substring(0, 500) }}{{ importDialog.data.length > 500 ? '...' : '' }}</pre>
            </div>
          </div>
        </div>
        <div class="dialog-actions">
          <button @click="importDialog.show = false" class="btn-secondary">{{ t('common.cancel') }}</button>
          <button @click="executeImport" :disabled="importDialog.loading || !importDialog.data" class="btn-primary">
            {{ importDialog.loading ? t('dataTable.importing') : t('common.import') }}
          </button>
        </div>
      </div>
    </div>

    <!-- 导出数据弹窗 -->
    <div v-if="exportDialog.show" class="modal-overlay" @click.self="exportDialog.show = false">
      <div class="dialog export-dialog">
        <h3>{{ t('dataTable.exportTitle') }}</h3>
        <div class="dialog-content">
          <div class="form-group">
            <label>{{ t('dataTable.format') }}</label>
            <Select
              :model-value="exportDialog.format"
              :options="exportFormatOptions"
              @update:model-value="onExportFormatChange"
            />
          </div>
        </div>
        <div class="dialog-actions">
          <button @click="exportDialog.show = false" class="btn-secondary">{{ t('common.cancel') }}</button>
          <button @click="executeExport" :disabled="exportDialog.loading" class="btn-primary">
            {{ exportDialog.loading ? t('dataTable.exporting') : t('common.export') }}
          </button>
        </div>
      </div>
    </div>

    <!-- 数据生成器（B8） -->
    <DataGeneratorDialog
      v-if="currentTab"
      :open="showGenerator"
      :connection-id="currentTab.connectionId"
      :database="currentTab.database"
      :table="currentTab.tableName"
      :driver="isPostgresConnection ? 'postgres' : 'mysql'"
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
  type DataFilterCondition,
  type TableChange,
} from '@/stores/dataBrowser'
import { useConnectionStore } from '@/stores/connection'
import { useDebounce } from '@/composables/useDebounce'
import { confirmAction } from '@/composables/useConfirm'
import { useUnsavedGuard } from '@/composables/useUnsavedGuard'
import { useVirtualRows } from '@/composables/useVirtualRows'
import { collectTableChanges, formatSqlValue, isBinaryType, isSameValue } from '@/utils/tableChanges'
import { quoteIdentifier } from '@/utils/sql'
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
  Folder,
  X,
  AlignHorizontalDistributeCenter
} from '@/components/icons'
import { Calendar, Columns3, Filter, Wand2 } from 'lucide-vue-next'
import Select from '@/components/ui/Select.vue'
import DataGeneratorDialog from '@/components/DataGeneratorDialog.vue'

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

// 自定义日期时间选择器状态
const showDateTimePicker = ref(false)
const pickerPosition = ref({ top: '0px', left: '0px' })
const currentYear = ref(new Date().getFullYear())
const currentMonth = ref(new Date().getMonth() + 1)
const selectedDate = ref('')
const selectedHour = ref(0)
const selectedMinute = ref(0)
const selectedSecond = ref(0)
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

// ========== 过滤栏（列头筛选 + 多条件构建器 + 已存过滤器） ==========
type FilterOperator = 'Eq' | 'Ne' | 'Gt' | 'Gte' | 'Lt' | 'Lte' | 'Like' | 'IsNull' | 'IsNotNull'

interface FilterRow {
  id: number
  column: string
  operator: FilterOperator
  value: string
}

interface SavedFilter {
  name: string
  logic: 'AND' | 'OR'
  rows: { column: string; operator: FilterOperator; value: string }[]
}

const showFilterPanel = ref(false)
const filterRows = ref<FilterRow[]>([])
const filterLogic = ref<'AND' | 'OR'>('AND')
/** 已应用到请求的筛选（面板里可以继续编辑而不立即生效） */
const activeFilters = ref<FilterRow[]>([])
const activeFilterLogic = ref<'AND' | 'OR'>('AND')
const savedFilters = ref<SavedFilter[]>([])
const selectedSavedFilter = ref('')
const newFilterName = ref('')
let filterRowSeq = 0

const filterOperatorOptions: { label: string; value: FilterOperator }[] = [
  { label: '=', value: 'Eq' },
  { label: '!=', value: 'Ne' },
  { label: '>', value: 'Gt' },
  { label: '>=', value: 'Gte' },
  { label: '<', value: 'Lt' },
  { label: '<=', value: 'Lte' },
  { label: 'LIKE', value: 'Like' },
  { label: 'IS NULL', value: 'IsNull' },
  { label: 'IS NOT NULL', value: 'IsNotNull' },
]

const filterColumnOptions = computed(() => allColumnsForTab.value.map(col => col.name))

const savedFilterKey = computed(() =>
  currentTab.value
    ? `auroradb:savedFilters:${currentTab.value.connectionId}:${currentTab.value.database}:${currentTab.value.tableName}`
    : ''
)

/** 条件是否有效：IS NULL 类不需要值，其它必须填值 */
function isUsableFilter(row: FilterRow): boolean {
  if (!row.column) return false
  if (row.operator === 'IsNull' || row.operator === 'IsNotNull') return true
  return row.value !== ''
}

function isNullOperator(operator: FilterOperator): boolean {
  return operator === 'IsNull' || operator === 'IsNotNull'
}

/** 数值列把输入转成数字，避免 PG 文本与整型比较报错 */
function coerceFilterValue(row: FilterRow): unknown {
  if (isNullOperator(row.operator)) return null
  const type = allColumnsForTab.value.find(col => col.name === row.column)?.data_type?.toLowerCase() ?? ''
  const numeric = /int|decimal|numeric|float|double|real|year|bit/.test(type)
  if (numeric && row.value.trim() !== '' && !Number.isNaN(Number(row.value))) {
    return Number(row.value)
  }
  return row.value
}

function toFilterConditions(rows: FilterRow[]): DataFilterCondition[] {
  return rows.filter(isUsableFilter).map(row => ({
    column: row.column,
    operator: row.operator,
    value: coerceFilterValue(row),
  }))
}

function addFilterRow(column = '') {
  showFilterPanel.value = true
  filterRows.value = [
    ...filterRows.value,
    {
      id: ++filterRowSeq,
      column: column || filterColumnOptions.value[0] || '',
      operator: 'Eq',
      value: '',
    },
  ]
}

function removeFilterRow(id: number) {
  filterRows.value = filterRows.value.filter(row => row.id !== id)
}

/** 生成的 WHERE 预览（可直接复制到 SQL 编辑器） */
const filterWherePreview = computed(() => {
  const rows = filterRows.value.filter(isUsableFilter)
  if (rows.length === 0) return ''
  const quote = (name: string) => (isPostgresConnection.value ? `"${name}"` : `\`${name}\``)
  const literal = (row: FilterRow): string => {
    const coerced = coerceFilterValue(row)
    if (coerced === null) return ''
    if (typeof coerced === 'number') return String(coerced)
    return `'${String(coerced).replace(/'/g, "''")}'`
  }
  const parts = rows.map(row => {
    const column = quote(row.column)
    switch (row.operator) {
      case 'Eq': return `${column} = ${literal(row)}`
      case 'Ne': return `${column} != ${literal(row)}`
      case 'Gt': return `${column} > ${literal(row)}`
      case 'Gte': return `${column} >= ${literal(row)}`
      case 'Lt': return `${column} < ${literal(row)}`
      case 'Lte': return `${column} <= ${literal(row)}`
      case 'Like': return `${column} LIKE ${literal(row)}`
      case 'IsNull': return `${column} IS NULL`
      case 'IsNotNull': return `${column} IS NOT NULL`
    }
  })
  return `WHERE ${parts.join(` ${filterLogic.value} `)}`
})

async function copyWherePreview() {
  try {
    await navigator.clipboard.writeText(filterWherePreview.value)
    showToast(t('dataTable.filterCopied'))
  } catch {
    showToast(t('dataTable.filterCopied'), 'error')
  }
}

/** 应用筛选：会丢弃未保存的修改，因此先确认 */
async function applyFilters() {
  if (hasUnsavedChanges.value) {
    const confirmed = await confirmAction({
      message: t('dataTable.filterDiscardConfirm'),
      confirmText: t('common.confirm'),
    })
    if (!confirmed) return
  }
  activeFilters.value = filterRows.value.filter(isUsableFilter).map(row => ({ ...row }))
  activeFilterLogic.value = filterLogic.value
  browserStore.setPage(1)
  await loadData()
}

async function clearFilters() {
  filterRows.value = []
  activeFilters.value = []
  activeFilterLogic.value = 'AND'
  filterLogic.value = 'AND'
  selectedSavedFilter.value = ''
  browserStore.setPage(1)
  await loadData()
}

function loadSavedFilters() {
  if (!savedFilterKey.value) {
    savedFilters.value = []
    return
  }
  try {
    const raw = localStorage.getItem(savedFilterKey.value)
    savedFilters.value = raw ? (JSON.parse(raw) as SavedFilter[]) : []
  } catch {
    savedFilters.value = []
  }
}

function persistSavedFilters() {
  if (!savedFilterKey.value) return
  localStorage.setItem(savedFilterKey.value, JSON.stringify(savedFilters.value))
}

function saveCurrentFilter() {
  const name = newFilterName.value.trim()
  if (!name) return
  const rows = filterRows.value.filter(isUsableFilter).map(row => ({
    column: row.column,
    operator: row.operator,
    value: row.value,
  }))
  const next: SavedFilter = { name, logic: filterLogic.value, rows }
  const index = savedFilters.value.findIndex(item => item.name === name)
  if (index >= 0) savedFilters.value[index] = next
  else savedFilters.value.push(next)
  persistSavedFilters()
  selectedSavedFilter.value = name
  newFilterName.value = ''
}

async function applySavedFilter(name: string) {
  const saved = savedFilters.value.find(item => item.name === name)
  if (!saved) return
  filterLogic.value = saved.logic
  filterRows.value = saved.rows.map(row => ({ ...row, id: ++filterRowSeq }))
  await applyFilters()
}

function deleteSavedFilter() {
  const name = selectedSavedFilter.value
  if (!name) return
  savedFilters.value = savedFilters.value.filter(item => item.name !== name)
  persistSavedFilters()
  selectedSavedFilter.value = ''
}

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

// 导入导出弹窗
const importDialog = ref({
  show: false,
  format: 'csv' as 'csv' | 'sql',
  data: '',
  filePath: '',
  fileName: '',
  fileSize: 0,
  loading: false
})

const exportDialog = ref({
  show: false,
  format: 'csv' as 'csv' | 'xlsx' | 'sql' | 'json',
  loading: false
})

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

// 导入 / 导出格式选项
const importFormatOptions = [
  { label: 'CSV', value: 'csv' },
  { label: 'SQL', value: 'sql' },
]

const exportFormatOptions = [
  { label: 'CSV', value: 'csv' },
  { label: 'Excel (xlsx)', value: 'xlsx' },
  { label: 'SQL', value: 'sql' },
  { label: 'JSON', value: 'json' },
]

function onPageSizeChange(value: string) {
  browserStore.setPageSize(Number(value))
  reload()
}

function onImportFormatChange(value: string) {
  importDialog.value.format = value as 'csv' | 'sql'
  handleImportFormatChange()
}

function onExportFormatChange(value: string) {
  exportDialog.value.format = value as 'csv' | 'xlsx' | 'sql' | 'json'
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

// PostgreSQL 当前只提供表数据浏览，写入能力尚未实现
const isPostgresConnection = computed(() => connectionStore.currentConnection?.driver === 'postgres')

// 只读连接同样禁止写入（后端也会兜底拦截）
const isReadOnlyConnection = computed(
  () => isPostgresConnection.value || connectionStore.currentConnection?.safety_mode === 'read-only'
)

const readOnlyHint = computed(() =>
  isPostgresConnection.value
    ? t('dataTable.postgresReadOnlyHint')
    : t('dataTable.readOnlyConnectionHint')
)

const readOnlyBadgeLabel = computed(() =>
  isPostgresConnection.value
    ? t('dataTable.postgresReadOnly')
    : t('dataTable.readOnlyConnection')
)

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
  rowHeight: 36,
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
})

onUnmounted(() => {
  document.removeEventListener('keydown', handleKeyDown)
  // 拖拽列宽中卸载时清理全局监听
  document.removeEventListener('mousemove', handleResize)
  document.removeEventListener('mouseup', stopResize)
  clearPendingEdit()
  if (toastTimer) clearTimeout(toastTimer)
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
  filterRows.value = []
  activeFilters.value = []
  filterLogic.value = 'AND'
  activeFilterLogic.value = 'AND'
  selectedSavedFilter.value = ''
  newFilterName.value = ''
  loadSavedFilters()
  
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

// 排序
function sort(column: string) {
  if (sortColumn.value === column) {
    sortDirection.value = sortDirection.value === 'Asc' ? 'Desc' : 'Asc'
  } else {
    sortColumn.value = column
    sortDirection.value = 'Asc'
  }
  loadData()
}

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

  // 日期时间类型
  if (type.includes('datetime')) return '0000-00-00 00:00:00'
  if (type.includes('date')) return '0000-00-00'
  if (type.includes('time')) return '00:00:00'
  if (type.includes('timestamp')) return '0000-00-00 00:00:00'
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

// 判断是否为日期时间类型
function isDateTimeType(type: string): boolean {
  return type.includes('datetime') || type.includes('timestamp') || type.includes('date') || type.includes('time')
}

// 初始化日期时间选择器
function initDateTimePicker(value: string, type: string) {
  showDateTimePicker.value = false
  
  let dateStr = ''
  let timeStr = ''
  
  if (value.includes('T')) {
    // datetime-local 格式: 2024-01-01T12:00:00
    const [date, time] = value.split('T')
    dateStr = date || ''
    timeStr = time?.substring(0, 8) || '00:00:00'
  } else if (type.includes('time')) {
    // time 格式: 12:00:00
    timeStr = value.substring(0, 8)
    dateStr = ''
  } else if (type.includes('date')) {
    // date 格式: 2024-01-01
    dateStr = value.substring(0, 10)
    timeStr = '00:00:00'
  }
  
  selectedDate.value = dateStr
  
  if (dateStr) {
    const date = new Date(dateStr)
    currentYear.value = date.getFullYear()
    currentMonth.value = date.getMonth() + 1
  } else {
    const now = new Date()
    currentYear.value = now.getFullYear()
    currentMonth.value = now.getMonth() + 1
  }
  
  // 解析时间
  if (timeStr) {
    const [h, m, s] = timeStr.split(':').map(Number)
    selectedHour.value = h || 0
    selectedMinute.value = m || 0
    selectedSecond.value = s || 0
  } else {
    selectedHour.value = 0
    selectedMinute.value = 0
    selectedSecond.value = 0
  }
}

// 打开日期时间选择器
function openDateTimePicker(event: MouseEvent) {
  initDateTimePicker(editingValue.value || '', editingColumnType.value)
  
  // 计算弹层位置
  const target = event.currentTarget as HTMLElement
  const rect = target.getBoundingClientRect()
  pickerPosition.value = {
    top: `${rect.bottom + 4}px`,
    left: `${rect.left}px`
  }
  
  showDateTimePicker.value = true
}

// 计算日历天数
const calendarDays = computed(() => {
  const days = []
  const firstDay = new Date(currentYear.value, currentMonth.value - 1, 1)
  const lastDay = new Date(currentYear.value, currentMonth.value, 0)
  const prevMonthLastDay = new Date(currentYear.value, currentMonth.value - 1, 0)
  
  const startWeekday = firstDay.getDay()
  const totalDays = lastDay.getDate()
  const prevMonthDays = prevMonthLastDay.getDate()
  
  const today = new Date()
  const todayStr = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`
  
  // 上个月的日期
  for (let i = startWeekday - 1; i >= 0; i--) {
    const day = prevMonthDays - i
    const date = new Date(currentYear.value, currentMonth.value - 2, day)
    const dateStr = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
    days.push({
      day,
      date: dateStr,
      isCurrentMonth: false,
      isToday: dateStr === todayStr
    })
  }
  
  // 当前月的日期
  for (let day = 1; day <= totalDays; day++) {
    const dateStr = `${currentYear.value}-${String(currentMonth.value).padStart(2, '0')}-${String(day).padStart(2, '0')}`
    days.push({
      day,
      date: dateStr,
      isCurrentMonth: true,
      isToday: dateStr === todayStr
    })
  }
  
  // 下个月的日期
  const remainingDays = 42 - days.length // 6行 x 7列
  for (let day = 1; day <= remainingDays; day++) {
    const date = new Date(currentYear.value, currentMonth.value, day)
    const dateStr = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
    days.push({
      day,
      date: dateStr,
      isCurrentMonth: false,
      isToday: dateStr === todayStr
    })
  }
  
  return days
})

// 上一个月
function prevMonth() {
  if (currentMonth.value === 1) {
    currentMonth.value = 12
    currentYear.value--
  } else {
    currentMonth.value--
  }
}

// 下一个月
function nextMonth() {
  if (currentMonth.value === 12) {
    currentMonth.value = 1
    currentYear.value++
  } else {
    currentMonth.value++
  }
}

// 选择日期
function selectDate(date: string) {
  selectedDate.value = date
  updateEditingValue()
}

// 更新时间
function updateTime() {
  // 确保值在有效范围内
  selectedHour.value = Math.max(0, Math.min(23, selectedHour.value || 0))
  selectedMinute.value = Math.max(0, Math.min(59, selectedMinute.value || 0))
  selectedSecond.value = Math.max(0, Math.min(59, selectedSecond.value || 0))
  updateEditingValue()
}

// 更新编辑值
function updateEditingValue() {
  const type = editingColumnType.value
  const timeStr = `${String(selectedHour.value).padStart(2, '0')}:${String(selectedMinute.value).padStart(2, '0')}:${String(selectedSecond.value).padStart(2, '0')}`
  
  if (type.includes('datetime') || type.includes('timestamp')) {
    // 如果没有选择日期，使用当前日期
    if (!selectedDate.value) {
      selectedDate.value = `${currentYear.value}-${String(currentMonth.value).padStart(2, '0')}-01`
    }
    editingValue.value = `${selectedDate.value}T${timeStr}`
  } else if (type.includes('time')) {
    editingValue.value = timeStr
  } else if (type.includes('date')) {
    editingValue.value = selectedDate.value
  }
}

// 应用日期时间选择
function applyDateTime() {
  updateEditingValue()
  showDateTimePicker.value = false
  // 直接完成编辑并保存
  finishEdit()
}

// 格式化日期时间为 input 接受的格式
function formatDateTimeForInput(value: string, type: string): string {
  if (!value) return ''

  // 替换空格为 T，符合 datetime-local 格式要求
  if (type.includes('datetime') || type.includes('timestamp')) {
    // 格式: 2024-01-01 12:00:00 -> 2024-01-01T12:00:00
    return value.replace(' ', 'T').substring(0, 19)
  }
  if (type.includes('date')) {
    // 格式: 2024-01-01
    return value.substring(0, 10)
  }
  if (type.includes('time')) {
    // 格式: 12:00:00
    return value.substring(0, 8)
  }
  return value
}

// 从 input 格式化回数据库格式
function formatDateTimeFromInput(value: string, type: string): string {
  if (!value) return ''

  if (type.includes('datetime') || type.includes('timestamp')) {
    // 格式: 2024-01-01T12:00:00 -> 2024-01-01 12:00:00
    return value.replace('T', ' ')
  }
  return value
}

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
  const driver = isPostgresConnection.value ? 'postgres' : 'mysql'
  if (isPostgresConnection.value) {
    return table.split('.').map(part => quoteIdentifier(part, driver)).join('.')
  }
  const database = currentTab.value?.database
  return database
    ? `${quoteIdentifier(database, driver)}.${quoteIdentifier(table, driver)}`
    : quoteIdentifier(table, driver)
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
  const driver = isPostgresConnection.value ? 'postgres' : 'mysql'
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
    driver: isPostgresConnection.value ? 'postgres' : 'mysql'
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

function handleScroll() {
  hideContextMenu()
  handleVirtualScroll()

  // 触底预取：滚动到接近底部时自动续页（距底部 120px 内触发）
  const el = tableContainer.value
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

// 导入导出
function showImportDialog() {
  importDialog.value = {
    show: true,
    format: 'csv',
    data: '',
    filePath: '',
    fileName: '',
    fileSize: 0,
    loading: false
  }
}

// 格式化文件大小
function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
}

// 选择导入文件
async function selectImportFile() {
  try {
    importDialog.value.loading = true
    const result = await invoke<[string, string]>('read_file_from_dialog', {
      format: importDialog.value.format
    })
    const [filePath, content] = result
    importDialog.value.filePath = filePath
    importDialog.value.fileName = filePath.split('/').pop() || filePath.split('\\').pop() || ''
    importDialog.value.data = content
    // 估算文件大小（字符数 * 2 字节，近似 UTF-8）
    importDialog.value.fileSize = new Blob([content]).size
  } catch (error: unknown) {
    const errorMessage = String(error)
    // 用户取消选择时不显示错误
    if (!errorMessage.includes('用户取消') && !errorMessage.includes('被取消')) {
      showToast(t('dataTable.selectFileFailed', { error: errorMessage }), 'error')
    }
  } finally {
    importDialog.value.loading = false
  }
}

// 处理导入格式变更
function handleImportFormatChange() {
  // 清除已选择的文件
  importDialog.value.data = ''
  importDialog.value.filePath = ''
  importDialog.value.fileName = ''
  importDialog.value.fileSize = 0
}

function showExportDialog() {
  exportDialog.value = {
    show: true,
    format: 'csv',
    loading: false
  }
}

async function executeExport() {
  if (!currentTab.value) {
    showToast(t('dataTable.openTableFirst'), 'error')
    return
  }
  const conn = connectionStore.currentConnection
  if (!conn) {
    showToast(t('dataTable.connectFirst'), 'error')
    return
  }

  exportDialog.value.loading = true
  const format = exportDialog.value.format
  try {
    // 两种格式都由后端直接写文件：Excel 用工作簿，其余逐行流式写出
    const filePath =
      format === 'xlsx'
        ? await browserStore.exportTableToExcel(
            conn.id,
            currentTab.value.database,
            currentTab.value.tableName,
            `${currentTab.value.tableName}.xlsx`
          )
        : await browserStore.exportTableToFile(
            conn.id,
            currentTab.value.database,
            currentTab.value.tableName,
            format,
            `${currentTab.value.tableName}.${format}`
          )
    showToast(t('dataTable.fileSaved', { path: filePath }))
    exportDialog.value.show = false
  } catch (e: unknown) {
    const errorMessage = String(e)
    // 用户取消保存时不提示错误
    if (!errorMessage.includes('用户取消')) {
      console.error('Export failed:', e)
      showToast(t('dataTable.exportFailed', { error: errorMessage }), 'error')
    }
  } finally {
    exportDialog.value.loading = false
  }
}

async function executeImport() {
  if (!currentTab.value) {
    showToast(t('dataTable.openTableFirst'), 'error')
    return
  }
  const conn = connectionStore.currentConnection
  if (!conn) {
    showToast(t('dataTable.connectFirst'), 'error')
    return
  }

  if (!importDialog.value.data.trim()) {
    showToast(t('dataTable.importDataRequired'), 'error')
    return
  }

  importDialog.value.loading = true
  try {
    const count = await browserStore.importData(
      conn.id,
      currentTab.value.database,
      currentTab.value.tableName,
      importDialog.value.format,
      importDialog.value.data
    )
    showToast(t('dataTable.importSuccess', { count }))
    importDialog.value.show = false
    await loadData()
  } catch (e: unknown) {
    console.error('Import failed:', e)
    showToast(t('dataTable.importFailed', { error: String(e) }), 'error')
  } finally {
    importDialog.value.loading = false
  }
}

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
  background: var(--bg-primary);
  overflow: hidden;
  position: relative;
}

/* 工具栏 */
.table-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--space-2) var(--space-4);
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-default);
  flex-shrink: 0;
  gap: var(--space-4);
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  flex: 1;
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.table-info {
  font-weight: var(--font-semibold);
  font-size: var(--text-sm);
  white-space: nowrap;
  color: var(--text-primary);
}

.toolbar-actions {
  display: flex;
  gap: var(--space-2);
}

.toolbar-actions button {
  padding: var(--space-1) var(--space-3);
  border: 1px solid var(--border-default);
  background: var(--surface-default);
  color: var(--text-primary);
  cursor: pointer;
  border-radius: var(--radius-md);
  font-size: var(--text-xs);
  white-space: nowrap;
  display: flex;
  align-items: center;
  gap: var(--space-1);
  transition: var(--transition-colors);
}

.toolbar-actions button:hover:not(:disabled) {
  background: var(--surface-hover);
  border-color: var(--border-strong);
}

.toolbar-actions button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.toolbar-actions button.btn-cancel-query {
  border-color: var(--color-danger);
  color: var(--color-danger);
}

.search-input {
  padding: var(--space-1) var(--space-2);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  font-size: var(--text-xs);
  background: var(--surface-default);
  color: var(--text-primary);
  width: 150px;
  transition: var(--transition-colors);
}

.search-input:focus {
  outline: none;
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}

.row-info {
  color: var(--text-secondary);
  font-size: var(--text-xs);
  white-space: nowrap;
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
  width: auto;
  border-right: 1px solid var(--border-default);
}

/* 行号列 */
.row-number-header,
.row-number {
  width: 40px !important;
  min-width: 20px !important;
  max-width: 50px !important;
  text-align: center;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-default);
  padding: var(--space-2) !important;
}

.row-number {
  text-align: center;
  vertical-align: middle;
}

.row-num {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

th, td {
  padding: var(--space-2);
  text-align: left;
  border-bottom: 1px solid var(--border-default);
  border-right: 1px solid var(--border-default);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  position: relative;
}

th {
  background: var(--bg-secondary);
  font-weight: var(--font-semibold);
  cursor: pointer;
  user-select: none;
  position: sticky;
  top: 0;
  z-index: 1;
}

/* ========== 冻结列样式 ========== */
.frozen-row-number,
.frozen-col {
  position: sticky !important;
  /* 不透明背景：水平滚动时阻止下面单元格穿透 */
  background: inherit;
}

/* thead 里的冻结列：同时 top + left 双向 sticky，层级要盖过普通表头(z-index:1)和普通冻结 td(z-index:2) */
thead .frozen-row-number,
thead .frozen-col {
  z-index: 3;
}

/* 普通冻结单元格：高于默认 td、低于表头冻结 */
.frozen-row-number,
tbody .frozen-col {
  z-index: 2;
}

/* 冻结列右边缘阴影，水平滚动时视觉分隔更清晰 */
.frozen-row-number,
.frozen-col {
  box-shadow: 2px 0 4px -2px rgba(0, 0, 0, 0.18);
}

/* 选中行里的冻结单元格保持选中背景（tr.selected 里的 td 背景色） */
tr.selected .frozen-row-number,
tr.selected .frozen-col {
  /* 背景色会由 tr.selected 里的 td 规则设定，z-index 仍保持 2 */
}

.th-content {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.col-name {
  overflow: hidden;
  text-overflow: ellipsis;
}

.data-type {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-normal);
}

.sort-indicator {
  margin-left: var(--space-1);
  color: var(--brand-500);
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
  background: var(--surface-hover);
}

tr.selected {
  background: #87CEEB !important;
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
  color: var(--info-500);
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
  background: var(--color-background);
  box-sizing: border-box;
  color: var(--color-text);
  box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.15), 0 4px 12px rgba(0, 0, 0, 0.15);
  transition: all 0.15s ease;
}

.edit-input:focus {
  border-color: var(--brand-600);
  box-shadow: 0 0 0 4px rgba(59, 130, 246, 0.2), 0 6px 16px rgba(0, 0, 0, 0.2);
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
  background: var(--color-background);
  border: 1px solid var(--brand-500);
  box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.15), 0 4px 12px rgba(0, 0, 0, 0.15);
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
  color: var(--danger-500);
}

.datetime-actions .btn-cancel:hover {
  background: var(--danger-100);
}

/* 自定义日期时间选择器弹层 */
.datetime-picker-popup {
  position: fixed;
  background: var(--color-background);
  border: 1px solid var(--border-color);
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
  border-top: 1px solid var(--border-color);
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
  border: 1px solid var(--border-color);
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
  border-top: 1px solid var(--border-color);
  justify-content: flex-end;
}

.picker-actions button {
  padding: 5px 14px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  background: var(--color-background);
  cursor: pointer;
  font-size: 12px;
  transition: all 0.15s ease;
}

.picker-actions .btn-cancel {
  color: var(--text-secondary);
  border-color: var(--border-color);
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
  background: rgba(59, 130, 246, 0.05);
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
  background: var(--color-background);
  border: 1px solid var(--color-border);
  border-radius: 4px;
  box-shadow: 0 2px 8px var(--color-shadow);
  z-index: 1000;
  min-width: 120px;
}

.menu-item {
  padding: 8px 16px;
  cursor: pointer;
  font-size: 13px;
  color: var(--color-text);
}

.menu-item:hover {
  background: var(--color-background-hover);
}

.menu-divider {
  height: 1px;
  background: var(--color-border);
  margin: 4px 0;
}

/* 分页 */
.table-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 16px;
  background: var(--color-background-soft);
  border-top: 1px solid var(--color-border);
  flex-shrink: 0;
}

.pagination {
  display: flex;
  gap: 8px;
  align-items: center;
}

.pagination button {
  padding: 4px 12px;
  border: 1px solid var(--color-border);
  background: var(--color-background);
  color: var(--color-text);
  cursor: pointer;
  border-radius: 4px;
  font-size: 12px;
}

.pagination button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 分页条数选择：外层定宽，触发器与分页按钮同高 */
.page-size {
  width: 132px;
  flex-shrink: 0;
}

.page-size :deep(.custom-select .select-trigger) {
  padding: 0 8px;
  border-color: var(--color-border);
  border-radius: 4px;
  background: var(--color-background);
  color: var(--color-text);
  font-size: 12px;
}

/* 空状态 */
.empty-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--color-text-secondary);
  font-size: 14px;
}

.empty-state.error-state {
  color: var(--color-danger);
}

/* 空表提示 */
.empty-row {
  background: transparent;
}

.empty-cell {
  text-align: center;
  color: var(--color-text-secondary);
  font-size: 13px;
  padding: 24px 0;
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
  color: var(--color-text-secondary);
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
  background: var(--color-background);
  padding: 24px;
  border-radius: 8px;
  box-shadow: 0 4px 16px var(--color-shadow);
  min-width: 300px;
}

.confirm-dialog h3 {
  margin: 0 0 12px 0;
  font-size: 16px;
  color: var(--color-text);
}

.confirm-dialog p {
  margin: 0 0 20px 0;
  color: var(--color-text-secondary);
}

.confirm-actions {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
}

.btn-primary {
  background: var(--color-success);
  color: white;
  border: none;
  padding: 8px 16px;
  border-radius: 4px;
  cursor: pointer;
}

.btn-secondary {
  background: var(--color-background-soft);
  border: 1px solid var(--color-border);
  color: var(--color-text);
  padding: 8px 16px;
  border-radius: 4px;
  cursor: pointer;
}

/* 工具栏样式 */
.toolbar-divider {
  width: 1px;
  height: 20px;
  background: var(--color-border);
  margin: 0 4px;
}

.toolbar-actions button.active {
  background: rgba(25, 118, 210, 0.15);
  border-color: var(--color-primary);
  color: var(--color-primary);
}

/* 列选择器（宽表按需加载） */
.column-selector {
  position: relative;
}

.column-selector-menu {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  z-index: 60;
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

.transaction-indicator {
  color: var(--color-success);
  font-size: 16px;
  animation: pulse 1.5s infinite;
}

/* 无主键只读徽标 */
.readonly-badge {
  display: inline-flex;
  align-items: center;
  padding: 2px 8px;
  border-radius: 999px;
  font-size: var(--text-xs);
  line-height: 1.4;
  color: var(--text-secondary);
  background: var(--surface-hover);
  border: 1px solid var(--border-default);
  white-space: nowrap;
}

/* 保存预览弹窗 */
.save-preview-dialog {
  max-width: 760px;
}

.preview-summary {
  margin: 0 0 6px 0;
  font-size: 13px;
  color: var(--color-text);
}

.preview-hint {
  margin: 0 0 12px 0;
  font-size: 12px;
  color: var(--color-text-secondary);
}

.save-preview-dialog .result-textarea {
  width: 100%;
  min-height: 220px;
  resize: vertical;
  padding: 10px;
  font-family: monospace;
  font-size: 12px;
  line-height: 1.5;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-background-soft);
  color: var(--color-text);
  box-sizing: border-box;
}

.preview-error {
  margin: 12px 0 0 0;
  font-size: 12px;
  color: var(--color-danger);
  white-space: pre-wrap;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}

/* 导入导出弹窗 */
.dialog {
  background: var(--color-background);
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
  border-bottom: 1px solid var(--color-border);
  padding-bottom: 12px;
  color: var(--color-text);
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
  color: var(--color-text-secondary);
}

.form-group textarea {
  width: 100%;
  padding: 8px 12px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  font-size: 13px;
  font-family: inherit;
  background: var(--color-background);
  color: var(--color-text);
}

/* 自定义下拉框：触发器与对话框内输入框保持相同的外观 */
.form-group :deep(.select-trigger) {
  width: 100%;
  height: 38px;
  min-height: 38px;
  padding: 0 12px;
  border-color: var(--color-border);
  border-radius: 4px;
  background: var(--color-background);
  color: var(--color-text);
  font-size: 13px;
}

.form-group textarea {
  resize: vertical;
  min-height: 120px;
  font-family: monospace;
}

.form-group textarea.result-textarea {
  background: var(--color-background-soft);
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
  background: var(--color-background-soft);
  border-radius: 4px;
  border: 1px solid var(--color-border);
}

.file-name {
  font-weight: 500;
  color: var(--color-text);
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-size {
  color: var(--color-text-secondary);
  font-size: 12px;
}

.file-hint {
  color: var(--color-text-secondary);
  font-size: 13px;
  padding: 8px 0;
}

.file-preview {
  background: var(--color-background-soft);
  border: 1px solid var(--color-border);
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
  color: var(--color-text);
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
  background: var(--color-success);
}

.toast-message.error {
  background: var(--color-danger);
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
  min-width: 16px;
  height: 16px;
  padding: 0 4px;
  border-radius: 8px;
  background: var(--color-primary);
  color: #fff;
  font-size: 10px;
  line-height: 1;
}

.filter-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 16px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-default);
  flex-shrink: 0;
}

.filter-panel-header {
  display: flex;
  align-items: center;
  gap: 12px;
}

.filter-panel-title {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.filter-logic {
  display: flex;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  overflow: hidden;
}

.filter-logic button {
  padding: 2px 10px;
  border: none;
  background: var(--surface-default);
  color: var(--text-secondary);
  font-size: 11px;
  cursor: pointer;
}

.filter-logic button.active {
  background: var(--color-primary);
  color: #fff;
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
  gap: 6px;
}

.filter-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.filter-join {
  min-width: 48px;
  font-size: 11px;
  color: var(--text-secondary);
}

.filter-select,
.filter-value {
  padding: 3px 6px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-primary);
  font-size: 12px;
}

.filter-select {
  min-width: 130px;
}

.filter-op {
  min-width: 90px;
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
  color: var(--color-danger);
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
  margin-left: auto;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0 2px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  opacity: 0.5;
  cursor: pointer;
}

.th-filter-btn:hover,
.th-filter-btn.active {
  opacity: 1;
  color: var(--color-primary);
}
</style>
