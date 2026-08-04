<script setup>
defineProps({
  isLoading: { type: Boolean, default: false },
  stats: { type: Object, default: () => ({ folder_count: 0, indexed_files: 0 }) },
  isScanning: { type: Boolean, default: false },
  isAdding: { type: Boolean, default: false },
  formatFileCount: { type: Function, required: true },
});
defineEmits(['scan-all', 'add-source']);
</script>

<template>
  <div class="page-header">
    <div>
      <h1 class="page-title">音乐源管理</h1>
      <p class="page-subtitle">
        管理本地音乐文件夹与在线音乐来源 —
        <template v-if="!isLoading">{{ stats.folder_count }} 个本地文件夹，共 {{ formatFileCount(stats.indexed_files) }} 个音频文件</template>
        <template v-else>加载中…</template>
      </p>
    </div>
    <div class="page-actions">
      <button class="btn btn-secondary" @click="$emit('scan-all')" :disabled="isScanning">
        <svg v-if="isScanning" class="spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M21 12a9 9 0 11-6.219-8.56"/>
        </svg>
        <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M21.5 2v6h-6M2.5 22v-6h6M2 11.5a10 10 0 0118.8-4.3M22 12.5a10 10 0 01-18.8 4.3"/>
        </svg>
        {{ isScanning ? '扫描中…' : '重新扫描' }}
      </button>
      <button class="btn btn-primary" @click="$emit('add-source')" :disabled="isAdding">
        <svg v-if="isAdding" class="spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M21 12a9 9 0 11-6.219-8.56"/>
        </svg>
        <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="12" y1="5" x2="12" y2="19"/>
          <line x1="5" y1="12" x2="19" y2="12"/>
        </svg>
        {{ isAdding ? '添加中…' : '添加本地文件夹' }}
      </button>
    </div>
  </div>
</template>

<style scoped>
/* page-header / page-title / page-actions 使用全局统一样式 */

.page-actions .btn {
  display: flex;
  align-items: center;
  gap: 8px;
}

.page-actions .btn svg {
  width: 18px;
  height: 18px;
}
</style>