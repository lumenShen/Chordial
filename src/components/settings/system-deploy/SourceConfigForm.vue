<template>
  <div>
    <!-- 配置 -->
    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">监听端口</label>
        <span class="setting-desc">0 = 使用默认端口（{{ src.spec.default_port }}）</span>
      </div>
      <div class="setting-control">
        <input
          type="number"
          min="0"
          max="65535"
          :value="src.config.port"
          @change="onChangePort($event)"
          class="number-input"
          :disabled="src.runtime.status === 'running'"
        />
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">启用快速部署</label>
        <span class="setting-desc">关闭后忽略「自动运行」标志</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input
            type="checkbox"
            :checked="src.config.enabled"
            @change="onToggleEnabled($event)"
          />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">程序启动时自动运行</label>
        <span class="setting-desc">应用启动时自动拉起此服务</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input
            type="checkbox"
            :checked="src.config.auto_start"
            :disabled="!src.config.enabled"
            @change="onToggleAutoStart($event)"
          />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">启动后自动配置 base_url</label>
        <span class="setting-desc">把对应来源（{{ src.spec.id }}）的 base_url 设为 {{ src.runtime.base_url }}</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input
            type="checkbox"
            :checked="src.config.auto_configure_base_url"
            @change="onToggleAutoBaseUrl($event)"
          />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>
  </div>
</template>

<script setup>
const props = defineProps({
  src: { type: Object, required: true },
});

const emit = defineEmits(['port-change', 'toggle-enabled', 'toggle-auto-start', 'toggle-auto-base-url']);
function onChangePort(ev) {
  emit('port-change', ev);
}
function onToggleEnabled(ev) {
  emit('toggle-enabled', ev);
}
function onToggleAutoStart(ev) {
  emit('toggle-auto-start', ev);
}
function onToggleAutoBaseUrl(ev) {
  emit('toggle-auto-base-url', ev);
}
</script>

<style scoped>
/* 设置项（本页使用更紧凑的行距与更浅的分隔线） */
.setting-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 0;
  border-bottom: 1px solid var(--border-lightest, rgba(0, 0, 0, 0.04));
}

.setting-item:last-of-type {
  border-bottom: none;
}

.setting-info {
  flex: 1;
  min-width: 0;
  margin-right: 16px;
}

.setting-label {
  display: block;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary, #333);
  margin-bottom: 2px;
}

.number-input {
  width: 100px;
  padding: 6px 10px;
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 8px;
  font-size: 13px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
}

.number-input:disabled {
  background: var(--bg-tertiary, rgba(0, 0, 0, 0.04));
  cursor: not-allowed;
}

/* Toggle（本页为 24px 高加大尺寸变体） */
.toggle {
  position: relative;
  display: inline-block;
  width: 44px;
  height: 24px;
}

.toggle input {
  opacity: 0;
  width: 0;
  height: 0;
}

.toggle-slider {
  position: absolute;
  cursor: pointer;
  inset: 0;
  background: var(--bg-tertiary, #ccc);
  border-radius: 24px;
  transition: 0.2s;
}

.toggle-slider::before {
  content: '';
  position: absolute;
  width: 18px;
  height: 18px;
  left: 3px;
  top: 3px;
  background: white;
  border-radius: 50%;
  transition: 0.2s;
}

.toggle input:checked + .toggle-slider {
  background: var(--primary-color);
}

.toggle input:checked + .toggle-slider::before {
  transform: translateX(20px);
}

.toggle input:disabled + .toggle-slider {
  opacity: 0.4;
  cursor: not-allowed;
}
</style>