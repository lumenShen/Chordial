# gpui 0.2.2 本地补丁

上游 crate：`gpui 0.2.2`（Zed，crates.io）。这里加入了歌词渲染需要的
**字形模糊**、**平滑字形定位/缩放**、**水平渐变扫光遮罩**，以及播放器次要
控件使用的**场景级分组滤镜**；`paint_glyph` 的行为保持不变。升级 gpui 时
需要把下面几处重新应用。

## 动机

gpui 没有内容滤镜（`filter: blur()`），`paint_glyph` 只能画未模糊的字形位图。
歌词行原先用「同一字形按稀疏偏移重复画 9 次」近似高斯，本质是 delta 核 +
source-over 叠加，σ≥2 时重影明显、内部 alpha 只有约 0.66。

另外，`paint_glyph` 把字形起点 `floor()` 到整个设备像素，且 Windows/Linux 的
纵向子像素档位为 1。静态 UI 文本这样最清晰；但歌词每帧都在弹簧驱动下做
亚像素移动，整像素吸附会表现为「走几帧不动、然后跳 1px」。同时行缩放若用
`font_size × scale` 每帧重新光栅化，DirectWrite 在每个分数字号下的 hinting
都会变化，导致抗锯齿和字形高度在帧间抖动，并让字形图集按连续字号膨胀。

最后，AMLL 的逐词点亮不是给每个字整块改色，而是 `mask-image` 的一条软渐变
边界扫过整行（左亮右暗）。逐字形 alpha 会在字形边界产生阶梯；需要在
fragment shader 里按屏幕 x 插值，才能让同一个字形内部也连续变化。

## 改动

1. `src/text_system.rs`
   - `RenderGlyphParams` 增加 `blur_radius: Pixels`（σ，设备像素，量化到 1/2 px），
     模糊字形的 `font_size` 也量化到 1/2 逻辑 px，避免弹簧动画把图集 key 空间撑大；
     `Hash` 一并纳入；图集 key 因此自动按 σ 分桶。
   - `RenderGlyphParams` 增加 `subpixel_positioning: bool`：
     - 为 true 时纵向使用 `SUBPIXEL_VARIANTS_Y_PRECISE`（4 档），并让平台光栅器
       关闭纵向网格拟合（DirectWrite：`DWRITE_GRID_FIT_MODE_DISABLED`），
       否则分数基线会被重新吸附到整像素；
     - 为 false 时保持上游行为（Windows/Linux 纵向 1 档、默认网格拟合）。
   - 新增 `RenderGlyphParams::subpixel_variant_counts()` / `subpixel_shift()`，
     各平台按同一套档位数把 `subpixel_variant` 换算成设备像素偏移。
   - `TextSystem::raster_bounds`：有模糊时把平台返回的 bounds 四周外扩
     `ceil(3σ)`，给卷积留零填充边界。
   - `TextSystem::rasterize_glyph`：平台光栅化后对单通道 alpha 掩码做卷积。
   - 新增 `blur_alpha_mask`：
     - σ < 2：精确可分离高斯（半径 `ceil(3σ)`）；
     - σ ≥ 2：Kovesi 三次盒式近似，`w ≈ sqrt(4σ² + 1)`，在最近两个奇数宽度
       之间按方差混合；整条链保持 f32，最后一次量化到 u8。
     - 与 Skia `SkBlurEngine` 的策略一致（`GaussianPass` / `ThreeBoxApproxPass`）。
2. `src/window.rs`
   - 新增公开 `Window::paint_glyph_with_blur(origin, font_id, glyph_id,
     font_size, color, blur_radius)`。
   - 新增公开 `Window::paint_glyph_scaled(origin, font_id, glyph_id, font_size,
     color, blur_radius, scale)`：
     - 基线取 **分数坐标**（`origin` 不再 floor），按 4×4 子像素档位光栅化；
     - `scale` 不参与光栅化，而是通过 `MonochromeSprite::transformation`
       围绕基线原点缩放已缓存的字形 sprite。因此弹簧动画期间图集 key 不变，
       不会每帧重新 hinting/光栅化；
     - 模糊字形仍只用 1 个图集条目（位置的小数部分由变换矩阵平移承担），
       避免 σ × 16 倍图集。
   - 三个入口共用 `paint_glyph_inner`；`paint_glyph` 保持原行为。
3. 平台光栅器
   - Windows `platform/windows/direct_write.rs`：基线偏移改用
     `subpixel_shift()`；`subpixel_positioning` 时禁用纵向 grid fit。
   - Linux `platform/linux/text_system.rs`、macOS `platform/mac/text_system.rs`：
     改用 `subpixel_shift()`（macOS 本就有 4 档纵向子像素）。
