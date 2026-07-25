/**
 * Screen Wake Lock Composable
 *
 * 封装 Web Screen Wake Lock API（navigator.wakeLock），在 PlayerView 打开且
 * 处于播放状态时阻止屏幕自动关闭 / 进入休眠模式。
 *
 * 行为：
 * - setActive(true)  请求 'screen' 类型 wake lock
 * - setActive(false) 释放当前持有的 wake lock
 * - 监听 visibilitychange：页面被隐藏时浏览器会自动释放 wake lock，
 *   重新可见时若仍期望保持唤醒则自动重新请求
 * - 组件作用域销毁时自动释放并移除监听器
 * - 不支持 Wake Lock API 的环境静默降级，仅打印一次警告
 *
 * 兼容性：
 * - Tauri Windows: WebView2 (Chromium 内核) ✓ 支持
 * - Tauri macOS:   WKWebView 13+ ✓ 支持
 * - Tauri Linux:   WebKitGTK 较新版本 ✓ 支持
 * - 普通 Web 浏览器: Chrome 84+ / Edge 84+ / Safari 16.4+ 支持；Firefox 暂不支持
 *
 * @example
 * import { useWakeLock } from '@/composables/useWakeLock.js'
 * import { watch } from 'vue'
 * const { setActive } = useWakeLock()
 * watch(() => store.state.isPlaying, (v) => setActive(v), { immediate: true })
 */

import { ref, onScopeDispose } from 'vue'

export function useWakeLock() {
  // 当前持有的 sentinel；null 表示未持有
  let sentinel = null
  // 用户期望状态：true 表示应当保持唤醒
  let desired = false
  // 是否已警告过不支持（避免重复刷屏）
  let warnedUnsupported = false

  // 是否支持 Wake Lock API
  const supported = ref(
    typeof navigator !== 'undefined' && 'wakeLock' in navigator
  )

  async function request() {
    if (!supported.value) {
      if (!warnedUnsupported) {
        warnedUnsupported = true
        console.warn(
          '[useWakeLock] 当前环境不支持 Screen Wake Lock API，屏幕防休眠功能已降级。'
        )
      }
      return
    }
    // 已持有则不重复请求
    if (sentinel) return
    // 页面不可见时浏览器会拒绝请求，跳过等 visibilitychange 触发
    if (document.visibilityState !== 'visible') return
    try {
      sentinel = await navigator.wakeLock.request('screen')
      if (!sentinel) return
      // 系统主动释放（页面隐藏、电量不足等）时清理引用；
      // 若用户仍期望唤醒且页面仍可见，尝试重新请求
      sentinel.addEventListener('release', () => {
        sentinel = null
        if (desired && document.visibilityState === 'visible') {
          request().catch(() => {})
        }
      })
    } catch (err) {
      // 常见失败原因：用户拒绝、页面不可见、电量不足、安全上下文限制
      console.warn('[useWakeLock] 请求 wake lock 失败:', err?.message || err)
    }
  }

  function release() {
    if (!sentinel) return
    try {
      sentinel.release()
    } catch {
      /* sentinel 可能已被系统释放，忽略 */
    }
    sentinel = null
  }

  /**
   * 设置期望的唤醒状态。
   * @param {boolean} active true 保持屏幕唤醒；false 释放
   */
  async function setActive(active) {
    desired = !!active
    if (desired) {
      await request()
    } else {
      release()
    }
  }

  // 页面重新可见时，若仍期望唤醒且当前未持有，自动重新请求
  function onVisibilityChange() {
    if (desired && document.visibilityState === 'visible' && !sentinel) {
      request().catch(() => {})
    }
  }
  document.addEventListener('visibilitychange', onVisibilityChange)

  // 作用域销毁时清理：释放 wake lock + 移除监听器
  onScopeDispose(() => {
    document.removeEventListener('visibilitychange', onVisibilityChange)
    desired = false
    release()
  })

  return {
    supported,
    setActive,
  }
}

export default useWakeLock
