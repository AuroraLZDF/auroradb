import { ref, watch, onScopeDispose, type Ref } from 'vue'

export function useDebounce<T>(value: Ref<T>, delay: number = 300) {
  const debouncedValue = ref<T>(value.value) as Ref<T>
  let timeoutId: ReturnType<typeof setTimeout> | null = null

  const cancel = () => {
    if (timeoutId !== null) {
      clearTimeout(timeoutId)
      timeoutId = null
    }
  }

  const flush = () => {
    cancel()
    debouncedValue.value = value.value
  }

  watch(value, (newValue) => {
    cancel()
    timeoutId = setTimeout(() => {
      debouncedValue.value = newValue
      timeoutId = null
    }, delay)
  })

  onScopeDispose(cancel)

  return { debouncedValue, flush, cancel }
}

