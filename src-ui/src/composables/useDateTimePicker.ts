import { ref, computed, type Ref } from 'vue'

/**
 * 数据表格单元格编辑的自定义日期时间选择器。
 * 与 DataTable.vue 的编辑值双向联动：直接读写 editingValue/editingColumnType。
 */
export function useDateTimePicker(options: {
  editingValue: Ref<string>
  editingColumnType: Ref<string>
  /** 「确认」按钮：写回编辑值后直接结束单元格编辑 */
  finishEdit: () => void
}) {
  const { editingValue, editingColumnType } = options

  const showDateTimePicker = ref(false)
  const pickerPosition = ref({ top: '0px', left: '0px' })
  const currentYear = ref(new Date().getFullYear())
  const currentMonth = ref(new Date().getMonth() + 1)
  const selectedDate = ref('')
  const selectedHour = ref(0)
  const selectedMinute = ref(0)
  const selectedSecond = ref(0)

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
    options.finishEdit()
  }

  return {
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
  }
}

// 判断是否为日期时间类型
export function isDateTimeType(type: string): boolean {
  return type.includes('datetime') || type.includes('timestamp') || type.includes('date') || type.includes('time')
}

// 格式化日期时间为 input 接受的格式
export function formatDateTimeForInput(value: string, type: string): string {
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
export function formatDateTimeFromInput(value: string, type: string): string {
  if (!value) return ''

  if (type.includes('datetime') || type.includes('timestamp')) {
    // 格式: 2024-01-01T12:00:00 -> 2024-01-01 12:00:00
    return value.replace('T', ' ')
  }
  return value
}
