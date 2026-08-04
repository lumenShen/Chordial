<template>
  <Teleport to="body">
    <div ref="root" class="player-view" tabindex="0" @keydown="onKeydown">
      <!-- AMLL React 播放器挂载点 -->
      <div ref="amllContainer" class="amll-container" @click="onAmllClick" />
      <!-- 顶部窗口拖动区域（仅桌面端）
           pointer-events:none 让点击穿透到 AMLL 的 ControlThumb；
           -webkit-app-region:drag 由 Tauri native 层识别用于拖动窗口 -->
      <div v-if="isDesktop" class="drag-region" />

      <!-- AMLL MenuButton 触发的歌曲菜单 Sheet -->
      <TrackMenuSheet />

      <!-- AMLL bottomControls 第一个按钮（type="playlist"）触发的播放队列 Sheet -->
      <PlayQueueSheet />
    </div>
  </Teleport>
</template>

<script setup>
import { ref, onMounted, onBeforeUnmount, watch } from 'vue'
import { createRoot } from 'react-dom/client'
import { createElement } from 'react'
import { AmllPlayer } from '@/amll/AmllPlayer.jsx'
import { useAmllBridge } from '@/amll/useAmllBridge.js'
import { PlayerStore } from '@/stores/player/index.js'
import { usePlatform } from '@/composables/usePlatform.js'
import { useWakeLock } from '@/composables/useWakeLock.js'
import TrackMenuSheet from '@/components/player/TrackMenuSheet.vue'
import PlayQueueSheet from '@/components/player/PlayQueueSheet.vue'

const { isDesktop } = usePlatform()

// 屏幕唤醒锁：PlayerView 已挂载（即 isPlayerViewOpen=true）且 isPlaying=true 时
// 阻止系统自动关闭屏幕 / 进入休眠。组件卸载时由 onScopeDispose 自动释放。
const { setActive: setWakeLockActive } = useWakeLock()
watch(
  () => PlayerStore.state.isPlaying,
  (playing) => setWakeLockActive(playing),
  { immediate: true }
)

const root = ref(null)
const amllContainer = ref(null)
let reactRoot = null

// 创建 Jotai store 并建立 Vue ↔ React 双向同步
const store = useAmllBridge()

function handleClose() {
  PlayerStore.closePlayerView()
}

function onKeydown(e) {
  if (e.key === 'Escape') {
    e.preventDefault()
    handleClose()
  } else if (e.key === ' ') {
    e.preventDefault()
    PlayerStore.togglePlay()
  }
}

/**
 * AMLL 容器点击事件委托。
 *
 * PrebuiltLyricPlayer 内部 horizontalLayout 的 bottomControls 第一个按钮
 * （PrebuiltToggleIconButton type="playlist"）未暴露 onClick prop，
 * 这里通过 DOM 选择器识别该按钮的点击，触发播放队列 Sheet。
 *
 * 选择器说明：
 *   .-DaA0W_bottomControls 是 AMLL 横向布局底部控件容器的 hashed class
 *   （对应源码 horizontal.module.css 中的 "bottomControls"）
 *   > button:first-child 选取第一个按钮（即 type="playlist"）
 *
 * 使用事件委托而非直接 addEventListener 的原因：
 *   - React 渲染是异步的，首次挂载后 DOM 可能稍后才出现
 *   - AMLL 在 horizontal/vertical 布局切换时会重新渲染 bottomControls
 *   - 事件委托天然支持 DOM 重新渲染，无需手动重绑
 */
function onAmllClick(e) {
  const target = e.target
  if (!(target instanceof Element)) return
  // closest() 兼容按钮内部 svg/icon 子元素点击
  const btn = target.closest('.-DaA0W_bottomControls > button:first-child')
  if (btn) {
    PlayerStore.openPlayQueue()
  }
}

onMounted(() => {
  // 挂载 React AMLL 播放器
  reactRoot = createRoot(amllContainer.value)
  reactRoot.render(createElement(AmllPlayer, { store }))

  // 标记过渡完成
  PlayerStore.setPlayerViewTransitioning(false)

  // 聚焦以接收键盘事件
  root.value?.focus()
})

onBeforeUnmount(() => {
  reactRoot?.unmount()
  reactRoot = null
})
</script>

<style scoped>
.player-view {
  position: fixed;
  inset: 0;
  z-index: 300;
  display: flex;
  flex-direction: column;
  background: #000;
  outline: none;
  overflow: hidden;
  animation: player-enter 0.4s ease-out;
}

.amll-container {
  flex: 1;
  min-height: 0;
  width: 100%;
  height: 100%;
  position: relative;
  overflow: hidden;
}

/* 顶部窗口拖动区域：仅桌面端
   - 高度 32px，覆盖整个宽度
   - -webkit-app-region:drag 让 Tauri 识别为窗口拖动区
   - pointer-events:none 让点击穿透到下方的 AMLL ControlThumb
   - Tauri 的 drag 识别独立于浏览器 pointer-events，两者可共存 */
.drag-region {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  height: 32px;
  -webkit-app-region: drag;
  pointer-events: none;
  z-index: 310;
}

@keyframes player-enter {
  from {
    opacity: 0;
    transform: translateY(100%);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
</style>
