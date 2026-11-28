# chordial-amll-gpui

在 [gpui](https://github.com/zed-industries/zed/tree/main/crates/gpui) 上等价重实现
[AMLL（Apple Music Like Lyrics）](https://amll.dev/) 的核心歌词组件：

- **Lyric render**：逐字形绘制、整行扫光渐变（跨字/词连续前沿）、逐词上浮（离场回退）、
  强调词按词段合并后逐字缩放/位移/辉光、背景歌词行、对唱行、翻译/音译、
  间奏呼吸点；
- **LyricPlayer**：AMLL `LyricPlayerBase` 的热行/缓冲行状态机、解析弹簧滚动、
  对齐锚点、隐藏已播放行、模糊/缩放过渡；
- **LyricBackgroundRenderer**：AMLL `BackgroundRender` / `MeshGradientRenderer`
  的 Rust/gpui 等价实现（专辑封面 → 32×32 调色纹理 → BHP 双三次网格 →
  旋转/音量/暗角/抖动），以及纯色（CSS）与 None 渲染器；
- **歌词解析**：LRC / ESLRC（逐词）。

> 行为与视觉参数参考 AMLL（GPL-3.0）的公开实现与文档，本 crate 为独立
> Rust 重写，未复制其源码。AMLL 项目地址：https://github.com/Steve-xmh/applemusic-like-lyrics

## 基准版本

参数与行为以本机 `amll-player` 实际锁定的 **`@applemusic-like-lyrics/core 0.5.2` /
`@applemusic-like-lyrics/react-full 0.4.2`** 为准（参考 checkout `0f3a9a24`），
不混用其他版本（例如 0.7.1）的公式。`react-full` 的 `PrebuiltLyricPlayer` 使用
`DomLyricPlayer`，首次布局、动态弹簧参数和背景行缩放均按该锁定版本核对。

## 与 AMLL 的对应关系

| AMLL | 本 crate |
| --- | --- |
| `LyricLine` / `LyricWord` (`interfaces.ts`) | `model::LyricLine` / `model::LyricWord` |
| `LyricPlayerBase` (`lyric-player/base.ts`) | `player::LyricPlayer` |
| `DomSlimLyricPlayer` + `lyric-line.ts` | `render::LyricPlayerView` + 逐字形绘制 |
| `utils/spring.ts` | `spring::Spring`（解析解，帧率无关） |
| `bg-render/base.ts` + `AbstractBaseRenderer` | `background::LyricBackgroundRenderer` |
| `MeshGradientRenderer` + BHP mesh + shader | `background::MeshGradientRenderer`（CPU 光栅化） |
| `lyricBackgroundRendererAtom` 的 `{ renderer }` 配置 | `background::BackgroundRendererKind` |
| `@applemusic-like-lyrics/lyric` 的 LRC/ESLRC | `parser::parse_lrc` / `parse_eslrc` |

## 性能与内存约束

- 每帧只整形/绘制 `LyricPlayer::visible_range()`（含 300px overscan）；
  文本整形结果按行 LRU 缓存（默认 256 行），淘汰后保留高度估算。
- 弹簧是解析解，`update` 只遍历"可见 + 未收敛"的行集合，与歌词总行数无关。
- `set_time` 用二分 + 前缀最大结束时间定位活跃行，避免每帧 O(n) 扫描。
- 背景光栅化输出分辨率严格由 `render_scale` 决定，不按耗时或像素预算缩小；
  光栅化/合成缓冲跨帧复用；每帧仅最终 BGRA 上传缓冲分配一次；
  低于目标 FPS 时复用上一帧 `RenderImage`。
- 所有热点路径有 `amll.*` 性能检查点（见 `chordal-core/src/perf.rs`），
  `tests/perf_scaling.rs` 断言解析/整理/布局/更新的复杂度不退化。

## 内存上界（默认配置）

| 项 | 上界 |
| --- | --- |
| 文本整形缓存 | 256 行（按行 LRU） |
| 专辑调色纹理 | 32×32 RGBA = 4 KB |
| 背景光栅缓冲 | 默认显示设备像素 ÷ 4 × 4B × 2（合成 + scratch） |
| 背景上传帧 | max(130k, 光栅像素) × 4B（由 gpui 持有） |
| 封面 RGBA | 最长边 512 → 1 MB（由 UI 侧持有一次） |
| 播放器行状态 | O(歌词行数)，约 200 B/行 |

## 等价范围

**语义/参数完全对齐（除下方明确差异）**：热行/缓冲行状态机、滚动偏移与逐行
delay、间奏判定（≥4s、下一行前 250ms）、对齐锚点、隐藏已播放行、模糊/缩放
阈值（`blur = 1 + 距离`，窄视口 ×0.8）、逐词上浮（0.05em / 背景行 0.1em，
`max(1s, 词时长)`、`ease-out`、离场倒放）、强调词条件与缓动（含
`chunkAndSplitLyricWords` 的非 CJK 合并语义）、强调词 `text-shadow` 辉光、
Mesh 的 BHP 矩阵、UV 映射、旋转/音量/暗角/抖动公式、32×32 调色与滤镜顺序。
扫光改为**整行一条 mask 前沿**（软边默认 0.6em，设置 → 播放 → 歌词可调），
不再按词各自重播。

**弹簧参数（core 0.5.2）**：

| 用途 | mass | damping | stiffness |
| --- | --- | --- | --- |
| 普通播放 posY | 0.9 | `2.2 * sqrt(stiffness)` | `170 + 50 * ratio^0.2` |
| seek / 间奏 posY | 0.9 | 15 | 90 |
| 主行 scale | 2 | 25 | 100 |
| 背景行 scale | 1 | 20 | 50 |

普通播放的 `interval = clamp(当前主行起点 - 上一主行起点, 100, 800)`，
`ratio = 1 - (interval - 100) / 700`。参数只在计算结果真正变化时更新，
重切解析解会保留当前位置与速度。首次可见行从 `2 * height` 屏外进入。
`soft` 仍按 AMLL 语义强制临界阻尼；非 soft 且 ζ>1 时使用真正的双实根解析解，
修正参考 0.5.2 把该分支也送入临界阻尼公式的问题。

**背景的刻意差异**：鼓点包络在本实现里额外驱动网格顶点的整体呼吸（NDC
向外扩张）与 RGB 提亮，让低分辨率、平滑封面上也能看清击打；AMLL 只做
UV 缩放/旋转。所有鼓点视觉幅度乘以 `set_beat_strength` 倍率（0–1.5，
应用默认 0.5，可在设置中调整，0 只保留背景流动）。封面切换按墙钟 420ms
交叉淡出，退场层保持不透明，避免高刷屏上多层 alpha 叠加导致背景变暗；
旧背景帧会主动从 gpui sprite atlas 移除。

**逐像素对照（2026-11，amll-player 锁定 core 0.5.2 / react-full 0.4.2）**：
在 1600×1000 与 800×1000 两个窗口下逐元素测量真实 AMLL Player
（react-full `PrebuiltLyricPlayer`，`DomLyricPlayer`），并将下列数值写入实现：

| 项 | 实测值（已实现） |
| --- | --- |
| 字号 | `max(max(5vh, 2.5vw), 14px)`（宽 ≤768：`max(8vw, 12px)`） |
| 字重 / 行高 | 600 / 1.2 |
| 译文 | 0.5em、行高 1.5em、透明度 0.3 |
| 背景行 | 0.7em、wrapper 0.4 |
| 行内边距 | 左右 1em、上下 0.4em（= 0.02×H） |
| 容器渐隐 | 上下 10%（竖向仅顶部 10%） |
| 高亮行 | wrapper 0.85；动态行亮/暗层 1.0/0.4 → 非活跃 0.2 |
| 非当前行缩放 | 0.97（播放中）/ 背景行 0.75 |
| 行缩放锚点 | `.lyricLine { transform-origin: left }`（对唱行 `right`）→ 左/右边缘 × 垂直中心；`translateY` 在外层 `.lyricLineWrapper` |
| 强调缩放锚点 | 字符 `<span>` 未设 `transform-origin`，取默认 `50% 50%`（字符盒中心） |
| 间奏点 | `.interludeDots` 的左右 padding = 歌词文本 padding；`transform-origin: center` | 首点与歌词文本左边缘对齐（对唱行与右边缘对齐），整组围绕中心缩放 |
| 逐词浮动 | 每词 `0 → -0.05em`（BG ×2），`max(1s, 词时长)`、delay = 词起点、`ease-out`；离场 `playbackRate=-1` 回退 |
| 强调分组 | `chunkAndSplitLyricWords`：非 CJK 合并无空白相邻词；CJK 先拆单字，按时长 ≥1s 判断 |
| 强调辉光 | `text-shadow: 0 0 min(0.3, blur*0.3)em`，alpha = `empEasing(x) * blur`（真高斯模糊，σ=R/2） |
| 弹簧 | posY 普通播放按行间隔动态计算；seek/间奏 m0.9 d15 s90；主行 scale m2 d25 s100；背景行 scale m1 d20 s50 |

**GPU 渲染路径（2026-11 优化）**：

- **背景**：沉浸层背景 = 动画 Mesh Gradient，铺满整个窗口。光栅分辨率由
  调用方 `set_background_render_scale` 控制（Chordial 默认按显示区域 ÷ 2，
  除数严格决定光栅尺寸；30fps），由 `paint_image`
  交给 GPU **双线性采样放大到全窗** —— 屏幕上每个输出像素的采样/混合都在
  GPU 上完成。光栅内循环做了三处优化：增量重心坐标递推、位运算镜像重复
  采样（去掉逐像素 `rem_euclid`）、调色板 BGRA 预转换（帧缓冲零拷贝）。
  背景不可见时完全跳过光栅与动画帧请求。
- **歌词字形**：全部通过 gpui 的 GPU 精灵/图元管线绘制。模糊在**字形光栅化
  阶段**完成：对 alpha 掩码做可分离高斯（σ<2）或 Kovesi 三次盒式近似（σ≥2，
  与 Skia `ThreeBoxApproxPass` 同族），结果缓存在字形图集里，每个字形仍只有
  一次 sprite draw。σ 按 0.5 设备像素量化，上限 5px（与 AMLL `lyric-group.ts` 的
  `min(5, blur)` 一致）。边缘渐隐区整行跳过。
- **亚像素定位与缩放**：歌词位置/缩放来自弹簧，每帧都是分数。字形通过
  `paint_glyph_scaled` 按 4×4 子像素档位光栅化（Windows 上关闭 DirectWrite
  的纵向 grid fit），基线不再被吸附到整像素；行缩放（0.97/0.75）围绕
  AMLL `.lyricLine` 的 `transform-origin: left/right`（左/右边缘 × 垂直中心），
  强调缩放（1.1）围绕字符盒中心（AMLL 字符 span 的默认 `50% 50%`），
  都用 sprite 变换缩放**已缓存的字形位图**，而不是把 `font_size × scale`
  当新字号重新光栅化。效果：滚动时不再「几帧不动、然后跳 1px」，缩放动画
  期间抗锯齿/字形高度也不会逐帧变化，字形图集 key 保持稳定。
- **扫光遮罩（mask-image）**：动态歌词的亮暗不是逐字改色，而是给整行一条
  共享的水平渐变遮罩（`SpriteMask`）：`light_from` 左侧为亮层、`dark_from`
  右侧为暗层，中间按 fragment x 线性插值。前沿在一行的所有词之间**连续
  推进**（词开始时从上一词尾出发，词结束时到达本词尾），因此渐变带会压在
  相邻的字/词之间，而不是每个词各自点亮一遍。软边宽度默认 `0.6em`
  （与 AMLL 一致），可在「设置 → 播放 → 歌词」调到 0.1–4.0em。遮罩在
  fragment shader 里计算，所以同一个宽字形内部也是连续渐变，并且同时作用
  于强调词的辉光 sprite。AMLL 非活跃行的亮/暗 mask alpha 都是 0.2（均匀
  暗层），活跃行是 1 / 0.4。
- **逐词上浮**：普通词在自己的时长内以 `ease-out` 上浮 0.05em（背景行
  0.1em），行离开热行集合时按 AMLL 的 `playbackRate = -1` 回退；强调词改用
  字符级 float（`sin(πx)·up + x·up`，按字符 stagger），并与辉光/缩放动画
  共用同一条时间线。
- **帧节奏**：由 gpui 真实 `request_animation_frame` 驱动，不伪造定时 RAF。
  `prepaint` 先完成测量/布局并启动新弹簧，再推进动画、组合各动画源并决定
  是否请求下一帧；歌词动画跟随显示器刷新率，仅背景流动时按 `bg_fps` 限速，
  全部静止后停止。暂停时已启动的弹簧仍会收敛并持续请求帧。

**实现载体差异（仍为近似，非逐像素相同）**：

| AMLL | 本 crate | 差异 |
| --- | --- | --- |
| CSS `filter: blur()` | 字形 alpha 掩码的可分离高斯 / 三次盒式卷积 | 逐字形而非整行；σ 量化 0.5 设备像素、上限 5px |
| CSS `mask-image` 扫光 | `SpriteMask`：shader 内按 fragment x 的水平渐变 | 整行共享一条前沿（非逐词独立遮罩）；软边默认 0.6em，设置内可调 |
| 浏览器 CSS transition | 80ms 一阶平滑 | 过渡曲线略有不同 |
| WebGL 三角形光栅（全分辨率） | 低分辨率 CPU 场 + GPU 上采样 | 同一公式；1/64 像素量，GPU 放大 |
| `requestAnimationFrame` | gpui `request_animation_frame` | 与窗口刷新率一致 |
| 行 hover 底色 `#fff1` | 未实现行级 hover | 需要逐行命中测试 |

> gpui 0.2.2 未开放自定义 shader / 离屏渲染 / 字形位图（`rasterize_glyph`
> 为 crate 私有）。`vendor/gpui` 因此加了三个最小补丁：`paint_glyph_with_blur`
> 在平台光栅化后对 alpha 掩码做卷积，图集按 `(glyph, size, σ bucket)` 缓存；
> `paint_glyph_scaled` 支持分数基线 + 4×4 子像素档位 + 无网格拟合，并用
> sprite 变换承载缩放；`MonochromeSprite` 增加 `SpriteMask`，由
> Windows/Blade/Metal 三个 fragment shader 按设备像素 x 插值亮暗 alpha。
> 补丁说明见 `vendor/gpui/PATCHES.md`。

**与 DOM 版的有意差异**：

- 视口外（含 overscan）的行在布局时直接吸附目标，不像 AMLL 为全部 group
  逐帧更新弹簧；首次可见行仍从 `2 * height` 屏外进入。这样保持每帧更新成本
  只与可见行相关。
- 滚轮/拖拽/惯性歌词滚动尚未实现，`scroll_offset` 目前恒为 0；滚动边界仍按
  AMLL 公式计算，后续交互可直接复用。
- 非 soft 的真正过阻尼弹簧使用双实根解析解；`soft` 的强制临界阻尼语义不变。
- CSS 透明度/模糊 transition 用 80ms 一阶平滑近似，曲线并非逐像素相同。

**尚未实现**：AMLL 的滚轮/拖拽歌词滚动（见上方有意差异）；
非逐词 LRC 不做逐字扫光（与 AMLL `isNonDynamic` 分支一致，只有行级透明度）；
`parser` 模块是独立的 LRC/ESLRC 入口，Chordial 生产路径使用
`chordal-core` 的 LRC 解析 + YRC 逐词行，再交给本 crate 渲染。

## 最小用法

```rust
let view = cx.new(|cx| {
    let mut v = LyricPlayerView::new(cx);
    v.set_clock(Some(playback_clock)); // 实现 PlaybackClock
    v.set_lyrics(chordial_amll_gpui::parse_auto(&lrc_text)
        .into_iter()
        .collect(), 0);
    v
});
```

背景需要封面时，由调用方解码为 RGBA8 后 `set_album_rgba(w, h, data)`；
视图会在 `prepaint` 中按可见尺寸驱动网格动画。
