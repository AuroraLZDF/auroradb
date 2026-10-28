<template>
  <div class="custom-select" ref="selectRef">
    <!-- 选择框 -->
    <div
      class="select-trigger"
      :class="[{ open: isOpen, disabled }, `size-${size}`]"
      @click="toggleDropdown"
    >
      <span class="select-value" :class="{ 'is-placeholder': !displayValue }">
        {{ displayValue || placeholderText }}
      </span>
      <ChevronDown class="select-arrow" :size="size === 'sm' ? 12 : 16" />
    </div>

    <!-- 下拉选项列表：挂载到 body，避免被弹窗等滚动容器裁剪 -->
    <Teleport to="body">
      <Transition name="dropdown">
        <div
          v-if="isOpen"
          ref="dropdownRef"
          class="select-dropdown"
          :class="`size-${size}`"
          :style="dropdownStyle"
        >
          <!-- 搜索框（可选） -->
          <div v-if="searchable" class="select-search">
            <input
              ref="searchInputRef"
              v-model="searchText"
              type="text"
              :placeholder="t('common.search')"
              @click.stop
              @keydown.esc="closeDropdown"
            />
          </div>

          <!-- 选项列表（可滚动） -->
          <div class="select-options" ref="optionsRef" :style="{ maxHeight: optionsMaxHeight }">
            <div
              v-for="option in filteredOptions"
              :key="option.value"
              class="select-option"
              :class="{ selected: isSelected(option) }"
              @click="selectOption(option)"
            >
              <span class="option-label">{{ option.label }}</span>
              <Check v-if="isSelected(option)" class="option-check" :size="size === 'sm' ? 12 : 14" />
            </div>
            <div v-if="filteredOptions.length === 0" class="select-empty">
              {{ t('ui.noMatchOptions') }}
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick } from 'vue'
import { ChevronDown, Check } from 'lucide-vue-next'
import { t } from '@/i18n'

interface Option {
  label: string
  value: string
}

const props = withDefaults(defineProps<{
  modelValue?: string
  multiValue?: string[]
  options: (Option | string)[]
  placeholder?: string
  disabled?: boolean
  searchable?: boolean
  multiple?: boolean
  size?: 'sm' | 'md'
}>(), {
  modelValue: '',
  multiValue: undefined,
  disabled: false,
  searchable: false,
  multiple: false,
  size: 'md'
})

const emit = defineEmits<{
  'update:modelValue': [value: string]
  'update:multiValue': [value: string[]]
}>()

const selectRef = ref<HTMLElement | null>(null)
const dropdownRef = ref<HTMLElement | null>(null)
const optionsRef = ref<HTMLElement | null>(null)
const searchInputRef = ref<HTMLInputElement | null>(null)
const isOpen = ref(false)
const searchText = ref('')
const dropdownStyle = ref<Record<string, string>>({})
const optionsMaxHeight = ref('240px')

/** 下拉框与触发框的间距 */
const DROPDOWN_GAP = 4
/** 下拉框与视口边缘的最小间距 */
const VIEWPORT_MARGIN = 8
/** 选项列表优先展示的最大高度 */
const OPTIONS_MAX_HEIGHT = 240
/** 搜索框占用的高度（用于计算剩余可用空间） */
const SEARCH_AREA_HEIGHT = 49

// 标准化选项格式
const normalizedOptions = computed<Option[]>(() => {
  return props.options.map(opt => {
    if (typeof opt === 'string') {
      return { label: opt, value: opt }
    }
    return opt
  })
})

// 过滤后的选项
const filteredOptions = computed(() => {
  if (!searchText.value) return normalizedOptions.value
  const search = searchText.value.toLowerCase()
  return normalizedOptions.value.filter(opt =>
    opt.label.toLowerCase().includes(search)
  )
})

// 当前选中值（多选时取 multiValue，单选时取 modelValue）
const selectedValues = computed<string[]>(() => {
  if (props.multiple) return props.multiValue ?? []
  return props.modelValue ? [props.modelValue] : []
})

// 显示值（多选按选择顺序拼接）
const displayValue = computed(() => {
  if (selectedValues.value.length === 0) return ''
  const labelMap = new Map(normalizedOptions.value.map(opt => [opt.value, opt.label]))
  return selectedValues.value.map(value => labelMap.get(value) ?? value).join(', ')
})

// 选项是否选中
function isSelected(option: Option): boolean {
  return selectedValues.value.includes(option.value)
}

// 占位提示（未传 placeholder 时使用通用文案）
const placeholderText = computed(() => props.placeholder ?? t('common.pleaseSelect'))

// 切换下拉框
function toggleDropdown() {
  if (props.disabled) return
  if (isOpen.value) {
    closeDropdown()
    return
  }
  searchText.value = ''
  // 先算好位置再打开，避免下拉框闪现到错误位置
  updateDropdownPosition()
  isOpen.value = true
  nextTick(() => {
    if (props.searchable && searchInputRef.value) {
      searchInputRef.value.focus()
    }
    // 滚动到选中项
    scrollToSelected()
  })
}

// 关闭下拉框
function closeDropdown() {
  isOpen.value = false
  searchText.value = ''
}

// 选择选项
function selectOption(option: Option) {
  if (props.multiple) {
    const current = props.multiValue ?? []
    const next = current.includes(option.value)
      ? current.filter(value => value !== option.value)
      : [...current, option.value]
    emit('update:multiValue', next)
    return
  }
  emit('update:modelValue', option.value)
  closeDropdown()
}

