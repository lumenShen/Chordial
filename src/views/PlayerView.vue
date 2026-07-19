<template>
  <Teleport to="body">
    <div ref="root" class="player-view" tabindex="0" @keydown="onKeydown">
      <!-- AMLL React 播放器挂载点 -->
      <div ref="amllContainer" class="amll-container" />
      <!-- 顶部窗口拖动区域（仅桌面端）
           pointer-events:none 让点击穿透到 AMLL 的 ControlThumb；
           -webkit-app-region:drag 由 Tauri native 层识别用于拖动窗口 -->
      <div v-if="isDesktop" class="drag-region" />
    </div>
  </Teleport>
</template>

<script setup>
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { createRoot } from 'react-dom/client'
import { createElement } from 'react'
import { AmllPlayer } from '@/amll/AmllPlayer.jsx'
import { useAmllBridge } from '@/amll/useAmllBridge.js'
import { PlayerStore } from '@/stores/player.js'
import { usePlatform } from '@/composables/usePlatform.js'

const { isDesktop } = usePlatform()

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
