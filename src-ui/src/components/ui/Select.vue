<template>
  <div class="custom-select" ref="selectRef">
    <!-- 选择框 -->
    <div
      class="select-trigger"
      :class="{ open: isOpen, disabled }"
      @click="toggleDropdown"
    >
      <span class="select-value">{{ displayValue || placeholderText }}</span>
      <ChevronDown class="select-arrow" :size="16" />
    </div>

    <!-- 下拉选项列表 -->
    <Transition name="dropdown">
      <div v-if="isOpen" class="select-dropdown">
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
        <div class="select-options" ref="optionsRef">
          <div
            v-for="option in filteredOptions"
            :key="option.value"
            class="select-option"
            :class="{ selected: option.value === modelValue }"
            @click="selectOption(option)"
          >
            <span class="option-label">{{ option.label }}</span>
            <Check v-if="option.value === modelValue" class="option-check" :size="14" />
          </div>
          <div v-if="filteredOptions.length === 0" class="select-empty">
            {{ t('ui.noMatchOptions') }}
          </div>
        </div>
      </div>
    </Transition>
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
  modelValue: string
  options: (Option | string)[]
  placeholder?: string
  disabled?: boolean
  searchable?: boolean
}>(), {
  disabled: false,
  searchable: false
})

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()

const selectRef = ref<HTMLElement | null>(null)
const optionsRef = ref<HTMLElement | null>(null)
const searchInputRef = ref<HTMLInputElement | null>(null)
const isOpen = ref(false)
const searchText = ref('')

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

// 显示值
const displayValue = computed(() => {
  const option = normalizedOptions.value.find(opt => opt.value === props.modelValue)
  return option?.label || ''
})

// 占位提示（未传 placeholder 时使用通用文案）
const placeholderText = computed(() => props.placeholder ?? t('common.pleaseSelect'))

// 切换下拉框
function toggleDropdown() {
  if (props.disabled) return
  isOpen.value = !isOpen.value
  if (isOpen.value) {
    searchText.value = ''
    nextTick(() => {
      if (props.searchable && searchInputRef.value) {
        searchInputRef.value.focus()
      }
      // 滚动到选中项
      scrollToSelected()
    })
  }
}

// 关闭下拉框
function closeDropdown() {
  isOpen.value = false
  searchText.value = ''
}

// 选择选项
function selectOption(option: Option) {
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

// 点击外部关闭
function handleClickOutside(e: MouseEvent) {
  if (selectRef.value && !selectRef.value.contains(e.target as Node)) {
    closeDropdown()
  }
}

onMounted(() => {
  document.addEventListener('click', handleClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
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
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 14px;
  cursor: pointer;
  transition: border-color 0.2s, box-shadow 0.2s;
  min-height: 40px;
}

.select-trigger:hover:not(.disabled) {
  border-color: var(--brand-400);
}

.select-trigger.open {
  border-color: var(--brand-500);
  box-shadow: 0 0 0 3px rgba(33, 150, 243, 0.1);
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

.select-arrow {
  flex-shrink: 0;
  margin-left: 8px;
  color: var(--text-secondary);
  transition: transform 0.2s;
}

.select-trigger.open .select-arrow {
  transform: rotate(180deg);
}

/* 下拉列表 */
.select-dropdown {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  background: var(--bg-primary);
  border: 1px solid var(--border-default);
  border-radius: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.15);
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
  font-size: 14px;
  color: var(--text-primary);
  cursor: pointer;
  transition: background 0.15s;
}

.select-option:hover {
  background: var(--bg-hover);
}

.select-option.selected {
  background: rgba(33, 150, 243, 0.1);
  color: var(--brand-500);
  font-weight: 500;
}

.option-label {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.option-check {
  flex-shrink: 0;
  margin-left: 8px;
  color: var(--brand-500);
}

.select-empty {
  padding: 20px;
  text-align: center;
  color: var(--text-tertiary);
  font-size: 13px;
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