<script setup>
/**
 * HIcon - 全局图标组件（bootstrap-icons 名称 → Hugeicons 映射）
 *
 * 用于"全面切换 hugeicons"过渡：以 bootstrap 的图标名（如 'play-fill'）作为
 * 统一入口，内部映射到 Hugeicons 组件渲染，避免逐个文件重写 import。
 *
 * 用法：
 *   <HIcon name="play-fill" :size="16" />
 *   <HIcon name="search" class="search-icon" />
 *
 * 未命中的名称回退到 MusicNote01Icon。
 *
 * 注意：这里显式命名导入（而非 import *），确保 tree-shaking 只打包用到的图标。
 */
import { computed } from 'vue';
import { HugeiconsIcon } from '@hugeicons/vue';
import {
  RefreshIcon,
  ArrowTurnBackwardIcon,
  ArrowLeft02Icon,
  RepeatIcon,
  ArrowRight01Icon,
  BarChartIcon,
  Logout01Icon,
  ExternalLinkIcon,
  PackageIcon,
  RadioIcon,
  Calendar01Icon,
  Camera01Icon,
  ReceiptTextIcon,
  CircleCheckIcon,
  ChevronDownIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  ChevronUpIcon,
  HistoryIcon,
  CloudDownloadIcon,
  CloudIcon,
  CpuIcon,
  DiscIcon,
  DropletIcon,
  FrownIcon,
  AlertCircleIcon,
  Alert02Icon,
  Folder01Icon,
  FolderOpenIcon,
  GithubIcon,
  GripIcon,
  HardDriveIcon,
  HeartIcon,
  HourglassIcon,
  Home01Icon,
  InboxIcon,
  InformationCircleIcon,
  Layout01Icon,
  Link01Icon,
  CheckListIcon,
  MusicNote01Icon,
  ListMusicIcon,
  PencilIcon,
  CrownIcon,
  MicIcon,
  MapPinIcon,
  Playlist01Icon,
  Clock01Icon,
  UserGroupIcon,
  UserIcon,
  TelephoneIcon,
  PlayIcon,
  AddCircleIcon,
  PowerIcon,
  QrCodeIcon,
  Rocket01Icon,
  SaveIcon,
  Search01Icon,
  SecurityIcon,
  ShuffleIcon,
  SlidersHorizontalIcon,
  StarIcon,
  StopIcon,
  ToolsIcon,
  Delete01Icon,
  TextIcon,
  Wifi01Icon,
  Wrench01Icon,
  CancelCircleIcon,
  Cancel01Icon,
} from '@hugeicons/core-free-icons';

const MAP = {
  'arrow-clockwise': RefreshIcon,
  'arrow-counterclockwise': ArrowTurnBackwardIcon,
  'arrow-left': ArrowLeft02Icon,
  'arrow-repeat': RepeatIcon,
  'arrow-right': ArrowRight01Icon,
  'bar-chart': BarChartIcon,
  'box-arrow-right': Logout01Icon,
  'box-arrow-up-right': ExternalLinkIcon,
  'box-seam': PackageIcon,
  'broadcast': RadioIcon,
  'calendar': Calendar01Icon,
  'camera': Camera01Icon,
  'card-text': ReceiptTextIcon,
  'check-circle-fill': CircleCheckIcon,
  'chevron-down': ChevronDownIcon,
  'chevron-left': ChevronLeftIcon,
  'chevron-right': ChevronRightIcon,
  'chevron-up': ChevronUpIcon,
  'clock-history': HistoryIcon,
  'cloud-arrow-down': CloudDownloadIcon,
  'cloud-arrow-down-fill': CloudDownloadIcon,
  'cloud-download': CloudDownloadIcon,
  'cloud-search': CloudIcon,
  'cpu': CpuIcon,
  'crown': CrownIcon,
  'disc': DiscIcon,
  'disc3': DiscIcon,
  'clock': Clock01Icon,
  'droplet-half': DropletIcon,
  'emoji-frown': FrownIcon,
  'exclamation-circle': AlertCircleIcon,
  'exclamation-circle-fill': AlertCircleIcon,
  'exclamation-triangle': Alert02Icon,
  'exclamation-triangle-fill': Alert02Icon,
  'folder': Folder01Icon,
  'folder2-open': FolderOpenIcon,
  'geo-alt': MapPinIcon,
  'github': GithubIcon,
  'grip-vertical': GripIcon,
  'hdd-network': HardDriveIcon,
  'heart': HeartIcon,
  'hourglass-split': HourglassIcon,
  'house': Home01Icon,
  'inbox': InboxIcon,
  'info-circle': InformationCircleIcon,
  'layout-text-window-reverse': Layout01Icon,
  'link-45deg': Link01Icon,
  'list-ol': CheckListIcon,
  'mic': MicIcon,
  'music-note': MusicNote01Icon,
  'music-note-beamed': MusicNote01Icon,
  'music-note-list': ListMusicIcon,
  'pencil': PencilIcon,
  'people': UserGroupIcon,
  'person': UserIcon,
  'phone': TelephoneIcon,
  'play-circle': PlayIcon,
  'play-fill': PlayIcon,
  'playlist': Playlist01Icon,
  'plus-circle': AddCircleIcon,
  'plus-lg': AddCircleIcon,
  'plus-square': AddCircleIcon,
  'power': PowerIcon,
  'qr-code': QrCodeIcon,
  'rocket-takeoff': Rocket01Icon,
  'save': SaveIcon,
  'search': Search01Icon,
  'shield-check': SecurityIcon,
  'shuffle': ShuffleIcon,
  'sliders': SlidersHorizontalIcon,
  'stars': StarIcon,
  'stop-fill': StopIcon,
  'tools': ToolsIcon,
  'trash': Delete01Icon,
  'type': TextIcon,
  'type-strikethrough': TextIcon,
  'ui-checks': CheckListIcon,
  'wifi': Wifi01Icon,
  'wrench-adjustable': Wrench01Icon,
  'x-circle-fill': CancelCircleIcon,
  'x-lg': Cancel01Icon,
};

const props = defineProps({
  /** bootstrap 风格图标名，如 'play-fill' / 'search' */
  name: { type: String, default: '' },
  /** 尺寸（px），默认 16 */
  size: { type: [Number, String], default: 16 },
  /** 描边宽度，默认 1.5 */
  strokeWidth: { type: Number, default: 1.5 },
});

const resolvedIcon = computed(() => MAP[props.name] || MusicNote01Icon);
</script>

<template>
  <HugeiconsIcon
    :icon="resolvedIcon"
    :size="size"
    :stroke-width="strokeWidth"
    class="hicon"
    v-bind="$attrs"
  />
</template>

<style scoped>
.hicon {
  display: inline-block;
  flex-shrink: 0;
  vertical-align: -2px;
}
</style>
