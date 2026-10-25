<template>
  <span class="status-dot" :class="`status-dot--${status}`">
    <Circle :size="size" :fill="fillColor" />
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { Circle } from 'lucide-vue-next'

interface Props {
  status: 'online' | 'offline' | 'reconnecting' | 'error'
  size?: number
}

const props = withDefaults(defineProps<Props>(), {
  size: 8
})

const fillColor = computed(() => {
  const colors: Record<string, string> = {
    online: 'var(--status-online)',
    offline: 'var(--status-offline)',
    reconnecting: 'var(--status-busy)',
    error: 'var(--status-error)'
  }
  return colors[props.status] || colors.offline
})
</script>

<style scoped>
.status-dot {
  display: inline-flex;
  align-items: center;
}

.status-dot :deep(svg) {
  color: transparent;
}

.status-dot--online :deep(svg) {
  animation: pulse 2s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.6; }
}
</style>
