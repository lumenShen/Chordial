import { computed } from 'vue';
import { AmllSettingsStore, AMLL_DEFAULTS } from '@/stores/amllSettings.js';

/**
 * 歌词与播放器设置页的组合式逻辑。
 *
 * 直接绑定到 AmllSettingsStore.state，改动会自动通过 store.set 同步到 AMLL Jotai store，
 * 并由 AmllSettingsStore.bind 反向监听 atom 变化持久化到 localStorage。
 *
 * 拆分为子组件后，各设置区块（效果 / 字体 / 流体背景 / 显示项 / 布局 / 重置）
 * 通过调用本 composable 获取同一份响应式 state 与计算属性，保持对外行为不变。
 */
export function useLyricsSettings() {
	const settings = AmllSettingsStore.state;

	// 鼓点频段：用 computed 把 [low, high] 数组拆成两个双向绑定的数字
	// 滑块仅覆盖常用范围（下限 20~500、上限 ~2000），数字输入框允许输入超出滑块范围的值
	// 仅保留逻辑约束：low >= 0 且 low < high（间距至少 5Hz）
	const lowFreqLow = computed({
		get: () => settings.lowFreqVolumeRange[0],
		set: (v) => {
			if (!Number.isFinite(v)) return;
			const high = settings.lowFreqVolumeRange[1];
			// 强制下限 < 上限（保留 5Hz 间距），下限不得低于 0
			if (v >= high) v = high - 5;
			settings.lowFreqVolumeRange = [Math.max(0, v), high];
		},
	});
	const lowFreqHigh = computed({
		get: () => settings.lowFreqVolumeRange[1],
		set: (v) => {
			if (!Number.isFinite(v)) return;
			const low = settings.lowFreqVolumeRange[0];
			// 强制上限 > 下限（保留 5Hz 间距），上限不超过 20000Hz（人耳听觉上限）
			if (v <= low) v = low + 5;
			settings.lowFreqVolumeRange = [low, Math.min(20000, v)];
		},
	});

	function resetToDefaults() {
		if (!confirm('确定要清除所有 AMLL 配置并恢复默认值吗？')) return;
		for (const key of Object.keys(AMLL_DEFAULTS)) {
			settings[key] = Array.isArray(AMLL_DEFAULTS[key])
				? [...AMLL_DEFAULTS[key]]
				: AMLL_DEFAULTS[key];
		}
	}

	return {
		settings,
		lowFreqLow,
		lowFreqHigh,
		resetToDefaults,
	};
}