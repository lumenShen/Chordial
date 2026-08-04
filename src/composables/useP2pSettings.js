/**
 * P2P 设置页共享 composable（模块级单例）。
 *
 * 承载 P2pSettingsView 及其子组件（P2pServiceControl / P2pMatchCodeQr /
 * P2pPeerList / P2pManualMatch / P2pScannerDialog）的全部状态与动作逻辑：
 * - 服务状态（启用 / 广播 / 权限 / 监听地址）
 * - 匹配码轮换倒计时
 * - 二维码生成与扫描
 * - 局域网发现 / 可信设备 / 已连接 peer 列表
 * - 手动匹配
 * - 状态轮询定时器与生命周期清理
 *
 * 由于是模块级单例，页面与子组件调用 useP2pSettings() 拿到的都是同一份引用，
 * 生命周期（onMounted/onUnmounted）仅在首次调用（父组件 setup）时注册一次。
 *
 * @example
 * import { useP2pSettings } from '@/composables/useP2pSettings';
 * const { p2pEnabled, onToggleP2p, refreshStatus } = useP2pSettings();
 */
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue';
import QRCode from 'qrcode';
import jsQR from 'jsqr';
import { p2pApi } from '@/api/p2p.js';
import { useP2pEvents } from '@/composables/useP2pEvents.js';

let state = null;

