<script setup>
/**
 * ArtistHero — 歌手详情页顶部信息区
 *
 * 职责：
 *  - 封面（圆形）+ 名称 + 元信息 + 简介 + 操作按钮
 *  - 封面通过 useCoverImage 加载（ ResourceManager 缓存）
 *  - 入场动画：cover scaleIn(bouncy) → info 子元素 stagger fadeInUp
 *
 * 动画管线：
 *  - 进入：onMounted + artist.id 变化时重播（切换歌手场景）
 *  - 完成后清除内联 transform，恢复 CSS :hover 的 translateY 上浮
 */
import { useTemplateRef, watch, nextTick, onMounted, toRef } from 'vue';
import { useCoverImage } from '@/composables/useCoverImage';
import { useAnime } from '@/composables/useAnime.js';

const props = defineProps({
  artist: { type: Object, default: null },
  isFavorite: { type: Boolean, default: false },
});

const emit = defineEmits(['play-all', 'toggle-favorite']);

const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

// toRef 将 props.artist 转为 ref，供 useCoverImage 监听
// 父级已用 shallowRef 保证 Artist 实例不被深代理
// useCoverImage 内部已 watch item.value?.id 自动 reload，无需在此重复 watch
const artistRef = toRef(props, 'artist');
const { coverUrl } = useCoverImage(artistRef, 'large');

// --- 入场动画 ---
// cover 先 scaleIn（bouncy 弹性），info 子元素 stagger fadeInUp
// delay 150ms 等 cover 启动后再启动，形成视觉层次
function playEnter() {
  run(({ animate, stagger, presets }) => {
    const root = rootRef.value;
    if (!root) return;

    animate('.artist-cover-wrapper', {
      ...presets.scaleIn,
    });

    animate('.artist-info > *', {
      ...presets.fadeInUp,
      delay: stagger(80, { start: 150 }),
      onComplete: () => {
        // 清除内联 transform，恢复 CSS :hover 的上浮效果
        rootRef.value?.querySelectorAll('.artist-info > *').forEach((el) => {
          el.style.transform = '';
        });
      },
    });
  });
}

onMounted(playEnter);
// artist.id 变化（切换歌手）时重新播放
watch(() => props.artist?.id, () => nextTick(playEnter), { flush: 'post' });

const trackCount = () => props.artist?.trackIds?.length ?? 0;
const albumCount = () => props.artist?.albumIds?.length ?? 0;
</script>

<template>
  <div ref="root" class="artist-hero">
    <div class="artist-cover-wrapper">
      <img
        v-if="coverUrl"
        :src="coverUrl"
        class="artist-cover"
        :alt="artist.name"
      />
      <div v-else class="artist-cover-placeholder">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
          <circle cx="12" cy="7" r="4" />
        </svg>
      </div>
    </div>

    <div class="artist-info">
      <span class="artist-label">歌手</span>
      <h1 class="artist-name">{{ artist.name }}</h1>
      <div class="artist-meta">
        <span>{{ trackCount() }} 首歌曲</span>
        <span class="separator">·</span>
        <span>{{ albumCount() }} 张专辑</span>
      </div>
      <p v-if="artist.bio" class="artist-bio">{{ artist.bio }}</p>

      <div class="artist-actions">
        <button class="btn btn-primary btn-play" @click="emit('play-all')">
          <svg viewBox="0 0 24 24" fill="currentColor">
            <path d="M8 5v14l11-7z" />
          </svg>
          播放全部
        </button>
        <button
          class="btn btn-secondary"
          :class="{ 'is-favorite': isFavorite }"
          @click="emit('toggle-favorite')"
        >
          <svg
            viewBox="0 0 24 24"
            :fill="isFavorite ? 'currentColor' : 'none'"
            stroke="currentColor"
            stroke-width="2"
          >
            <path d="M4.318 6.318a4.5 4.5 0 000 6.364L12 20.364l7.682-7.682a4.5 4.5 0 00-6.364-6.364L12 7.636l-1.318-1.318a4.5 4.5 0 00-6.364 0z" />
          </svg>
          {{ isFavorite ? '已收藏' : '收藏' }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.artist-hero {
  display: flex;
  gap: 28px;
  margin-bottom: 28px;
}

.artist-cover-wrapper {
  flex-shrink: 0;
  width: 200px;
  height: 200px;
  border-radius: 50%;
  overflow: hidden;
  box-shadow: var(--shadow-md);
  /* contain 隔离重绘范围，降低 hover 时 GPU 合成开销 */
  contain: layout style paint;
}

.artist-cover {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.artist-cover-placeholder {
  width: 100%;
  height: 100%;
  background: var(--bg-tertiary);
  display: flex;
  align-items: center;
  justify-content: center;
}

.artist-cover-placeholder svg {
  width: 80px;
  height: 80px;
  color: var(--text-tertiary);
}

.artist-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: center;
  min-width: 0; /* 允许子元素 ellipsis */
}

.artist-label {
  font-size: var(--font-size-sm);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 1px;
  color: var(--text-secondary);
  margin-bottom: var(--space-2);
}

.artist-name {
  font-size: 32px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0 0 var(--space-4) 0;
  line-height: 1.2;
  letter-spacing: -0.5px;
}

.artist-meta {
  font-size: var(--font-size-base);
  color: var(--text-secondary);
  margin-bottom: var(--space-4);
}

.artist-meta .separator {
  margin: 0 var(--space-2);
}

.artist-bio {
  font-size: var(--font-size-base);
  color: var(--text-secondary);
  line-height: 1.6;
  margin-bottom: var(--space-6);
  max-width: 600px;
}

.artist-actions {
  display: flex;
  gap: var(--space-3);
}

.btn-play {
  padding: 12px 32px;
}

.btn-play svg {
  width: 20px;
  height: 20px;
}

.btn-secondary.is-favorite {
  color: var(--primary-color);
}

@media (max-width: 767px) {
  .artist-hero {
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: var(--space-5);
    margin-bottom: var(--space-6);
  }

  .artist-cover-wrapper {
    width: 160px;
    height: 160px;
  }

  .artist-name {
    font-size: 24px;
  }

  .artist-actions {
    justify-content: center;
  }
}
</style>