4. 渐变扫光遮罩
   - `src/scene.rs`：新增公开 `SpriteMask { light_from, dark_from, light, dark }`，
     `MonochromeSprite` 末尾增加 `mask` 字段。`light_from < dark_from` 时
     fragment 在 `[light_from, dark_from]` 内从 `light` 线性过渡到 `dark`；
     `SpriteMask::NONE` 等价于乘 1。
   - `src/window.rs`：新增公开
     `paint_glyph_scaled_masked(..., scale, mask)`；遮罩 x 坐标按
     `scale_factor` 转为设备像素。`paint_glyph` / `paint_glyph_with_blur` /
     `paint_glyph_scaled` 使用 `SpriteMask::NONE`；SVG 的 `MonochromeSprite`
     也必须补 `mask: SpriteMask::NONE`。
   - 三个 fragment shader（Windows `shaders.hlsl`、Blade `shaders.wgsl`、
     Metal `shaders.metal`）：`MonochromeSprite` 增加 `SpriteMask`，顶点 shader
     把四个值作为 flat varying 传到片元；片元按
     `mix(light, dark, saturate((x - light_from) / (dark_from - light_from)))`
     乘到 alpha 上。`dark_from == light_from` 时退化为阶跃（`x >= light_from`
     取 dark），避免除零。
   - `build.rs`：cbindgen 的 `export.include` 增加 `"SpriteMask"`，Metal 的
     shared header 由 Rust 结构自动生成。
5. 图集边缘防渗色（修复强调词辉光的白点/白线）
   - `src/platform.rs`：新增 `ATLAS_TILE_PADDING = 1` 与
     `expand_atlas_tile_border()`；每个图集条目四周保留 1 像素边界，边界像素
     复制自图块边缘（不是黑色），所以 UV 落在图块边缘时线性采样不会读到
     相邻图块的字形墨迹。
   - Windows `directx_atlas.rs`、Blade `blade_atlas.rs`、Metal `metal_atlas.rs`：
     分配时把图块尺寸加上两侧边界，`AtlasTile.bounds.origin` 指向**内部**
     矩形，`padding` 记为 1；上传时写入含边界的矩形。shader 的
     `to_tile_position` 不需要改（它本来就是按 `bounds` 取内部矩形）。
     Windows 另外在 `CreateTexture2D` 时用零数据初始化纹理，避免尚未上传的
     图集区域是未定义内容。
   - `src/window.rs`：正的模糊半径量化后至少保留 0.5 设备像素；否则辉光会
     退化成未模糊的清晰白色字形，在文字边缘形成白点。
   - `chordial-amll-gpui` 的 `paint.rs`：emoji 跳过辉光（彩色 emoji 没有
     单色掩码，重复绘制只会叠出一份全亮度副本）。
6. `src/text_system.rs` 末尾 `#[cfg(test)] mod blur_alpha_tests`（参考用；
   vendor crate 不在 workspace 里，不能直接 `cargo test -p gpui`）。

## 精度验证

对真实字形（160×160，padding 30）与 PIL `GaussianBlur(sigma)` 对比：

| σ | RMSE | 最大差 |
| --- | --- | --- |
| 1.5 | 0.42 | 4 |
| 3.0 | 1.17 | 9 |
| 5.0 | 0.77 | 6 |

子像素定位（150% DPI，64px Segoe UI「夜」）：基线偏移 0 / 0.25 / 0.5 / 0.75
逻辑像素时，实测墨迹质心相对位移为 0 / 0.241 / 0.671 / 1.000 设备像素
（期望 0 / 0.25 / 0.75 / 1.0）；旧 `paint_glyph` 的前三档完全重合。

缩放锚点（150% DPI，48px，同一行 scale 1.0 与 0.97 对比）：垂直墨迹质心差
-0.13 设备像素、上下边界各收缩 1px（中心对称）——对应 AMLL `.lyricLine` 的
`transform-origin: left`（垂直中心）；水平左边缘仅移动 -1 设备像素
（0.2em 外扩锚点 + 字符中心缩放的合成结果）。旧实现围绕文本左上角缩放，
同场景垂直质心会额外上移约 1.8 设备像素。

扫光遮罩（150% DPI，48px）：截图检查活跃行，左侧字形为亮层、右侧为暗层，
软边界跨越当前词时在单个字形内部也呈现水平渐变（而不是整字跳变）；非活跃行
`light == dark == 0.2`，表现为均匀暗层。Windows/DirectWrite 路径已验证；
Blade/Metal shader 同步修改但未在本机运行。

## Windows 无边框窗口拖动 / 缩放

