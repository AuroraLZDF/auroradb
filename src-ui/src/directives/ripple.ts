// 按钮点击波纹效果指令
import type { DirectiveBinding } from 'vue'

interface RippleOptions {
  color?: string
  duration?: number
}

export const vRipple = {
  mounted(el: HTMLElement, binding: DirectiveBinding<RippleOptions | undefined>) {
    const options = binding.value || {}
    const color = options.color || 'rgba(255, 255, 255, 0.3)'
    const duration = options.duration || 400

    // 确保元素有相对定位
    if (getComputedStyle(el).position === 'static') {
      el.style.position = 'relative'
    }
    el.style.overflow = 'hidden'

    el.addEventListener('click', (e: MouseEvent) => {
      const rect = el.getBoundingClientRect()
      const size = Math.max(rect.width, rect.height)
      const x = e.clientX - rect.left - size / 2
      const y = e.clientY - rect.top - size / 2

      const ripple = document.createElement('span')
      ripple.style.cssText = `
        position: absolute;
        border-radius: 50%;
        background: ${color};
        width: ${size}px;
        height: ${size}px;
        left: ${x}px;
        top: ${y}px;
        transform: scale(0);
        animation: ripple-effect ${duration}ms ease-out;
        pointer-events: none;
      `

      el.appendChild(ripple)

      setTimeout(() => {
        ripple.remove()
      }, duration)
    })
  }
}

// 添加动画样式到 document
const style = document.createElement('style')
style.textContent = `
  @keyframes ripple-effect {
    to {
      transform: scale(2);
      opacity: 0;
    }
  }
`
document.head.appendChild(style)
