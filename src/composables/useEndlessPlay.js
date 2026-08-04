/**
 * useEndlessPlay - 无尽播放编排（左栏二 · 无尽播放）
 *
 * 提供两种无尽播放编排方式：
 *  1. 智能混音（smart-mix）：以"常听"歌曲为核心，与其余曲库交错循环，
 *     队列快触底时自动向队尾补充，实现无限播放。
 *  2. 完全随机（shuffle）：从整个音乐库完全随机播放，同样自动补充。
 *
 * "常听"集合：优先取最近播放记录里的歌曲 id（客户端记录），
 * 剩余曲库作为"其他"。这与后端播放统计存在差距，但接口尚未提供
 * 播放次数统计，先以最近播放近似。
 */
import { ref, watch } from 'vue';
import { library } from '@/api/musicSource';
import PlayerStore from '@/stores/player/index.js';
import { useFavorites } from './useFavorites.js';

const { recentLog, load: loadRecent } = useFavorites();

/** 补充阈值：剩余不足 N 首时补充 */
const REFILL_THRESHOLD = 8;
/** 每次补充数量 */
const REFILL_BATCH = 40;

let allSongsCache = null;
let allSongsCacheTime = 0;
const ALL_SONGS_TTL = 60_000;

/** 拉取全库歌曲（带缓存） */
async function getAllSongs(force = false) {
  const now = Date.now();
  if (!force && allSongsCache && now - allSongsCacheTime < ALL_SONGS_TTL) {
    return allSongsCache;
  }
  const data = await library.getSongsPage(0, 100000);
  allSongsCache = data.songs;
  allSongsCacheTime = now;
  return allSongsCache;
}

/** 打乱数组（原地，Fisher-Yates） */
function shuffle(arr) {
  const a = [...arr];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

/** 当前是否处于无尽播放模式 */
export const endlessMode = ref(null); // null | 'smart-mix' | 'shuffle'

let stopRefillWatcher = null;

/** 安装"快触底自动补充"监听 */
function installRefill() {
  stopRefillWatcher?.();
  stopRefillWatcher = watch(
    () => [PlayerStore.state.playlist.length, PlayerStore.state.currentIndex],
    () => {
      const len = PlayerStore.state.playlist.length;
      const idx = PlayerStore.state.currentIndex;
      if (len === 0 || idx < 0) return;
      if (len - idx - 1 <= REFILL_THRESHOLD) {
        actions.appendBatch();
      }
    }
  );
}

function stopRefill() {
  stopRefillWatcher?.();
  stopRefillWatcher = null;
}

/**
 * 构建智能混音队列：常听(前段) + 其他(后段) 交错。
 * 返回 flat 数组。
 */
async function buildSmartMixQueue(batch = REFILL_BATCH) {
  await loadRecent();
  const all = await getAllSongs();
  if (all.length === 0) return [];

  const recentIds = new Set((recentLog.value || []).map((r) => r.id));
  const hot = all.filter((s) => recentIds.has(s.id));
  const rest = all.filter((s) => !recentIds.has(s.id));

  // 前段：最近播放过的"常听" + 其余曲库前 10%（近似高频）
  const hotPool = shuffle(hot);
  const topPool = shuffle(rest.slice(0, Math.max(1, Math.round(rest.length * 0.1))));
  const topTier = shuffle([...hotPool, ...topPool]);
  const restPool = shuffle(rest.slice(topPool.length));

  const queue = [];
  let ti = 0;
  let ri = 0;
  let useTop = true;
  // 循环：top 一首 + rest 一首
  while (queue.length < batch && (ti < topTier.length || ri < restPool.length)) {
    if (useTop) {
      if (ti < topTier.length) queue.push(topTier[ti++]);
    } else if (ri < restPool.length) {
      queue.push(restPool[ri++]);
    } else if (ti < topTier.length) {
      queue.push(topTier[ti++]);
    }
    useTop = !useTop;
  }
  return queue;
}

/** 构建完全随机队列 */
async function buildShuffleQueue(batch = REFILL_BATCH) {
  const all = await getAllSongs();
  return shuffle(all).slice(0, batch);
}

const actions = {
  /**
   * 启动无尽播放。
   * @param {'smart-mix'|'shuffle'} mode
   * @param {object} [opts] { bpm? } 备用
   */
  async start(mode = 'smart-mix', opts = {}) {
    let queue;
    if (mode === 'shuffle') {
      queue = await buildShuffleQueue();
    } else {
      queue = await buildSmartMixQueue();
    }
    if (queue.length === 0) {
      throw new Error('音乐库为空，无法开始无尽播放');
    }
    endlessMode.value = mode;
    PlayerStore.setPlaylist(queue);
    installRefill();
    PlayerStore.play(queue[0], queue);
    return queue.length;
  },

    /** 手动追加一批（内部由 refill watcher 调用） */
    async appendBatch() {
      const mode = endlessMode.value;
      if (!mode) return 0;
      let extra;
      if (mode === 'shuffle') {
        extra = await buildShuffleQueue(REFILL_BATCH);
      } else {
        extra = await buildSmartMixQueue(REFILL_BATCH);
      }
      // 追加并去重（避免与队列中已有 id 重复）
      const existing = new Set(PlayerStore.state.playlist.map((t) => t.id));
      const fresh = extra.filter((t) => !existing.has(t.id));
      fresh.forEach((t) => PlayerStore.addToPlaylist(t));
      return fresh.length;
    },

  /** 停止无尽播放（保留当前队列，停止补充） */
  stop() {
    endlessMode.value = null;
    stopRefill();
  },
};

export function useEndlessPlay() {
  return {
    endlessMode,
    start: actions.start,
    stop: actions.stop,
    appendBatch: actions.appendBatch,
  };
}

export default actions;
