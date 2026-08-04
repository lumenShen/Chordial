<script setup>
defineProps({
  neteaseState: { type: Object, required: true },
  kugouState: { type: Object, required: true },
});
defineEmits(['go-netease', 'go-kugou']);
</script>

<template>
  <!-- 在线来源：网易云音乐 -->
  <div class="online-sources-section">
    <h2 class="section-subheading">
      <HIcon name="cloud-arrow-down" /> 在线音乐来源
    </h2>

    <div class="source-card card online-source-card" @click="$emit('go-netease')" role="button" tabindex="0"
      @keydown.enter.prevent="$emit('go-netease')" @keydown.space.prevent="$emit('go-netease')">
      <div class="source-info">
        <div class="source-icon netease-icon">
          <HIcon name="cloud-arrow-down" />
        </div>
        <div class="source-details">
          <h3 class="source-name">
            网易云音乐
            <span class="source-type-badge" :class="neteaseState.enabled ? 'enabled' : 'disabled'">
              {{ neteaseState.enabled ? '已启用' : '未启用' }}
            </span>
          </h3>
          <p class="source-path">
            <template v-if="neteaseState.loaded">
              {{ neteaseState.baseUrl || '（未配置 API 服务器）' }}
            </template>
            <template v-else>加载中…</template>
          </p>
          <div class="source-meta">
            <span class="meta-tag" :class="neteaseState.loggedIn ? 'ok' : 'warn'">
              <i class="bi" :class="neteaseState.loggedIn ? 'bi-check-circle-fill' : 'bi-exclamation-circle-fill'"></i>
              {{ neteaseState.loggedIn ? '已登录' : '未登录' }}
            </span>
            <span class="meta-tag">扫码登录 · 搜索歌单 · 在线下载</span>
          </div>
        </div>
      </div>
      <div class="source-actions">
        <button class="btn-icon" title="设置" aria-label="设置">
          <HIcon name="chevron-right" />
        </button>
      </div>
    </div>

    <!-- 酷狗音乐来源卡片 -->
    <div class="source-card card online-source-card" @click="$emit('go-kugou')" role="button" tabindex="0"
      @keydown.enter.prevent="$emit('go-kugou')" @keydown.space.prevent="$emit('go-kugou')">
      <div class="source-info">
        <div class="source-icon kugou-icon">
          <HIcon name="music-note-beamed" />
        </div>
        <div class="source-details">
          <h3 class="source-name">
            酷狗音乐
            <span class="source-type-badge" :class="kugouState.enabled ? 'enabled' : 'disabled'">
              {{ kugouState.enabled ? '已启用' : '未启用' }}
            </span>
          </h3>
          <p class="source-path">
            <template v-if="kugouState.loaded">
              {{ kugouState.baseUrl || '（未配置 API 服务器）' }}
            </template>
            <template v-else>加载中…</template>
          </p>
          <div class="source-meta">
            <span class="meta-tag" :class="kugouState.hasCookie ? 'ok' : 'warn'">
              <i class="bi" :class="kugouState.hasCookie ? 'bi-check-circle-fill' : 'bi-exclamation-circle-fill'"></i>
              {{ kugouState.hasCookie ? '已配置 Cookie' : '未配置 Cookie' }}
            </span>
            <span class="meta-tag">搜索歌曲 · 在线下载 · LRC 歌词</span>
          </div>
        </div>
      </div>
      <div class="source-actions">
        <button class="btn-icon" title="设置" aria-label="设置">
          <HIcon name="chevron-right" />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.source-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 20px;
}

.source-info {
  display: flex;
  align-items: center;
  gap: 16px;
  min-width: 0;
}

.source-icon {
  width: 48px;
  height: 48px;
  border-radius: var(--radius-md, 8px);
  background: var(--bg-tertiary, rgba(15, 15, 15, 0.05));
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--primary-color);
  flex-shrink: 0;
}

.source-icon svg {
  width: 24px;
  height: 24px;
}

.source-details {
  min-width: 0;
}

.source-name {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary, #333);
  margin: 0 0 4px 0;
}

.source-path {
  font-size: 13px;
  color: var(--text-secondary, #666);
  margin: 0 0 8px 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 500px;
}

.source-meta {
  font-size: 12px;
  color: var(--text-tertiary, #999);
}

.source-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.btn-icon {
  width: 36px;
  height: 36px;
  border: none;
  border-radius: var(--radius-md, 8px);
  background: transparent;
  color: var(--text-secondary, #666);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: all 0.2s ease;
}

.btn-icon:hover {
  background: var(--hover-bg, rgba(0, 0, 0, 0.05));
  color: var(--text-primary, #333);
}

.btn-icon svg {
  width: 18px;
  height: 18px;
}

/* ── 在线音乐来源区块 ─────────────────────────────────────────────── */
.online-sources-section {
  margin-top: 32px;
}

.section-subheading {
  font-size: 14px;
  font-weight: 700;
  color: var(--text-secondary, #666);
  margin: 0 0 16px;
  display: flex;
  align-items: center;
  gap: 6px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.online-source-card {
  cursor: pointer;
  transition: all 0.2s ease;
  border: 1px solid var(--border-color, #e8e8ed);
}

.online-source-card:hover {
  border-color: var(--primary-color);
  box-shadow: 0 4px 16px rgba(0, 122, 255, 0.1);
  transform: translateY(-1px);
}

.online-source-card:focus-visible {
  outline: 2px solid var(--primary-color);
  outline-offset: 2px;
}

.netease-icon {
  background: var(--primary-color);
  color: var(--primary-contrast);
}

.netease-icon i {
  font-size: 22px;
}

.kugou-icon {
  background: #4a4a4c;
  color: #fff;
}

.kugou-icon i {
  font-size: 22px;
}

.source-type-badge {
  display: inline-block;
  margin-left: 8px;
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 11px;
  font-weight: 600;
  vertical-align: middle;
}

.source-type-badge.enabled {
  background: rgba(40, 167, 69, 0.12);
  color: #28a745;
}

.source-type-badge.disabled {
  background: rgba(255, 193, 7, 0.15);
  color: #b8860b;
}

.meta-tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  margin-right: 8px;
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 11px;
  font-weight: 500;
  background: var(--bg-tertiary, rgba(15, 15, 15, 0.05));
  color: var(--text-tertiary, #999);
}

.meta-tag.ok {
  background: rgba(40, 167, 69, 0.1);
  color: #28a745;
}

.meta-tag.warn {
  background: rgba(255, 193, 7, 0.12);
  color: #b8860b;
}

/* ── Responsive ────────────────────────────────────────────────────── */
@media (max-width: 767px) {
  .source-card {
    flex-direction: column;
    align-items: flex-start;
    gap: 16px;
  }

  .source-path {
    max-width: 280px;
  }

  .source-actions {
    width: 100%;
    justify-content: flex-end;
  }

  /* 移动端点击区域 ≥ 40px */
  .btn-icon {
    width: 40px;
    height: 40px;
  }
}

@media (prefers-color-scheme: dark) {
  .source-name {
    color: var(--text-primary, #f6f6f6);
  }
}
</style>