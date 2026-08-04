<template>
  <div class="settings-section">
    <h3 class="block-title">
      <HIcon name="ui-checks" />
      显示项
    </h3>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">歌曲名称</label>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.showMusicName" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">艺人</label>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.showMusicArtists" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">专辑名</label>
        <span class="setting-desc">三项全开可能影响布局美观</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.showMusicAlbum" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">音量控制条</label>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.showVolumeControl" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">底部控制按钮</label>
        <span class="setting-desc">横屏右下角 / 竖屏播放键下方</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.showBottomControl" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">进度条显示剩余时间</label>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.showRemainingTime" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">隐藏歌词视图</label>
        <span class="setting-desc">即使有歌词数据也不显示歌词</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.hideLyricView" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>
  </div>
</template>

<script setup>
import { useLyricsSettings } from '@/composables/useLyricsSettings.js';

const { settings } = useLyricsSettings();
</script>