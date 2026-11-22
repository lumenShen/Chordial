# gpui 0.2.2 本地补丁

上游 crate：`gpui 0.2.2`（Zed，crates.io）。这里加了两项歌词渲染需要的能力：
**字形模糊**与**平滑字形定位/缩放**，并保持 `paint_glyph` 的行为不变。
升级 gpui 时需要把下面几处重新应用。

## 动机

gpui 没有内容滤镜（`filter: blur()`），`paint_glyph` 只能画未模糊的字形位图。
歌词行原先用「同一字形按稀疏偏移重复画 9 次」近似高斯，本质是 delta 核 +
source-over 叠加，σ≥2 时重影明显、内部 alpha 只有约 0.66。

另外，`paint_glyph` 把字形起点 `floor()` 到整个设备像素，且 Windows/Linux 的
纵向子像素档位为 1。静态 UI 文本这样最清晰；但歌词每帧都在弹簧驱动下做
亚像素移动，整像素吸附会表现为「走几帧不动、然后跳 1px」。同时行缩放若用
`font_size × scale` 每帧重新光栅化，DirectWrite 在每个分数字号下的 hinting
都会变化，导致抗锯齿和字形高度在帧间抖动，并让字形图集按连续字号膨胀。

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
4. `src/text_system.rs` 末尾 `#[cfg(test)] mod blur_alpha_tests`（参考用；
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

## 重新应用

- 以 `crates.io` 的 gpui 0.2.2 源码为基准，按上面改动打补丁；
- 保持 `blur_radius` / `subpixel_positioning` 在 `RenderGlyphParams` 中的
  字段顺序/hash 一致即可，图集 key 不依赖字段顺序；
- 扩大的 raster bounds 依赖平台光栅器接受“比字形更大、带零边距的矩形”；
  Windows/DirectWrite 路径已验证（`CreateAlphaTexture` 直接按矩形拷贝），
  macOS/Linux 路径尚未在本机验证；
- 若上游以后提供原生内容模糊 / 变换字形 API，删除 `[patch.crates-io] gpui` 和
  `vendor/gpui`，把 `render.rs` 的 `paint_glyph_blurred` 指向新 API。