// 滚动到选中项
function scrollToSelected() {
  if (!optionsRef.value) return
  const selectedEl = optionsRef.value.querySelector('.select-option.selected') as HTMLElement
  if (selectedEl) {
    selectedEl.scrollIntoView({ block: 'nearest' })
  }
}

/**
 * 计算下拉框的固定定位坐标：
 * 优先向下展开，下方空间不足时向上展开，并按剩余空间限制选项列表高度。
 */
function updateDropdownPosition() {
  const trigger = selectRef.value?.querySelector('.select-trigger') as HTMLElement | null
  if (!trigger) return
  const rect = trigger.getBoundingClientRect()
  const searchHeight = props.searchable ? SEARCH_AREA_HEIGHT : 0
  const spaceBelow = window.innerHeight - rect.bottom - DROPDOWN_GAP - VIEWPORT_MARGIN
  const spaceAbove = rect.top - DROPDOWN_GAP - VIEWPORT_MARGIN
  const openUp = spaceBelow < 160 && spaceAbove > spaceBelow
  const available = Math.max(96, (openUp ? spaceAbove : spaceBelow) - searchHeight)
  optionsMaxHeight.value = `${Math.min(OPTIONS_MAX_HEIGHT, available)}px`
  dropdownStyle.value = {
    position: 'fixed',
    left: `${Math.round(rect.left)}px`,
    width: `${Math.round(rect.width)}px`,
    top: openUp ? 'auto' : `${Math.round(rect.bottom + DROPDOWN_GAP)}px`,
    bottom: openUp ? `${Math.round(window.innerHeight - rect.top + DROPDOWN_GAP)}px` : 'auto',
  }
}

// 滚动或窗口尺寸变化时重新定位，避免下拉框错位
function handleViewportChange() {
  if (isOpen.value) updateDropdownPosition()
}

// 点击外部关闭
function handleClickOutside(e: MouseEvent) {
  const target = e.target as Node
  if (selectRef.value?.contains(target)) return
  if (dropdownRef.value?.contains(target)) return
  closeDropdown()
}

onMounted(() => {
  document.addEventListener('click', handleClickOutside)
  window.addEventListener('resize', handleViewportChange)
  window.addEventListener('scroll', handleViewportChange, true)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
  window.removeEventListener('resize', handleViewportChange)
  window.removeEventListener('scroll', handleViewportChange, true)
})
</script>

<style scoped>
.custom-select {
  position: relative;
  width: 100%;
}

/* 选择框触发器 */
.select-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-primary);
  font-size: var(--text-base);
  cursor: pointer;
  transition: var(--transition-colors), box-shadow var(--duration-fast) var(--ease-in-out);
  min-height: 40px;
}

/* 紧凑尺寸：用于表格单元格等密集场景 */
.select-trigger.size-sm {
  min-height: 26px;
  height: 26px;
  padding: 0 var(--space-2);
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
}

.select-trigger:hover:not(.disabled) {
  border-color: var(--brand-400);
}

.select-trigger.open {
  border-color: var(--brand-500);
  box-shadow: 0 0 0 3px rgba(33, 150, 243, 0.15);
}

.select-trigger.disabled {
  background: var(--bg-secondary);
  color: var(--text-tertiary);
  cursor: not-allowed;
}

.select-value {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.select-value.is-placeholder {
  color: var(--text-tertiary);
}

.select-arrow {
  flex-shrink: 0;
  margin-left: var(--space-1);
  color: var(--text-tertiary);
  transition: transform var(--duration-fast) var(--ease-in-out);
}

.select-trigger.open .select-arrow {
  transform: rotate(180deg);
}

/* 下拉列表（坐标与宽度由 JS 以内联样式注入） */
.select-dropdown {
  background: var(--surface-default);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  z-index: 1000;
  overflow: hidden;
}

/* 搜索框 */
.select-search {
  padding: 8px;
  border-bottom: 1px solid var(--border-subtle);
}

.select-search input {
  width: 100%;
  padding: 8px 10px;
  border: 1px solid var(--border-default);
  border-radius: 4px;
  font-size: 13px;
  background: var(--bg-secondary);
  color: var(--text-primary);
}

.select-search input:focus {
  outline: none;
  border-color: var(--brand-500);
}

/* 选项列表 - 支持滚动 */
.select-options {
  max-height: 240px;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-width: thin;
  scrollbar-color: var(--border-default) transparent;
}

/* 自定义滚动条 */
.select-options::-webkit-scrollbar {
  width: 8px;
}

.select-options::-webkit-scrollbar-track {
  background: transparent;
}

.select-options::-webkit-scrollbar-thumb {
  background: var(--border-default);
  border-radius: 4px;
}

.select-options::-webkit-scrollbar-thumb:hover {
  background: var(--text-tertiary);
}

/* 选项 */
.select-option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  font-size: var(--text-base);
  color: var(--text-primary);
  cursor: pointer;
  transition: background-color var(--duration-fast) var(--ease-in-out);
}

/* 紧凑尺寸的选项 */
.select-dropdown.size-sm .select-option {
  padding: 6px 10px;
  font-size: var(--text-sm);
}

.select-option:hover {
  background: var(--surface-hover);
}

.select-option.selected {
  background: var(--surface-selected);
  color: var(--brand-600);
  font-weight: var(--font-medium);
}

.option-label {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.option-check {
  flex-shrink: 0;
  margin-left: var(--space-1);
  color: var(--brand-500);
}

.select-empty {
  padding: var(--space-5);
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}

/* 下拉动画 */
.dropdown-enter-active,
.dropdown-leave-active {
  transition: opacity 0.15s ease, transform 0.15s ease;
}

.dropdown-enter-from,
.dropdown-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>