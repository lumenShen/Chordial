<template>
  <div class="p2p-view settings-view">
    <div class="section-header">
      <h2 class="section-title">P2P 资源共享</h2>
      <p class="section-subtitle">在局域网内与其他 Chordial 实例共享音乐库</p>
    </div>

    <P2pServiceControl />

    <P2pMatchCodeQr />

    <!-- 局域网扫描结果 -->
    <div class="settings-section">
      <h3 class="block-title">
        <HIcon name="broadcast" />局域网扫描结果
        <button class="action-btn ghost refresh-btn" @click="refreshStatus" :disabled="!p2pEnabled">
          <HIcon name="arrow-clockwise" />
        </button>
      </h3>
      <P2pPeerList mode="discovered" empty-text="未发现局域网设备，请确认对方已开启广播" />
    </div>

    <!-- 可信设备 -->
    <div class="settings-section">
      <h3 class="block-title"><HIcon name="shield-check" />可信设备</h3>
      <p class="block-desc">匹配同意过的设备会自动重连，无需再次确认</p>
      <P2pPeerList mode="trusted" empty-text="暂无可信设备" />
    </div>

    <!-- 已连接 peer -->
    <div class="settings-section">
      <h3 class="block-title"><HIcon name="people" />已连接</h3>
      <P2pPeerList mode="connected" empty-text="暂无已连接的对端" />
    </div>

    <P2pManualMatch />

    <P2pScannerDialog />
  </div>
</template>

<script setup>
import { useP2pSettings } from '@/composables/useP2pSettings.js';
import P2pServiceControl from '@/components/settings/p2p/P2pServiceControl.vue';
import P2pMatchCodeQr from '@/components/settings/p2p/P2pMatchCodeQr.vue';
import P2pPeerList from '@/components/settings/p2p/P2pPeerList.vue';
import P2pManualMatch from '@/components/settings/p2p/P2pManualMatch.vue';
import P2pScannerDialog from '@/components/settings/p2p/P2pScannerDialog.vue';

// 调用一次以注册状态轮询定时器与生命周期（仅在父组件 setup 注册一次）
const { p2pEnabled, refreshStatus } = useP2pSettings();
</script>

<style scoped>
.block-desc {
  font-size: 12px;
  color: var(--text-secondary, #666);
  margin: -10px 0 14px;
}

.refresh-btn {
  margin-left: auto;
  padding: 4px 8px;
}

/* 本页 action-btn 为实心灰底变体，与全局线框版不同 */
.action-btn {
  padding: 7px 14px;
  border: 1px solid transparent;
  border-radius: 8px;
  background: var(--bg-hover, #eaeaea);
  color: var(--text-primary, #333);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.2s ease;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.action-btn:hover:not(:disabled) {
  background: var(--bg-active, #dcdcdc);
}

.action-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.action-btn.ghost {
  background: transparent;
  border-color: var(--border-color, #e8e8e8);
}
</style>