/**
 * useMusicSourceManager — 音乐源管理页的全部组合式逻辑。
 *
 * 职责：
 *  - 数据加载（本地文件夹 / 统计 / 网易云 / 酷狗配置）
 *  - 动作（重新扫描 / 添加文件夹 / 移除文件夹 / 跳转设置页）
 *  - 状态管理（loading / scanning / adding / removing / 各来源状态）
 *  - 动画编排（spin 加载动画，限定到组件根节点）
 *  - 订阅全局库变更事件，事件驱动刷新
 *
 * 对外行为与原 MusicSourceManager.vue 完全一致。
 */
import { ref, onMounted, watch, nextTick, useTemplateRef } from 'vue';
import { useRouter } from 'vue-router';
import { open } from '@tauri-apps/plugin-dialog';
import {
  addLocalFolder,
  removeLocalFolder,
  getFolders,
  getLocalStats,
  rescanAll,
} from '../api/musicSource';
import * as neteaseApi from '@/api/musicSource/netease.js';
import * as kugouApi from '@/api/musicSource/kugou.js';
import { usePerf } from '@/utils/performanceMonitor.js';
import { useAnime } from '@/composables/useAnime.js';
import { useLibraryEvents } from '@/composables/useLibraryEvents.js';

export function useMusicSourceManager() {
  const { start, end, log } = usePerf('MusicSourceManager');

  const rootRef = useTemplateRef('root');
  const { run } = useAnime(() => rootRef.value);
  const router = useRouter();

  // ── State ──────────────────────────────────────────────────────────────────
  const folders = ref([]);
  const stats = ref({ folder_count: 0, indexed_files: 0 });
  const isLoading = ref(true);
  const isScanning = ref(false);
  const isAdding = ref(false);
  const isRemoving = ref(false);

  // 网易云来源状态
  const neteaseState = ref({
    enabled: false,
    baseUrl: '',
    loggedIn: false,
    loaded: false,
  });

  // 酷狗来源状态
  const kugouState = ref({
    enabled: false,
    baseUrl: '',
    hasCookie: false,
    loaded: false,
  });

  // 订阅全局库变更事件 — 在其他视图触发变更时同步刷新本页统计
  const { libraryVersion } = useLibraryEvents();

  // ── Lifecycle ──────────────────────────────────────────────────────────────
  onMounted(async () => {
    if (folders.value.length === 0) {
      await loadData();
    } else {
      isLoading.value = false;
    }
  });

  // 监听库变更事件：跳过初始值 0，避免与 onMounted 重复。
  // 本页自身的删除/添加/扫描操作完成后，后端也会 emit 此事件触发刷新，
  // 因此 handleDeleteSource 等不再主动调用 loadData，统一由事件驱动。
  watch(libraryVersion, (v) => {
    if (v === 0) return;
    loadData();
  });

  // loading spinner：CSS @keyframes spin 已迁移至 anime.js ANIME_LOOP.spin
  watch([isScanning, isAdding], ([scanning, adding]) => {
    if (!scanning && !adding) return;
    nextTick(() => {
      run(({ animate, loopPresets }) => {
        animate('.spin', { ...loopPresets.spin });
      });
    });
  });

  // ── Data loading ───────────────────────────────────────────────────────────
  const loadData = async () => {
    start('loadData');
    try {
      const [paths, s, neteaseCfg, kugouCfg] = await Promise.all([
        getFolders(),
        getLocalStats(),
        neteaseApi.getConfig().catch(() => null),
        kugouApi.getConfig().catch(() => null),
      ]);
      folders.value = (paths || []).map((p) => ({
        path: p,
        // 从路径提取显示名（最后一个目录名）
        name: p.split(/[/\\]/).filter(Boolean).pop() || p,
      }));
      stats.value = s || { folder_count: 0, indexed_files: 0 };
      if (neteaseCfg) {
        const cookie = (neteaseCfg.cookie || '').trim();
        neteaseState.value = {
          enabled: !!neteaseCfg.enabled,
          baseUrl: neteaseCfg.base_url || '',
          loggedIn: /(^|;\s*)MUSIC_U\s*=/.test(cookie),
          loaded: true,
        };
      }
      if (kugouCfg) {
        const cookie = (kugouCfg.cookie || '').trim();
        kugouState.value = {
          enabled: !!kugouCfg.enabled,
          baseUrl: kugouCfg.base_url || '',
          hasCookie: cookie.length > 0,
          loaded: true,
        };
      }
      end('loadData', { folderCount: folders.value.length, indexedFiles: stats.value.indexed_files });
    } catch (error) {
      console.error('Failed to load sources:', error);
      end('loadData', { error: error.message });
    } finally {
      isLoading.value = false;
    }
  };

  // ── Actions ────────────────────────────────────────────────────────────────
  const handleScanAll = async () => {
    isScanning.value = true;
    log('handleScanAll');
    start('scanAll');
    try {
      await rescanAll();
      await loadData();
      end('scanAll');
    } catch (error) {
      console.error('Failed to scan sources:', error);
      end('scanAll', { error: error.message });
    } finally {
      isScanning.value = false;
    }
  };

  const handleAddSource = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: '选择音乐文件夹',
      });

      if (selected) {
        isAdding.value = true;
        log('handleAddSource', { path: selected });
        start('addSource');
        await addLocalFolder(selected);
        await loadData();
        end('addSource');
      }
    } catch (error) {
      console.error('Failed to add source:', error);
      alert('添加音乐源失败: ' + error.message);
    } finally {
      isAdding.value = false;
    }
  };

  const handleDeleteSource = async (folder) => {
    const confirmed = confirm(`确定要移除音乐源 "${folder.name}" 吗？\n\n该操作将从数据库中清理该文件夹下的所有索引数据，不会删除磁盘上的文件。`);
    if (!confirmed) return;

    isRemoving.value = true;
    log('handleDeleteSource', { path: folder.path });
    start('deleteSource');
    try {
      await removeLocalFolder(folder.path);
      await loadData();
      end('deleteSource');
    } catch (error) {
      console.error('Failed to remove source:', error);
      end('deleteSource', { error: error.message });
      alert('移除音乐源失败: ' + error.message);
    } finally {
      isRemoving.value = false;
    }
  };

  // ── Helpers ────────────────────────────────────────────────────────────────
  const getSourceTypeLabel = () => '本地文件夹';

  const formatFileCount = (n) => {
    if (n == null) return '—';
    return n.toLocaleString();
  };

  // 跳转到网易云来源设置页
  const goToNeteaseSettings = () => {
    router.push('/music-sources/netease');
  };

  // 跳转到酷狗来源设置页
  const goToKugouSettings = () => {
    router.push('/music-sources/kugou');
  };

  return {
    rootRef,
    folders,
    stats,
    isLoading,
    isScanning,
    isAdding,
    isRemoving,
    neteaseState,
    kugouState,
    getSourceTypeLabel,
    formatFileCount,
    loadData,
    handleScanAll,
    handleAddSource,
    handleDeleteSource,
    goToNeteaseSettings,
    goToKugouSettings,
  };
}