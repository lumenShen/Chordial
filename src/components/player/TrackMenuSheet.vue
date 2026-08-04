<script setup>
/**
 * TrackMenuSheet — AMLL MenuButton 触发的歌曲菜单 Sheet（薄壳容器）
 *
 * 由 PlayerView.vue 内挂载，监听 PlayerStore.state.ui.isTrackMenuOpen 显示。
 * 六个功能区已拆分为 track-menu/ 下的独立子组件，本组件仅负责：
 *   - BottomSheet 容器编排
 *   - 空态 / Toast 生命周期
 *   - 从 useTrackMenu 获取共享状态
 *
 * 触发链路：AMLL musicInfo MenuButton → onRequestOpenMenuAtom.onEmit
 *        → PlayerStore.openTrackMenu() → isTrackMenuOpen=true → 本组件 BottomSheet 显示
 */
import BottomSheet from '@/components/ui/BottomSheet.vue';
import { useTrackMenu } from './track-menu/useTrackMenu.js';
import TrackMenuHero from './track-menu/TrackMenuHero.vue';
import TrackMenuActions from './track-menu/TrackMenuActions.vue';
import TrackMenuNavigation from './track-menu/TrackMenuNavigation.vue';
import TrackMenuSourceMatch from './track-menu/TrackMenuSourceMatch.vue';
import TrackMenuSourceSwitch from './track-menu/TrackMenuSourceSwitch.vue';
import TrackMenuQuality from './track-menu/TrackMenuQuality.vue';
import TrackMenuOpen from './track-menu/TrackMenuOpen.vue';

const { visible, track, toast, onClose } = useTrackMenu();
</script>

<template>
  <BottomSheet
    v-model:visible="visible"
    title="歌曲菜单"
    :detents="['large']"
    default-detent="large"
    @close="onClose"
  >
    <div v-if="!track" class="tms-empty">未在播放任何曲目</div>

    <div v-else class="tms-root">
      <TrackMenuHero />
      <TrackMenuActions />
      <TrackMenuNavigation />
      <TrackMenuSourceMatch />
      <TrackMenuSourceSwitch />
      <TrackMenuQuality />
      <TrackMenuOpen />
    </div>

    <!-- ── Toast ────────────────────────────────────── -->
    <Transition name="tms-toast">
      <div v-if="toast" class="tms-toast" :class="`tms-toast--${toast.type}`">
        {{ toast.msg }}
      </div>
    </Transition>
  </BottomSheet>
</template>

<style scoped>
.tms-root {
  display: flex;
  flex-direction: column;
  gap: var(--space-4, 16px);
  padding-bottom: var(--space-3, 12px);
}

.tms-empty {
  padding: var(--space-6, 32px) var(--space-4, 16px);
  text-align: center;
  color: var(--text-tertiary, #999);
  font-size: var(--font-size-sm, 13px);
}

/* ── Toast ─────────────────────────────────────── */
.tms-toast {
  position: fixed;
  bottom: calc(80px + env(safe-area-inset-bottom, 0px));
  left: 50%;
  transform: translateX(-50%);
  padding: 8px 16px;
  border-radius: var(--radius-full, 999px);
  font-size: var(--font-size-sm, 13px);
  color: #fff;
  background: rgba(0, 0, 0, 0.78);
  z-index: 500;
  pointer-events: none;
  white-space: nowrap;
  max-width: 90vw;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tms-toast--success { background: rgba(40, 167, 69, 0.92); }
.tms-toast--error { background: rgba(220, 53, 69, 0.92); }
.tms-toast--info { background: rgba(0, 122, 255, 0.92); }

.tms-toast-enter-active,
.tms-toast-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}

.tms-toast-enter-from,
.tms-toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(8px);
}
</style>