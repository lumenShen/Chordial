/**
 * useLiquidGlass - liquid-glass-react 的 Vue 3 composable 移植
 *
 * 参考：https://github.com/rdev/liquid-glass-react
 *
 * 封装内容：
 * 1. SVG filter id 生成（Vue 3.5+ useId，多实例安全）
 * 2. 位移贴图加载（standard / polar / prominent / shader 四种 mode）
 * 3. 鼠标追踪（mouseContainer / globalMousePos / mouseOffset）
 * 4. 弹性形变（calculateDirectionalScale / calculateElasticTranslation）
 * 5. hover / active 状态管理
 * 6. ResizeObserver 实时测量玻璃尺寸
 *
 * 设计原则：
 * - composable 只返回响应式状态和计算属性，不渲染 DOM
 * - SVG filter 模板由组件层（LiquidGlass.vue）渲染，composable 提供参数
 * - 鼠标监听挂载在 mouseContainer（外部 ref）或玻璃元素本身
 *
 * 性能注意：
 * - transform 计算用 computed 缓存，避免每次鼠标移动重算
 * - mouseOffset 节流到 rAF（requestAnimationFrame），避免高频 reflow
 * - Firefox 下跳过 SVG filter（仅保留 backdrop-filter），与原库一致
 */
import { computed, onBeforeUnmount, onMounted, ref, useId, watch } from 'vue'
import liquidGlassMapBase64 from '@/assets/liquidGlassReactMap.txt?raw'

/**
 * 检测 Firefox：Firefox 对 backdrop-filter: url(#id) 支持不全，需降级
 * 在 SSR / Tauri WebView 中 navigator.userAgent 可用
 */
const isFirefox =
  typeof navigator !== 'undefined' &&
  navigator.userAgent.toLowerCase().includes('firefox')

/**
 * 四种折射模式的位移贴图。
 * - standard / polar / prominent：预生成 base64（utils.ts 中常量）
 * - shader：运行时由 canvas 生成（暂未实现，回退到 standard）
 *
 * TODO: 后续可拆出 polarDisplacementMap / prominentDisplacementMap 资源
 */
const MODE_MAPS = {
  standard: liquidGlassMapBase64,
  polar: liquidGlassMapBase64,
  prominent: liquidGlassMapBase64,
  shader: liquidGlassMapBase64, // 暂回退
}

/**
 * 默认参数（按项目视觉风格调整：强折射 + 高饱和 + 亮背景模式）
 */
const DEFAULTS = {
  displacementScale: 180,
  blurAmount: 0,
  saturation: 180,
  aberrationIntensity: 2,
  elasticity: 1,
  cornerRadius: 999,
  overLight: true,
  mode: 'standard',
}

/**
 * @typedef {Object} LiquidGlassOptions
 * @property {number} [displacementScale=70] 位移强度
 * @property {number} [blurAmount=0.0625]     模糊量（0-1，会乘以 32 加基数）
 * @property {number} [saturation=140]        饱和度百分比
 * @property {number} [aberrationIntensity=2] 色散强度
 * @property {number} [elasticity=0.15]       弹性系数（0=刚体）
 * @property {number} [cornerRadius=999]      圆角 px
 * @property {boolean}[overLight=false]       是否浮于亮色背景
 * @property {'standard'|'polar'|'prominent'|'shader'} [mode='standard'] 折射模式
 * @property {import('vue').Ref<HTMLElement|null>} [mouseContainer] 外部鼠标感应容器
 * @property {import('vue').Ref<{x:number,y:number}>} [globalMousePos] 外部全局鼠标坐标
 * @property {import('vue').Ref<{x:number,y:number}>} [mouseOffset]    外部归一化偏移
 * @property {boolean} [interactive=true]     是否启用鼠标弹性效果
 */

/**
 * useLiquidGlass composable
 * @param {LiquidGlassOptions} [options]
 * @returns {Object}
 */