上游 `PlatformWindow::start_window_move` / `start_window_resize` 在 Windows
平台使用 trait 默认空实现，只有 Linux 实现了，因此客户区自绘标题栏无法拖动。
`platform/windows/window.rs` 增加：

- `start_window_move`：`ReleaseCapture()` 后
  `PostMessageW(WM_SYSCOMMAND, SC_MOVE | 2)`，由系统进入模态移动循环；
  低 4 位的 `2` 表示鼠标发起，效果与原生命中标题栏一致（贴边分屏、
  跨屏 DPI 变化、双击最大化均由系统处理）。
- `start_window_resize(edge)`：同样 `PostMessageW(WM_SYSCOMMAND, SC_SIZE + n)`，
  `n` 按 WinUser.h 的边/角编号（1 左 … 8 右下），让应用自绘的 8 个缩放手柄
  在 Windows 生效。

两条路径都用 `PostMessageW`，避免在 gpui 的 mouse-down 分发过程中重入窗口
过程。应用侧同时保留 `WindowControlArea::Drag`（`WM_NCHITTEST → HTCAPTION`）
与 mouse-down 兜底：返回 HTCAPTION 时客户区收不到 mouse-down，两条路径不会
同时触发。

## 300Hz 帧调度与绘制补丁

为支持 300Hz 显示与高回报率输入，另加了以下与歌词无关的改动：

1. **帧统计**：新增 `src/frame_stats.rs`，在 `src/window.rs` 的
   `on_request_frame` 回调中记录 draw / prepaint / paint / present / total /
   实际帧间隔，每 240 帧输出最近 240 帧窗口的 p50/p95/p99/max、窗口 miss
   数与累计 miss（`log target=gpui::frame`）。窗口统计避免启动/切页尖峰
   长期污染分位数；prepaint/paint 分段用于区分布局+预绘制与场景提交成本。
   环境变量：`GPUI_FRAME_STATS=1`、`GPUI_TARGET_HZ=300`。
2. **合成 300Hz 压测**：`src/platform/windows/vsync.rs` 支持
   `GPUI_BENCH_HZ=300`，用高精度 sleep+spin 替代 `DwmFlush`，并记录真实
   DWM 刷新周期。仅诊断用，默认关闭。
3. **路径平移/缩放合并**：`src/scene.rs` 新增 `Path::translated`；
   `src/window.rs` 新增 `paint_path_translated`，把缓存图标的
   “局部坐标 → 屏幕偏移 → 设备像素 → scene” 合并为一次顶点遍历。
4. **绘制分层**：新增 `src/elements/layer.rs` 的 `batch_layer`。同一 layer
   内所有 path/quad/glyph 共享一个 draw order，Windows 路径批处理从
   “每图标一次全屏 MSAA 通道”降为每 section 一次。
5. **`uniform_list` 复用测量**：`request_layout` 测得的第一项尺寸存入
   frame state，`prepaint` 不再重复渲染/布局该行。
6. **离屏 hitbox 裁剪**：`Window::insert_hitbox` 跳过与 content mask 不相交
   的 hitbox，滚动容器不再把离屏行计入每次命中测试。
7. **高回报率输入合并**：Windows 平台把 `WM_MOUSEMOVE` 与滚轮事件暂存到
   `WindowsWindowState`，在下一帧 `draw_window` 前统一派发一次；
   `WM_MOUSELEAVE` 清除待处理移动。1000Hz 鼠标不再每事件做一次完整命中测试。
8. **dirty view 统计**：`Window.dirty_views_count` 记录每帧开始绘制时的
   dirty 视图数，供帧统计使用。
9. **平滑滚动**：`Interactivity` 新增 `smooth_scroll()` 与
   `SmoothScrollState`（时间常数 45ms，帧率无关）。滚轮事件只更新目标偏移，
   prepaint 中推进实际偏移；未收敛时通过 `request_animation_frame` 继续
   请求帧。偏移被外部改写（程序化跳转、tracked handle、内容变化后的
   clamp）时重新锚定目标，避免动画把列表拉回旧位置。`Div` 与
   `UniformList` 都可启用；默认关闭，由应用配置控制。
10. **版本化保留子树**：新增 `src/elements/retained.rs` 的 `retained(...)`。
   调用方传入元素 id、版本号与布局 `Style`；版本未变时只请求一个叶子布局
   节点，并复用上一帧的 prepaint/paint 范围，不重建子树。侧栏歌单区用它
   避开每帧 30 行的构建、文本整形与布局。版本需包含所有内容/几何/裁剪/
   hover 输入（数据快照、滚动偏移、鼠标位置、主题、视口尺寸）。
   注意：用于行列表这类自包含子树是稳定的；把整块含交互视图/元素状态的
   面板包进保留子树，在 release（LTO）下出现过访问冲突，未再采用。