function createState() {
  // 与 useP2pEvents 单例对齐：启动 p2p-event 监听（toasts/pendingRequests 由全局桥接使用）
  useP2pEvents();

  const p2pEnabled = ref(false);
  const p2pBroadcast = ref(false);
  const p2pPermission = ref('readonly');
  const p2pStatus = ref({
    listening: false,
    listen_addr: '',
    match_code: '',
    permission: 'readonly',
    broadcast_enabled: false,
    peers: [],
    discovered: [],
    trusted_devices: [],
    instance_id: '',
    match_code_rotates_at: 0,
  });
  const remoteAddr = ref('');
  const remoteCode = ref('');

  // 二维码生成
  const qrDataUrl = ref('');
  // 二维码扫描
  const showScanner = ref(false);
  const videoEl = ref(null);
  const scannerError = ref('');
  let scanStream = null;
  let scanRaf = null;
  let scanCanvas = null;

  // 轮换倒计时
  const nowSec = ref(Math.floor(Date.now() / 1000));
  const rotateCountdown = computed(() =>
    p2pStatus.value.match_code_rotates_at
      ? Math.max(0, p2pStatus.value.match_code_rotates_at - nowSec.value)
      : 0,
  );

  const discoveredPeers = computed(() => p2pStatus.value.discovered || []);
  const trustedDevices = computed(() => p2pStatus.value.trusted_devices || []);
  const connectedPeers = computed(() => p2pStatus.value.peers || []);

  const trustedIds = computed(() => new Set(trustedDevices.value.map((d) => d.instance_id)));
  const connectedNames = computed(() => new Set(connectedPeers.value.map((p) => p.name)));

  function isTrusted(peer) {
    return !!(peer.instance_id && trustedIds.value.has(peer.instance_id));
  }
  function isConnected(peer) {
    return connectedNames.value.has(peer.name);
  }
  function peerKey(peer) {
    return peer.instance_id || `${peer.addr}:${peer.port}`;
  }

  function formatTime(sec) {
    if (!sec) return '';
    try {
      const d = new Date(sec * 1000);
      return `${d.getMonth() + 1}/${d.getDate()}`;
    } catch {
      return '';
    }
  }

  // ── 状态刷新 ────────────────────────────────
  const refreshStatus = async () => {
    try {
      p2pStatus.value = await p2pApi.status();
      p2pEnabled.value = p2pStatus.value.listening;
      p2pBroadcast.value = p2pStatus.value.broadcast_enabled;
      p2pPermission.value = p2pStatus.value.permission || 'readonly';
    } catch (e) {
      console.error('P2P 状态查询失败:', e);
    }
  };

  // ── 控制 ────────────────────────────────────
  const onToggleP2p = async () => {
    try {
      if (p2pEnabled.value) {
        await p2pApi.startServer({ broadcast: p2pBroadcast.value, permission: p2pPermission.value });
      } else {
        await p2pApi.stopServer();
        p2pBroadcast.value = false;
      }
      await refreshStatus();
    } catch (e) {
      console.error('P2P 切换失败:', e);
      p2pEnabled.value = !p2pEnabled.value;
    }
  };

  const onToggleBroadcast = async () => {
    try {
      await p2pApi.setBroadcast(p2pBroadcast.value);
      await refreshStatus();
    } catch (e) {
      console.error('切换广播失败:', e);
      p2pBroadcast.value = !p2pBroadcast.value;
    }
  };

  const onPermissionChange = async () => {
    try {
      await p2pApi.setPermission(p2pPermission.value);
    } catch (e) {
      console.error('设置权限失败:', e);
    }
  };

  const onRegenerateCode = async () => {
    try {
      await p2pApi.regenerateMatchCode();
      await refreshStatus();
    } catch (e) {
      console.error('重新生成匹配码失败:', e);
    }
  };

  const onRequestMatch = async () => {
    try {
      await p2pApi.requestMatch(remoteAddr.value, remoteCode.value);
      remoteAddr.value = '';
      remoteCode.value = '';
      await refreshStatus();
    } catch (e) {
      console.error('发起匹配失败:', e);
      alert('匹配失败: ' + e);
    }
  };

  const onConnectDiscovered = async (peer) => {
    // 信任设备会自动连接，这里仅处理未信任设备
    const addr = `${peer.addr}:${peer.port}`;
    try {
      await p2pApi.requestMatch(addr, peer.match_code);
      await refreshStatus();
    } catch (e) {
      console.error('连接发现设备失败:', e);
      alert('连接失败: ' + e);
    }
  };

  const onDisconnect = async (peerId) => {
    try {
      await p2pApi.disconnectPeer(peerId);
      await refreshStatus();
    } catch (e) {
      console.error('断开失败:', e);
    }
  };

  const onRemoveTrusted = async (instanceId) => {
    try {
      await p2pApi.removeTrusted(instanceId);
      await refreshStatus();
    } catch (e) {
      console.error('移除可信设备失败:', e);
    }
  };

  // ── 二维码生成 ──────────────────────────────
  const regenerateQr = async () => {
    if (!p2pEnabled.value) {
      qrDataUrl.value = '';
      return;
    }
    try {
      const payload = await p2pApi.getMatchPayload();
      const text = JSON.stringify({
        v: 1,
        iid: payload.instance_id,
        name: payload.name,
        addr: payload.listen_addr,
        port: payload.port,
        code: payload.match_code,
      });
      qrDataUrl.value = await QRCode.toDataURL(text, { width: 240, margin: 2 });
    } catch (e) {
      console.error('二维码生成失败:', e);
      qrDataUrl.value = '';
    }
  };

  // 监听 match_code 变化时重新生成二维码
  watch(() => p2pStatus.value.match_code, () => {
    regenerateQr();
  });
  watch(p2pEnabled, () => {
    regenerateQr();
  });

  // ── 二维码扫描 ──────────────────────────────
  const startScanner = async () => {
    scannerError.value = '';
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
      scannerError.value = '当前环境不支持摄像头';
      return;
    }
    try {
      scanStream = await navigator.mediaDevices.getUserMedia({
        video: { facingMode: 'environment' },
      });
      await nextTick();
      if (videoEl.value) {
        videoEl.value.srcObject = scanStream;
        videoEl.value.play();
        scanLoop();
      }
    } catch (e) {
      scannerError.value = `摄像头启动失败: ${e?.message || e}`;
    }
  };

  const scanLoop = () => {
    if (!showScanner.value || !videoEl.value) return;
    const video = videoEl.value;
    if (video.readyState === video.HAVE_ENOUGH_DATA) {
      const w = video.videoWidth;
      const h = video.videoHeight;
      if (!scanCanvas) scanCanvas = document.createElement('canvas');
      scanCanvas.width = w;
      scanCanvas.height = h;
      const ctx = scanCanvas.getContext('2d', { willReadFrequently: true });
      ctx.drawImage(video, 0, 0, w, h);
      const imgData = ctx.getImageData(0, 0, w, h);
      const result = jsQR(imgData.data, imgData.width, imgData.height, {
        inversionAttempts: 'attemptOnce',
      });
      if (result && result.data) {
        handleScannedPayload(result.data);
        return;
      }
    }
    scanRaf = requestAnimationFrame(scanLoop);
  };

  const handleScannedPayload = async (raw) => {
    let payload;
    try {
      payload = JSON.parse(raw);
    } catch {
      // 兼容纯文本 "addr:port|code"
      const parts = raw.split(/[|,]/);
      if (parts.length >= 2) {
        payload = { addr: parts[0], code: parts[1] };
      } else {
        scannerError.value = '无法识别的二维码内容';
        scanRaf = requestAnimationFrame(scanLoop);
        return;
      }
    }
    const addr = payload.addr || (payload.iid ? null : null);
    const code = payload.code || payload.match_code;
    if (!addr || !code) {
      scannerError.value = '二维码缺少地址或匹配码';
      scanRaf = requestAnimationFrame(scanLoop);
      return;
    }
    closeScanner();
    try {
      await p2pApi.requestMatch(addr, code);
      await refreshStatus();
    } catch (e) {
      console.error('二维码匹配失败:', e);
      alert('匹配失败: ' + e);
    }
  };

  const closeScanner = () => {
    showScanner.value = false;
    if (scanRaf) {
      cancelAnimationFrame(scanRaf);
      scanRaf = null;
    }
    if (scanStream) {
      scanStream.getTracks().forEach((t) => t.stop());
      scanStream = null;
    }
    if (videoEl.value) videoEl.value.srcObject = null;
    scannerError.value = '';
  };

  const openScanner = () => {
    showScanner.value = true;
  };

  watch(showScanner, (v) => {
    if (v) startScanner();
    else closeScanner();
  });

  // ── 生命周期 ────────────────────────────────
  let statusTimer = null;
  let nowTimer = null;

  onMounted(async () => {
    await refreshStatus();
    await regenerateQr();
    statusTimer = setInterval(refreshStatus, 3000);
    nowTimer = setInterval(() => {
      nowSec.value = Math.floor(Date.now() / 1000);
    }, 1000);
  });

  onUnmounted(() => {
    if (statusTimer) clearInterval(statusTimer);
    if (nowTimer) clearInterval(nowTimer);
    closeScanner();
  });

  return {
    // 状态
    p2pEnabled,
    p2pBroadcast,
    p2pPermission,
    p2pStatus,
    remoteAddr,
    remoteCode,
    qrDataUrl,
    showScanner,
    videoEl,
    scannerError,
    rotateCountdown,
    discoveredPeers,
    trustedDevices,
    connectedPeers,
    // 动作 / 辅助
    refreshStatus,
    onToggleP2p,
    onToggleBroadcast,
    onPermissionChange,
    onRegenerateCode,
    onRequestMatch,
    onConnectDiscovered,
    onDisconnect,
    onRemoveTrusted,
    isTrusted,
    isConnected,
    peerKey,
    formatTime,
    openScanner,
    closeScanner,
  };
}

export function useP2pSettings() {
  if (!state) state = createState();
  return state;
}