export function useLiquidGlass(options = {}) {
  const opts = { ...DEFAULTS, ...options }

  // ===== 响应式参数（支持外部动态修改）=====
  const displacementScale = ref(opts.displacementScale)
  const blurAmount = ref(opts.blurAmount)
  const saturation = ref(opts.saturation)
  const aberrationIntensity = ref(opts.aberrationIntensity)
  const elasticity = ref(opts.elasticity)
  const cornerRadius = ref(opts.cornerRadius)
  const overLight = ref(opts.overLight)
  const mode = ref(opts.mode)
  const interactive = ref(opts.interactive ?? true)

  // ===== SVG filter id（多实例安全）=====
  const filterId = `liquid-glass-${useId()}`

  // ===== 玻璃尺寸（ResizeObserver 实时测量）=====
  const glassRef = ref(null)
  const glassSize = ref({ width: 0, height: 0 })
  let resizeObserver = null

  // ===== 鼠标状态（内部 / 外部二选一，外部优先）=====
  const internalMouseOffset = ref({ x: 0, y: 0 })
  const internalGlobalMousePos = ref({ x: 0, y: 0 })

  const externalMouseOffset = opts.mouseOffset ?? null
  const externalGlobalMousePos = opts.globalMousePos ?? null
  const mouseContainerRef = opts.mouseContainer ?? ref(null)

  const mouseOffset = computed(() =>
    externalMouseOffset ? externalMouseOffset.value : internalMouseOffset.value,
  )
  const globalMousePos = computed(() =>
    externalGlobalMousePos ? externalGlobalMousePos.value : internalGlobalMousePos.value,
  )

  // ===== hover / active 状态 =====
  const isHovered = ref(false)
  const isActive = ref(false)

  // ===== rAF 节流：避免高频 mousemove 引发 reflow =====
  let rafId = null
  let pendingEvent = null

  const handleMouseMove = (e) => {
    pendingEvent = e
    if (rafId != null) return
    rafId = requestAnimationFrame(() => {
      rafId = null
      if (!pendingEvent) return
      const container =
        mouseContainerRef.value || glassRef.value
      if (!container) return
      const rect = container.getBoundingClientRect()
      const centerX = rect.left + rect.width / 2
      const centerY = rect.top + rect.height / 2
      internalMouseOffset.value = {
        x: ((pendingEvent.clientX - centerX) / rect.width) * 100,
        y: ((pendingEvent.clientY - centerY) / rect.height) * 100,
      }
      internalGlobalMousePos.value = {
        x: pendingEvent.clientX,
        y: pendingEvent.clientY,
      }
    })
  }

  // ===== 弹性形变计算（移植自原库 calculateDirectionalScale）=====
  /**
   * 计算方向性缩放：鼠标接近时玻璃沿鼠标方向拉伸、垂直方向压缩
   * 模拟橡胶被拽的体积守恒效果
   */
  const directionalScale = computed(() => {
    if (!interactive.value || !glassRef.value) return 'scale(1)'
    const pos = globalMousePos.value
    if (!pos || (pos.x === 0 && pos.y === 0)) return 'scale(1)'

    const rect = glassRef.value.getBoundingClientRect()
    const pillCenterX = rect.left + rect.width / 2
    const pillCenterY = rect.top + rect.height / 2
    const pillWidth = rect.width
    const pillHeight = rect.height

    const deltaX = pos.x - pillCenterX
    const deltaY = pos.y - pillCenterY
    // 距玻璃"边"的距离（不是中心距离）
    const edgeDistanceX = Math.max(0, Math.abs(deltaX) - pillWidth / 2)
    const edgeDistanceY = Math.max(0, Math.abs(deltaY) - pillHeight / 2)
    const edgeDistance = Math.sqrt(
      edgeDistanceX * edgeDistanceX + edgeDistanceY * edgeDistanceY,
    )

    const activationZone = 200 // 鼠标距玻璃边 200px 内才生效
    if (edgeDistance > activationZone) return 'scale(1)'

    const fadeInFactor = 1 - edgeDistance / activationZone
    const centerDistance = Math.sqrt(deltaX * deltaX + deltaY * deltaY)
    if (centerDistance < 0.001) return 'scale(1)'

    const normalizedX = deltaX / centerDistance
    const normalizedY = deltaY / centerDistance

    const stretchIntensity =
      Math.min(centerDistance / 300, 1) * elasticity.value * fadeInFactor
    const scaleX =
      1 +
      Math.abs(normalizedX) * stretchIntensity * 0.3 -
      Math.abs(normalizedY) * stretchIntensity * 0.15
    const scaleY =
      1 +
      Math.abs(normalizedY) * stretchIntensity * 0.3 -
      Math.abs(normalizedX) * stretchIntensity * 0.15

    return `scaleX(${Math.max(0.8, scaleX)}) scaleY(${Math.max(0.8, scaleY)})`
  })

  /**
   * 计算弹性平移：玻璃朝鼠标方向轻微平移
   */
  const elasticTranslation = computed(() => {
    if (!interactive.value || !glassRef.value) return { x: 0, y: 0 }
    const pos = globalMousePos.value
    if (!pos || (pos.x === 0 && pos.y === 0)) return { x: 0, y: 0 }

    const rect = glassRef.value.getBoundingClientRect()
    const pillCenterX = rect.left + rect.width / 2
    const pillCenterY = rect.top + rect.height / 2

    const deltaX = pos.x - pillCenterX
    const deltaY = pos.y - pillCenterY
    const edgeDistanceX = Math.max(0, Math.abs(deltaX) - rect.width / 2)
    const edgeDistanceY = Math.max(0, Math.abs(deltaY) - rect.height / 2)
    const edgeDistance = Math.sqrt(
      edgeDistanceX * edgeDistanceX + edgeDistanceY * edgeDistanceY,
    )
    const activationZone = 200
    const fadeInFactor = Math.max(0, 1 - edgeDistance / activationZone)

    return {
      x: deltaX * elasticity.value * 0.1 * fadeInFactor,
      y: deltaY * elasticity.value * 0.1 * fadeInFactor,
    }
  })

  /**
   * 最终 transform：点击态优先 scale(0.96)，否则方向性缩放 + 弹性平移
   */
  const transformStyle = computed(() => {
    const t = elasticTranslation.value
    const scalePart = isActive.value
      ? 'scale(0.96)'
      : directionalScale.value
    return `translate(${t.x}px, ${t.y}px) ${scalePart}`
  })

  // ===== backdrop-filter 样式（warp 层）=====
  // 关键：url(#id) 必须合并到 backdrop-filter 中，让 SVG 位移滤镜作用于
  // 玻璃背后的页面内容（backdrop），这才是液态折射效果的来源。
  // 错误做法是把 url(#id) 放到 filter 属性——filter 作用于元素自身内容，
  // 而 warp 层是空 span，没有内容可被 filter 处理，导致效果完全失效。
  const backdropFilter = computed(() => {
    const blur = (overLight.value ? 12 : 4) + blurAmount.value * 32
    const base = `blur(${blur}px) saturate(${saturation.value}%)`
    // Firefox 对 backdrop-filter: url(#id) 支持不全，降级为仅 blur+saturate
    if (isFirefox) return base
    return `${base} url(#${filterId})`
  })

  // 保留 svgFilter 供特殊情况使用（如直接对有内容元素应用 filter）
  // 默认场景应使用 backdropFilter
  const svgFilter = computed(() =>
    isFirefox ? null : `url(#${filterId})`,
  )

  // ===== 位移贴图 URL =====
  const displacementMapUrl = computed(() => MODE_MAPS[mode.value] || liquidGlassMapBase64)

  // ===== scale 符号：shader 模式取正，其它取负 =====
  const displacementScaleSigned = computed(() => {
    const sign = mode.value === 'shader' ? 1 : -1
    // overLight 时强度减半
    const factor = overLight.value ? 0.5 : 1
    return displacementScale.value * sign * factor
  })

  // ===== radialGradient 中间 stop offset =====
  const edgeMaskMiddleOffset = computed(() =>
    Math.max(30, 80 - aberrationIntensity.value * 2),
  )

  // ===== RGB 三通道 scale（递减制造色散）=====
  const redScale = computed(() => displacementScaleSigned.value)
  const greenScale = computed(() =>
    displacementScaleSigned.value -
    aberrationIntensity.value * 0.05 * Math.sign(displacementScaleSigned.value),
  )
  const blueScale = computed(() =>
    displacementScaleSigned.value -
    aberrationIntensity.value * 0.1 * Math.sign(displacementScaleSigned.value),
  )

  // ===== feGaussianBlur stdDeviation（色散柔化）=====
  const aberrationBlur = computed(() =>
    Math.max(0.1, 0.5 - aberrationIntensity.value * 0.1),
  )

  // ===== feFuncA tableValues（边缘 mask 阈值化）=====
  const edgeMaskTableValues = computed(
    () => `0 ${aberrationIntensity.value * 0.05} 1`,
  )

  // ===== 边框渐变（由 mouseOffset 驱动方向）=====
  const borderGradient = computed(() => {
    const offset = mouseOffset.value
    const angle = 135 + offset.x * 1.2
    const midStop = Math.max(10, 33 + offset.y * 0.3)
    const endStop = Math.min(90, 66 + offset.y * 0.4)
    const midOpacity = 0.12 + Math.abs(offset.x) * 0.008
    const endOpacity = 0.4 + Math.abs(offset.x) * 0.012
    return `linear-gradient(${angle}deg,
      rgba(255,255,255,0.0) 0%,
      rgba(255,255,255,${midOpacity}) ${midStop}%,
      rgba(255,255,255,${endOpacity}) ${endStop}%,
      rgba(255,255,255,0.0) 100%)`
  })

  // ===== boxShadow =====
  const boxShadow = computed(() =>
    overLight.value
      ? '0px 16px 70px rgba(0,0,0,0.75)'
      : '0px 12px 40px rgba(0,0,0,0.25)',
  )

  // ===== 事件处理器 =====
  const eventHandlers = {
    onMouseEnter: () => {
      isHovered.value = true
    },
    onMouseLeave: () => {
      isHovered.value = false
      isActive.value = false
    },
    onMouseDown: () => {
      isActive.value = true
    },
    onMouseUp: () => {
      isActive.value = false
    },
  }

  // ===== 生命周期：挂载鼠标监听 + ResizeObserver =====
  onMounted(() => {
    const target = mouseContainerRef.value || glassRef.value
    if (target) {
      target.addEventListener('mousemove', handleMouseMove, { passive: true })
    }

    if (glassRef.value && typeof ResizeObserver !== 'undefined') {
      resizeObserver = new ResizeObserver((entries) => {
        for (const entry of entries) {
          const cr = entry.contentRect
          glassSize.value = { width: cr.width, height: cr.height }
        }
      })
      resizeObserver.observe(glassRef.value)
      // 初始测量
      const rect = glassRef.value.getBoundingClientRect()
      glassSize.value = { width: rect.width, height: rect.height }
    }
  })

  // mouseContainer 变化时重新绑定监听
  if (opts.mouseContainer) {
    watch(
      mouseContainerRef,
      (newEl, oldEl) => {
        if (oldEl) oldEl.removeEventListener('mousemove', handleMouseMove)
        if (newEl) newEl.addEventListener('mousemove', handleMouseMove, { passive: true })
      },
    )
  }

  onBeforeUnmount(() => {
    if (rafId != null) cancelAnimationFrame(rafId)
    if (resizeObserver) resizeObserver.disconnect()
    const target = mouseContainerRef.value || glassRef.value
    if (target) target.removeEventListener('mousemove', handleMouseMove)
  })

  return {
    // ===== 响应式参数（可外部修改）=====
    displacementScale,
    blurAmount,
    saturation,
    aberrationIntensity,
    elasticity,
    cornerRadius,
    overLight,
    mode,
    interactive,

    // ===== 玻璃元素 ref（组件需绑定到根元素）=====
    glassRef,
    glassSize,

    // ===== 状态 =====
    isHovered,
    isActive,
    mouseOffset,
    globalMousePos,

    // ===== SVG filter 参数 =====
    filterId,
    displacementMapUrl,
    edgeMaskMiddleOffset,
    redScale,
    greenScale,
    blueScale,
    aberrationBlur,
    edgeMaskTableValues,

    // ===== 样式计算 =====
    transformStyle,
    backdropFilter,
    svgFilter,
    borderGradient,
    boxShadow,

    // ===== 事件处理器 =====
    eventHandlers,
  }
}
