<script setup>
/**
 * HomeEmptyState — 首页空状态
 *
 * 职责：
 *  - 无任何数据时展示引导文案与"添加音乐源"入口
 *  - 纯展示组件
 */
</script>

<template>
  <div class="empty-state">
    <div class="empty-icon">
      <HIcon name="music-note-beamed" />
    </div>
    <h3 class="empty-title">开始你的音乐之旅</h3>
    <p class="empty-desc">添加音乐源，发现属于你的音乐世界</p>
    <router-link to="/music-sources" class="empty-action">
      <HIcon name="plus-lg" />
      添加音乐源
    </router-link>
  </div>
</template>

<style scoped>
/* 空状态 */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 100px 40px;
  text-align: center;
}

.empty-icon {
  width: 80px;
  height: 80px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--primary-light);
  color: var(--primary-color);
  border-radius: 50%;
  font-size: 36px;
  margin-bottom: 24px;
}

.empty-title {
  font-size: 24px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0 0 8px 0;
}

.empty-desc {
  font-size: 15px;
  color: var(--text-secondary);
  margin: 0 0 28px 0;
}

.empty-action {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 14px 28px;
  background: var(--primary-color);
  color: white;
  border-radius: 100px;
  font-size: 15px;
  font-weight: 600;
  text-decoration: none;
  transition: all var(--transition-fast);
  box-shadow: var(--shadow-primary);
}

.empty-action:hover {
  background: var(--primary-hover);
  transform: translateY(-2px);
  box-shadow: 0 6px 20px rgba(0, 122, 255, 0.35);
}
</style>