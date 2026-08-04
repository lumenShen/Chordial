<script setup>
/**
 * TracksEmptyState — 歌曲列表空态
 *
 * 职责：
 *  - 无歌曲时展示占位图标、引导文案与"添加音乐源"入口
 *  - 纯展示组件，样式复用全局 components.css 中的 .empty-state 系列类
 */
</script>

<template>
  <div class="empty-state">
    <svg class="empty-state-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
      <path d="M9 18V5l12-2v13"/>
      <circle cx="6" cy="18" r="3"/>
      <circle cx="18" cy="16" r="3"/>
    </svg>
    <h3 class="empty-state-title">暂无歌曲</h3>
    <p class="empty-state-desc">添加音乐源开始扫描歌曲</p>
    <router-link to="/music-sources" class="btn btn-primary" style="margin-top: 16px;">
      添加音乐源
    </router-link>
  </div>
</template>