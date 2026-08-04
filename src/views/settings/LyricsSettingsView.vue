<template>
  <div class="lyrics-view settings-view">
    <div class="section-header">
      <h2 class="section-title">歌词与播放器</h2>
      <p class="section-subtitle">AMLL 播放器外观、歌词效果、流体背景与显示项</p>
    </div>

    <LyricsEffectsSection />
    <LyricsFontSection />
    <FluidBackgroundSection />
    <DisplayItemsSection />
    <LayoutSection />
    <ResetSection />
  </div>
</template>

<script setup>
import LyricsEffectsSection from '@/components/settings/lyrics/LyricsEffectsSection.vue';
import LyricsFontSection from '@/components/settings/lyrics/LyricsFontSection.vue';
import FluidBackgroundSection from '@/components/settings/lyrics/FluidBackgroundSection.vue';
import DisplayItemsSection from '@/components/settings/lyrics/DisplayItemsSection.vue';
import LayoutSection from '@/components/settings/lyrics/LayoutSection.vue';
import ResetSection from '@/components/settings/lyrics/ResetSection.vue';
</script>