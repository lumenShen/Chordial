<template>
  <div class="maintenance-view settings-view">
    <div class="section-header">
      <h2 class="section-title">数据维护</h2>
      <p class="section-subtitle">查看应用数据占用并执行清理操作</p>
    </div>

    <LibraryStatsSection
      :stats="stats"
      :loading="loading"
      @refresh="loadStats"
    />

    <RepairSection
      :pending-action="pendingAction"
      :busy="busy"
      @confirm-action="confirmAction"
    />

    <CleanupSection
      :pending-action="pendingAction"
      :busy="busy"
      @confirm-action="confirmAction"
    />

    <MaintenanceToast :toast="toast" :toast-icon="toastIcon" />
  </div>
</template>

<script setup>
import { useMaintenanceSettings } from '@/composables/useMaintenanceSettings';
import LibraryStatsSection from '@/components/settings/maintenance/LibraryStatsSection.vue';
import RepairSection from '@/components/settings/maintenance/RepairSection.vue';
import CleanupSection from '@/components/settings/maintenance/CleanupSection.vue';
import MaintenanceToast from '@/components/settings/maintenance/MaintenanceToast.vue';

const {
  stats,
  loading,
  busy,
  pendingAction,
  toast,
  toastIcon,
  loadStats,
  confirmAction,
} = useMaintenanceSettings();
</script>

<style scoped>
.maintenance-view {
  position: relative;
  padding-bottom: 80px;
}
</style>