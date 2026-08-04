<template>
  <div class="deployer-view settings-view">
    <div class="section-header">
      <h2 class="section-title">音乐源服务部署</h2>
      <p class="section-subtitle">
        一键部署网易云 / 酷狗音乐 API 服务到本地，并设置程序启动时自动运行
      </p>
    </div>

    <!-- 工具链检测 -->
    <ToolchainCheck
      :toolchain="toolchain"
      :loading="toolchainLoading"
      :all-tools-available="allToolsAvailable"
      @refresh="loadToolchain"
    />

    <!-- 来源列表 -->
    <SourceCard
      v-for="src in sources"
      :key="src.spec.id"
      :src="src"
      :busy="busy"
      :all-tools-available="allToolsAvailable"
      :expanded-logs="expandedLogs"
      :pending-remove="pendingRemove"
      :status-label="statusLabel"
      :status-class="statusClass"
      :source-icon="sourceIcon"
      :effective-port="effectivePort"
      @deploy-and-start="deployAndStart(src)"
      @start="startSource(src)"
      @stop="stopSource(src)"
      @pull="pullSource(src)"
      @install="installSource(src)"
      @remove="confirmRemove(src)"
      @port-change="onPortChange(src, $event)"
      @toggle-enabled="onToggleEnabled(src, $event)"
      @toggle-auto-start="onToggleAutoStart(src, $event)"
      @toggle-auto-base-url="onToggleAutoBaseUrl(src, $event)"
      @toggle-logs="toggleLogs(src.spec.id)"
    />

    <!-- Toast -->
    <ToastNotification :toast="toast" :toast-icon="toastIcon" />
  </div>
</template>

<script setup>
import ToolchainCheck from '@/components/settings/system-deploy/ToolchainCheck.vue';
import SourceCard from '@/components/settings/system-deploy/SourceCard.vue';
import ToastNotification from '@/components/settings/system-deploy/ToastNotification.vue';
import { useSourceDeploy } from '@/composables/useSourceDeploy.js';

const {
  sources,
  toolchain,
  toolchainLoading,
  busy,
  expandedLogs,
  pendingRemove,
  toast,
  toastIcon,
  allToolsAvailable,
  loadToolchain,
  deployAndStart,
  startSource,
  stopSource,
  pullSource,
  installSource,
  confirmRemove,
  onPortChange,
  onToggleEnabled,
  onToggleAutoStart,
  onToggleAutoBaseUrl,
  toggleLogs,
  effectivePort,
  statusLabel,
  statusClass,
  sourceIcon,
} = useSourceDeploy();
</script>

<style scoped>
.deployer-view {
  position: relative;
  padding-bottom: 80px;
}
</style>