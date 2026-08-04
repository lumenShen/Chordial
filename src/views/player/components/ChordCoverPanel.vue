<template>
  <div class="chord-cover-panel">
    <div class="cover-bg" v-if="coverUrl">
      <img :src="coverUrl" alt="" />
    </div>
    <div class="cover-main" :class="{ paused: musicPaused }">
      <img v-if="coverUrl" :src="coverUrl" alt="专辑封面" class="cover-img" />
      <div v-else class="cover-placeholder">
        <HugeiconsIcon :icon="MusicNote01Icon" :size="56" />
      </div>
    </div>
  </div>
</template>

<script setup>
/**
 * ChordCoverPanel — 竖屏布局的封面面板。
 * 封面 + 模糊背景，用于隐藏歌词时的沉浸式封面展示。
 * 暂停时封面轻微缩小（对齐 AMLL Cover 的 pauseShrink 动画）。
 */
import { HugeiconsIcon } from '@hugeicons/vue'
import { MusicNote01Icon } from '@hugeicons/core-free-icons'

defineProps({
  coverUrl: { type: String, default: '' },
  musicPaused: { type: Boolean, default: false },
})
</script>

<style scoped>
.chord-cover-panel {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
}

.cover-bg {
  position: absolute;
  inset: 0;
  overflow: hidden;
}

.cover-bg img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  filter: blur(30px) brightness(0.5) saturate(1.2);
  transform: scale(1.15);
}

.cover-main {
  position: relative;
  width: min(70vw, 70vh, 380px);
  aspect-ratio: 1;
  border-radius: 16px;
  overflow: hidden;
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
  transition: transform 0.6s cubic-bezier(0.3, 0.2, 0.2, 1.4);
}

/* 暂停时封面轻微缩小（对齐 AMLL pauseShrink） */
.cover-main.paused {
  transform: scale(0.92);
}

.cover-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}

.cover-placeholder {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(255, 255, 255, 0.08);
  color: rgba(255, 255, 255, 0.4);
}
</style>