<script setup>
/**
 * HomeHero — 首页 Hero Banner
 *
 * 职责：
 *  - 多专辑封面拼接背景（AlbumCollageBackground）
 *  - 问候语 + 播放全部/随机播放按钮
 *  - 统计数字（歌曲/专辑/歌手）
 *  - 纯展示组件，通过 emit 上报播放动作
 */
import AlbumCollageBackground from '../common/AlbumCollageBackground.vue';

defineProps({
  albums: { type: Array, default: () => [] },
  visible: { type: Boolean, default: false },
  greeting: { type: String, default: '' },
  stats: {
    type: Object,
    default: () => ({ artists: 0, albums: 0, tracks: 0 }),
  },
});

const emit = defineEmits(['play-all', 'shuffle']);
</script>

<template>
  <section class="hero-banner" v-if="visible">
    <!-- 多专辑封面拼接背景 -->
    <AlbumCollageBackground
      :albums="albums"
      :max-display="9"
      :animation-enabled="true"
      class="hero-collage-bg"
    />
    <div class="hero-content">
      <div class="hero-badge">
        <HIcon name="stars" />
        <span>今日推荐</span>
      </div>
      <h1 class="hero-title">{{ greeting }}，音乐爱好者</h1>
      <p class="hero-subtitle">探索你的音乐世界，发现无限可能</p>
      <div class="hero-actions">
        <button class="hero-btn primary" @click="emit('play-all')">
          <HIcon name="play-fill" />
          <span>播放全部</span>
        </button>
        <button class="hero-btn secondary" @click="emit('shuffle')">
          <HIcon name="shuffle" />
          <span>随机播放</span>
        </button>
      </div>
    </div>
    <div class="hero-stats">
      <div class="hero-stat">
        <span class="hero-stat-value">{{ stats.tracks }}</span>
        <span class="hero-stat-label">首歌曲</span>
      </div>
      <div class="hero-stat-divider"></div>
      <div class="hero-stat">
        <span class="hero-stat-value">{{ stats.albums }}</span>
        <span class="hero-stat-label">张专辑</span>
      </div>
      <div class="hero-stat-divider"></div>
      <div class="hero-stat">
        <span class="hero-stat-value">{{ stats.artists }}</span>
        <span class="hero-stat-label">位歌手</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* Hero Banner - 白/灰/黑三色调沉浸式头部 */
.hero-banner {
  position: relative;
  margin-bottom: 40px;
  padding: 40px 44px;
  border-radius: var(--radius-xl);
  overflow: hidden;
  min-height: 300px;
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  background: linear-gradient(135deg, #ffffff 0%, #f4f4f5 55%, #ececef 100%);
  border: 1px solid var(--border-light);
}

/* 多专辑封面背景 */
.hero-collage-bg {
  position: absolute;
  inset: 0;
  z-index: 0;
  opacity: 0.28;
}

.hero-content {
  position: relative;
  z-index: 1;
  max-width: 520px;
}

.hero-badge {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color);
  backdrop-filter: blur(8px);
  border-radius: 20px;
  color: var(--text-primary);
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 16px;
  will-change: backdrop-filter;
  contain: paint;
}

.hero-badge i {
  color: var(--text-primary);
}

.hero-title {
  font-size: 38px;
  font-weight: 800;
  color: var(--text-primary);
  margin: 0 0 12px 0;
  letter-spacing: -1px;
  line-height: 1.1;
}

.hero-subtitle {
  font-size: 15px;
  color: var(--text-secondary);
  margin: 0 0 24px 0;
  font-weight: 400;
}

.hero-actions {
  display: flex;
  gap: 12px;
}

.hero-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 24px;
  border-radius: 100px;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  transition: all var(--transition-fast);
  border: none;
}

.hero-btn.primary {
  background: var(--primary-color);
  color: var(--primary-contrast);
  box-shadow: var(--shadow-primary);
}

.hero-btn.primary:hover {
  transform: scale(1.04);
  box-shadow: var(--shadow-md);
}

.hero-btn.secondary {
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
}

.hero-btn.secondary:hover {
  background: var(--bg-hover);
}

.hero-btn i {
  font-size: 16px;
}

.hero-stats {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  gap: 20px;
  padding: 18px 22px;
  background: var(--bg-secondary);
  backdrop-filter: blur(12px);
  border-radius: var(--radius-lg);
  border: 1px solid var(--border-color);
  box-shadow: var(--shadow-sm);
  will-change: backdrop-filter;
  contain: layout paint;
}

.hero-stat {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
}

.hero-stat-value {
  font-size: 26px;
  font-weight: 800;
  color: var(--text-primary);
  line-height: 1;
  letter-spacing: -1px;
}

.hero-stat-label {
  font-size: 11px;
  color: var(--text-secondary);
  font-weight: 500;
}

.hero-stat-divider {
  width: 1px;
  height: 36px;
  background: var(--border-color);
}

/* 响应式 */
@media (max-width: 1024px) {
  .hero-banner {
    flex-direction: column;
    align-items: flex-start;
    gap: 32px;
    padding: 32px;
  }

  .hero-stats {
    width: 100%;
    justify-content: space-around;
  }
}

@media (max-width: 767px) {
  .hero-banner {
    padding: 28px;
    min-height: 280px;
  }

  .hero-title {
    font-size: 32px;
  }

  .hero-subtitle {
    font-size: 14px;
  }

  .hero-btn {
    padding: 12px 20px;
    font-size: 14px;
  }

  .hero-stats {
    padding: 16px 20px;
    gap: 16px;
  }

  .hero-stat-value {
    font-size: 22px;
  }
}

@media (max-width: 480px) {
  .hero-actions {
    flex-wrap: wrap;
  }

  .hero-btn span {
    display: none;
  }

  .hero-btn i {
    margin: 0;
    font-size: 20px;
  }
}

@media (prefers-color-scheme: dark) {
  .hero-banner {
    background: linear-gradient(135deg, #1b1b1d 0%, #161618 55%, #111113 100%);
    border-color: var(--border-light);
  }

  .hero-badge {
    background: rgba(255, 255, 255, 0.08);
  }

  .hero-btn.secondary {
    background: rgba(255, 255, 255, 0.08);
  }

  .hero-stats {
    background: rgba(255, 255, 255, 0.05);
  }
}
</style>