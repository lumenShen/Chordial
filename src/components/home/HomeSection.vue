<script setup>
/**
 * HomeSection — 通用区块容器（标题 + 查看全部链接 + 插槽）
 *
 * 职责：
 *  - 统一区块标题与"查看全部"跳转
 *  - 通过插槽承载内容（横向滚动 / 网格 / 列表 / 歌手网格）
 *  - compact 属性应用紧凑间距（最近添加区块）
 */
defineProps({
  title: { type: String, required: true },
  to: { type: String, required: true },
  compact: { type: Boolean, default: false },
});
</script>

<template>
  <section class="section" :class="{ 'section-compact': compact }">
    <div class="section-header">
      <h2 class="section-title">{{ title }}</h2>
      <router-link :to="to" class="section-link">
        查看全部
        <HIcon name="arrow-right" />
      </router-link>
    </div>
    <slot />
  </section>
</template>

<style scoped>
/* 区块样式 */
.section {
  margin-bottom: 48px;
}

.section-compact {
  margin-bottom: 40px;
}

.section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 20px;
}

.section-title {
  font-size: 24px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0;
  letter-spacing: -0.5px;
}

.section-link {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 14px;
  font-weight: 600;
  color: var(--primary-color);
  text-decoration: none;
  padding: 8px 16px;
  border-radius: 100px;
  transition: all var(--transition-fast);
  background: transparent;
}

.section-link:hover {
  background: var(--primary-light);
}

.section-link i {
  font-size: 14px;
}

/* 响应式 */
@media (max-width: 767px) {
  .section-title {
    font-size: 20px;
  }
}
</style>