11. **布局复用越界保护**：`LineLayoutCache::reuse_layouts` 把传入的
   `LineLayoutIndex` 夹到上一帧实际长度内。视图树在两帧之间增删节点
   （抽屉/覆盖层切换）时上一帧索引可能超过当前缓存长度，原实现直接
   切片会 panic；夹取后只是少复用几条布局缓存，不影响正确性。
12. **保留子树区间重定位**（`src/elements/retained.rs`）：缓存里的
   `prepaint_range` / `paint_range` 是上一帧（子树重建那一帧）的索引，
   命中时原实现只回放、不更新。由于回放的条目在每帧的实际偏移会随
   前面的元素状态数量变化，旧索引很快就不再指向本子树的条目；超过
   当前帧长度时 `reuse_paint` 直接切片 panic（实测
   `range end index 292 out of range for slice of length 272`）。现在
   命中时像 `AnyView` 一样，在回放前后取一次 `prepaint_index()` /
   `paint_index()` 并把区间重定位到本帧追加的位置。
   `Window` 同时增加单调帧序号（`draw` 时自增），缓存记录区间所在的
   帧号；只有“上一帧刚回放过”才允许命中，否则重建子树。这样即使
   某帧被外层复用跳过（元素状态被外层区间带过），也不会拿更早的帧
   的区间去回放别的条目。
13. **复用区间夹取**（`src/window.rs`）：`reuse_prepaint` / `reuse_paint`
   对 hitboxes、tooltip、deferred draws、dispatch tree、cursor styles、
   input handlers、mouse listeners、element states、tab stop、scene
   等所有切片都先按上一帧实际长度夹取，越界时（例如区间来自更早的帧）
   只少复用条目并打一条 `gpui::window` debug 日志，不再 panic。

## 场景级分组滤镜

播放器右上「更多」与底部进度/传输/音量/底栏需要作为两个整体淡出，但上游
没有内容滤镜，也没有元素级 transform。`src/scene.rs` 新增公开
`PaintFilter { opacity, scale, origin, blur_radius }` 与场景检查点：

- `Window::paint_with_filter` 只处理当前子树新增的 `paint_operations` 和
  primitive 数组；围绕 `origin` 统一缩放、乘 alpha，并在 `blur_radius > 0`
  时以 8 个低 alpha 偏移样本重放绘制操作。
- 模糊是场景层的多重采样，不引入离屏纹理或平台 shader；阴影与平台
  surface 只变换原图，不参与重复采样。
- `Window::prepaint_with_filter` 用相同缩放/锚点变换该子树新增的 hitbox
  与 window-control hitbox，不复制监听器、焦点或 dispatch 节点。
- `src/scene.rs` 末尾 `filter_tests` 覆盖锚点不位移、alpha 相乘、零模糊不
  复制、八样本数量与 layer 配对。

## 重新应用

- `Cargo.toml` 末尾追加了空 `[workspace]`：让 vendored 包可以独立
  `cargo test`（平滑滚动等单测），同时仍可作为主工作区的 path 依赖；
- 以 `crates.io` 的 gpui 0.2.2 源码为基准，按上面改动打补丁；
- 保持 `blur_radius` / `subpixel_positioning` 在 `RenderGlyphParams` 中的
  字段顺序/hash 一致即可，图集 key 不依赖字段顺序；
- 扩大的 raster bounds 依赖平台光栅器接受“比字形更大、带零边距的矩形”；
  Windows/DirectWrite 路径已验证（`CreateAlphaTexture` 直接按矩形拷贝），
  macOS/Linux 路径尚未在本机验证；
- 若上游以后提供原生内容模糊 / 变换字形 API，删除 `[patch.crates-io] gpui` 和
  `vendor/gpui`，把 `render.rs` 的 `paint_glyph_blurred` 指向新 API。

## Android 平台补丁

- `src/platform/android/text_system.rs`：`CosmicTextSystem::new` 在
  `FontSystem::new()` 之后手动 `db_mut().load_fonts_dir("/system/fonts")`。
  fontdb 的 `load_system_fonts` 在 unix 分支显式排除了 Android，不手动扫描
  时字体库为空，`.SystemUIFont`（Android 映射到 Roboto）与整个回退栈都会
  在 `resolve_font` panic。

- `src/platform/android/window.rs`：`on_touch` 把 MotionEvent 的物理像素除以
  `state.scale` 再交给 gpui。gpui 的窗口坐标是逻辑像素，不换算时所有点击都会
  落在窗口外，UI 看得到但点不动。
