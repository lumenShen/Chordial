//! paint（由 render.rs 拆分，行为不变）。

use super::*;
use super::emphasis::*;
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_lyric_line(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    line: &LyricLine,
    shape: &LineShape,
    line_frame: LineFrameState,
    style: &LyricTextStyle,
    colors: LyricRenderColors,
    time: u64,
    is_dynamic: bool,
    fade: LyricFade,
) -> u64 {
    let scale = (line_frame.scale / 100.0).max(0.01);
    let line_top = bounds.top() + px(line_frame.top);
    let text_left = bounds.left() + shape.padding_x;
    let text_right = bounds.right() - shape.padding_x;
    // `.lyricLine` 的 `padding: 0.2em; margin: -0.2em` 把缩放盒向外扩 0.2em，
    // `transform-origin: left/right` 因此落在文本内容边缘外 0.2em。em 使用
    // 该行自己的字号（背景行是 0.7em）。
    let origin_inset = px(f32::from(shape.main_font_size) * 0.2);
    let pivot_x = if line.is_duet {
        text_right + origin_inset
    } else {
        text_left - origin_inset
    };
    let pivot_y = line_top + shape.height * 0.5;
    let xform = LineXform {
        scale,
        pivot_x,
        pivot_y,
        // 切换动画只缩放内容：围绕歌词容器中心，不碰容器/背景。
        content_scale: fade.content_scale,
        content_cx: bounds.left() + bounds.size.width * 0.5,
        content_cy: bounds.top() + bounds.size.height * 0.5,
    };
    let mut y = line_top + shape.padding_top;
    let mut painted = 0u64;

    // 与 AMLL 的 --bright/dark-mask-alpha 一致：由缩放弹簧驱动。
    let k = ((line_frame.scale / 100.0 - 0.97) / 0.03).clamp(0.0, 1.0);
    let bright = k * 0.8 + 0.2;
    let dark = k * 0.2 + 0.2;
    // 整行扫光的渐变带宽度。亮暗不再由每个词自己的窄边决定，而是一条
    // 覆盖整行的 mask 前沿；软边取 1.5em 左右，才能同时压住相邻的字/词，
    // 让渐变看起来是在整行上扫过，而不是缩在单个字形里。
    let fade_em = style.sweep_fade_em.max(0.0001);
    let line_enabled = line_frame.enabled;
    // AMLL `.lyricBgLine`：背景行 wrapper 透明度 0.4。
    let wrapper_alpha = if line.is_bg { 0.4 } else { 1.0 };
    // fade.opacity 是整层的切换透明度；行自身的 AMLL 透明度照旧保留。
    let opacity = line_frame.opacity * wrapper_alpha * fade.opacity;
    let blur = line_frame.blur;

    // AMLL 歌词容器的 CSS mask：上下各按容器高度比例渐隐（竖向布局底部为 0）。
    let container_h = f32::from(bounds.size.height);
    let fade_top = container_h * style.edge_fade_top.max(0.0);
    let fade_bottom = container_h * style.edge_fade_bottom.max(0.0);
    let edge_for = |row_top: Pixels, row_h: f32| -> f32 {
        // 渐隐按字形在屏幕上的实际中心计算（行缩放围绕行框中心）。
        let center = f32::from(xform.y(row_top + px(row_h * 0.5)) - bounds.top());
        let mut a = 1.0f32;
        if fade_top > 0.5 {
            a *= (center / fade_top).clamp(0.0, 1.0);
        }
        if fade_bottom > 0.5 {
            a *= ((container_h - center) / fade_bottom).clamp(0.0, 1.0);
        }
        a
    };

    for row in &shape.main {
        // AMLL 默认 align-items: flex-start；对唱行切到 flex-end。
        let row_origin_x = if line.is_duet {
            text_right - row.width
        } else {
            text_left
        };
        let edge = edge_for(y, f32::from(row.height));
        // 渐隐区/极低透明度行整行跳过：省掉全部字形绘制。
        if opacity * edge >= 0.004 {
            painted += paint_row(
                window,
                row,
                row_origin_x,
                y,
                xform,
                row.line.font_size,
                line,
                time,
                is_dynamic,
                bright,
                dark,
                f32::from(row.line.font_size) * fade_em,
                opacity,
                blur,
                edge,
                colors.text,
                colors.glow,
                &shape.emphasis_chunks,
                &shape.word_chunks,
                line_enabled,
                fade,
            );
        }
        y += row.height;
    }

    for row in &shape.translation {
        // AMLL 默认 align-items: flex-start；对唱行切到 flex-end。
        let row_origin_x = if line.is_duet {
            text_right - row.width
        } else {
            text_left
        };
        let edge = edge_for(y, f32::from(row.height));
        if opacity * edge >= 0.004 {
            painted += paint_row_plain(
                window,
                row,
                row_origin_x,
                y,
                xform,
                row.line.font_size,
                colors.translation.opacity(0.3),
                opacity,
                blur,
                edge,
                fade,
            );
        }
        y += row.height;
    }
    for row in &shape.roman {
        // AMLL 默认 align-items: flex-start；对唱行切到 flex-end。
        let row_origin_x = if line.is_duet {
            text_right - row.width
        } else {
            text_left
        };
        let edge = edge_for(y, f32::from(row.height));
        if opacity * edge >= 0.004 {
            painted += paint_row_plain(
                window,
                row,
                row_origin_x,
                y,
                xform,
                row.line.font_size,
                colors.translation.opacity(0.3),
                opacity,
                blur,
                edge,
                fade,
            );
        }
        y += row.height;
    }

    painted
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_row(
    window: &mut Window,
    row: &ShapedRow,
    origin_x: Pixels,
    row_top: Pixels,
    xform: LineXform,
    font_size: Pixels,
    line: &LyricLine,
    time: u64,
    is_dynamic: bool,
    bright: f32,
    dark: f32,
    fade: f32,
    opacity: f32,
    blur: f32,
    edge: f32,
    base_color: Hsla,
    glow_color: Hsla,
    chunks: &[EmphasisChunk],
    word_chunks: &[Option<usize>],
    line_enabled: bool,
    switch_fade: LyricFade,
) -> u64 {
    let line_height = f32::from(row.height);
    let ascent = f32::from(row.line.ascent);
    let descent = f32::from(row.line.descent);
    // 行内基线（未缩放坐标）；行级缩放统一由 `xform` 施加。
    let baseline = row_top + px((line_height - ascent - descent) / 2.0 + ascent);
    // AMLL 的强调动画作用在字符 `<span>` 上，未设置 transform-origin，
    // 因此锚点是字符盒（宽 = advance，高 = line-height）的中心。
    let center_y = row_top + row.height * 0.5;
    let mut painted = 0u64;
    let mut word_cursor = 0usize;
    let text = row.line.text.clone();
    let font_px = f32::from(font_size);
    // 切换动画的模糊按本行字号从 em 换算成 px，与行自身的 AMLL 模糊叠加。
    let blur_px = blur + switch_fade.blur_em * font_px;

    // AMLL `initFloatAnimation`：每个词在自己的时长内上浮 0.05em（背景行 0.1em），
    // 行离场时以 playbackRate = -1 回退。先按词缓存，避免逐字形重复计算。
    // 非逐词歌词（isNonDynamic）在 AMLL 里直接渲染整行文本，不创建 word span，
    // 因此也不会有浮动动画。
    //
    // 每帧不再为一行分配 `Vec<f32>`（300Hz × 可见行 = 大量短命分配）：
    // 字形按 word_cursor 单调推进，遇到新词才算一次并保存在标量里。
    let mut float_offset = 0.0f32;
    let mut float_word: Option<usize> = None;
    let mut last_span: Option<(usize, usize)> = None;
    let mut next_char_ordinal = 0usize;

    // 整行一条扫光：所有字形共享同一个前沿，渐变带才能跨字、跨词连续推进。
    // 背景行按 AMLL 走均匀 0.4 alpha，不参与扫光。
    let sweep = if is_dynamic && !line.is_bg {
        row_sweep_bounds(&row.words, line, time, fade)
    } else {
        None
    };

    for (run_index, run) in row.line.runs.iter().enumerate() {
        for (glyph_index, glyph) in run.glyphs.iter().enumerate() {
            while word_cursor + 1 < row.words.len()
                && glyph.index >= row.words[word_cursor].byte_range.end
            {
                word_cursor += 1;
            }
            let span = row
                .words
                .get(word_cursor)
                .filter(|w| w.byte_range.contains(&glyph.index));

            // 强调动画的 stagger 序号按“词内非空白字符”连续计数；同一个词跨
            // 视觉行时由 `WordSpan::char_offset` 接续。
            let mut char_ordinal = None;
            if let Some(span) = span {
                let key = (span.word_index, span.byte_range.start);
                if last_span != Some(key) {
                    last_span = Some(key);
                    next_char_ordinal = span.char_offset;
                }
                char_ordinal = Some(next_char_ordinal);
                if text
                    .get(glyph.index..)
                    .and_then(|s| s.chars().next())
                    .is_some_and(|c| !c.is_whitespace())
                {
                    next_char_ordinal += 1;
                }
            }

            // 强调词使用 char span 自己的 float 动画（含 stagger 和 sin 曲线），
            // 不再叠加 word 级别的 float；非强调词才走 word float。
            let chunk = span.and_then(|span| {
                word_chunks
                    .get(span.word_index)
                    .copied()
                    .flatten()
                    .and_then(|index| chunks.get(index))
            });
            let emphasis_word = chunk.is_some_and(|chunk| chunk.emphasized);
            if is_dynamic && !emphasis_word {
                let word_index = span.map(|span| span.word_index);
                if float_word != word_index {
                    float_word = word_index;
                    float_offset = word_index
                        .and_then(|index| line.words.get(index))
                        .map(|word| word_float_offset(word, line, time, line_enabled, font_px))
                        .unwrap_or(0.0);
                }
            }
            let float_y = if span.is_some() && !emphasis_word {
                float_offset
            } else {
                0.0
            };

            let glyph_x = origin_x + glyph.position.x;
            let next_x = run
                .glyphs
                .get(glyph_index + 1)
                .map(|g| g.position.x)
                .or_else(|| {
                    row.line
                        .runs
                        .get(run_index + 1)?
                        .glyphs
                        .first()
                        .map(|g| g.position.x)
                });
            let advance = next_x.map_or(row.width - glyph.position.x, |x| x - glyph.position.x);
            let advance = if advance > px(0.01) {
                advance
            } else {
                font_size * 0.5
            };
            let center_x = glyph_x + advance * 0.5;

            // 内容缩放叠加在行弹簧缩放之上：xform 已按它平移字形中心，
            // 这里同倍率缩放字形本身，整体等价于对内容做一次 scale。
            let mut glyph_scale = xform.scale * xform.content_scale;
            let mut off_x = px(0.0);
            let mut off_y = px(float_y);
            let mut ha = 1.0f32;
            // AMLL 的 `mask-image` 扫光：一条带软边界的水平渐变作为**整行**
            // 的 mask 层。`row_sweep_bounds` 每行只算一次，所有字形共用同一
            // 组边界，因此边界可以落在两个字/词之间；alpha 仍由 shader 按
            // fragment x 插值，同一个字形内部也是连续渐变。
            let mut mask = SpriteMask::NONE;
            if let Some((light_x, dark_x)) = sweep {
                mask = SpriteMask::new(
                    f32::from(xform.x(origin_x + px(light_x))),
                    f32::from(xform.x(origin_x + px(dark_x))),
                    bright,
                    dark,
                );
            } else if is_dynamic {
                // 动态行但本视觉行没有可用的词时间轴（例如纯空白行）：
                // 保持 AMLL 的整块暗层。
                ha = dark;
            }
            let mut glow = 0.0f32;
            let mut glow_sigma_em = 0.0f32;

            // 强调词：按强调分组的合并时序逐字 scale/translate/glow。
            if let (Some(ordinal), Some(chunk)) = (char_ordinal, chunk)
                && let Some(frame) =
                    emphasize_glyph(chunk, ordinal, line, time, line_enabled, font_px)
            {
                glyph_scale = xform.scale * xform.content_scale * frame.scale;
                off_x = px(frame.offset_x);
                off_y = px(frame.offset_y + frame.float_y);
                glow = frame.glow;
                glow_sigma_em = frame.glow_sigma_em;
            }

            // 背景人声行：AMLL SOLID 模式无逐词遮罩，统一 0.4 alpha
            //（乘上 wrapper 的 0.4 之前；wrapper 已在 opacity 里）。
            if line.is_bg {
                ha = 0.4;
                mask = SpriteMask::NONE;
            }

            let color = Hsla {
                a: base_color.a * (ha * opacity * edge).clamp(0.0, 1.0),
                ..base_color
            };
            // 先围绕字符盒中心做强调缩放，再叠加行级仿射变换：
            //   final(q) = L(C) + (line_scale * emp_scale) * (q + offset - C)
            // 与 AMLL 的「字符 span scale(默认中心 origin) → 行元素 scale(left/right center)」
            // 组合结果一致。字形始终用基准字号光栅化，缩放由 sprite 变换承担。
            let origin = transformed_glyph_origin(
                xform,
                glyph_x,
                baseline,
                center_x,
                center_y,
                glyph_scale,
                point(off_x, off_y),
            );
            let gx = origin.x;
            let gy = origin.y;

            if glow > 0.01 {
                // AMLL 强调词用 CSS `text-shadow: 0 0 R rgba(...)`；R 是模糊
                // 半径，浏览器按高斯 σ = R/2 实现。这里直接绘制一份真模糊的
                // 字形（vendor/gpui 在光栅化阶段卷积 alpha 掩码），与行级滤镜
                // 模糊在方差域叠加；比“稀疏复制 9 次”更接近真实辉光。
                let shadow_sigma = (font_px * glow_sigma_em * 0.5).max(0.0);
                let sigma = (blur_px * blur_px + shadow_sigma * shadow_sigma).sqrt();
                // mask 作用于整个词元素（含 text-shadow），所以辉光也要乘上
                // 当前字形的亮/暗层 alpha。
                let shadow_color = Hsla {
                    a: glow_color.a * (glow * ha * opacity * edge).clamp(0.0, 1.0),
                    ..glow_color
                };
                painted += paint_glyph_blurred(
                    window,
                    point(gx, gy),
                    run.font_id,
                    glyph.id,
                    glyph.is_emoji,
                    font_size,
                    glyph_scale,
                    shadow_color,
                    sigma,
                    mask,
                );
            }

            painted += paint_glyph_blurred(
                window,
                point(gx, gy),
                run.font_id,
                glyph.id,
                glyph.is_emoji,
                font_size,
                glyph_scale,
                color,
                blur_px,
                mask,
            );
        }
    }
    painted
}

/// AMLL `initFloatAnimation` 的等价相位。
///
/// - 正放：`ease-out` 在 `max(1s, 词时长)` 内把词上移 0.05em（背景行 0.1em）；
/// - 行离开热行集合时，AMLL 把动画 `playbackRate` 设为 -1 回退。这里用
///   “离场时间 / 词时长”还原同一轨迹，并在 0 处截断。
pub(super) fn word_float_offset(
    word: &LyricWord,
    line: &LyricLine,
    time: u64,
    enabled: bool,
    font_size: f32,
) -> f32 {
    let duration = word.duration_ms().max(1000) as f32;
    let phase = if time < line.start_time {
        0.0
    } else if enabled {
        ((time.saturating_sub(word.start_time)) as f32 / duration).clamp(0.0, 1.0)
    } else {
        // 刚离场时从“行结束时刻的相位”开始倒放。
        let at_end =
            ((line.end_time.saturating_sub(word.start_time)) as f32 / duration).clamp(0.0, 1.0);
        (at_end - (time.saturating_sub(line.end_time)) as f32 / duration).clamp(0.0, 1.0)
    };
    let up = if line.is_bg { 0.1 } else { 0.05 };
    -up * font_size * ease_out(phase)
}

/// CSS `ease-out` = `cubic-bezier(0, 0, 0.58, 1)`。
pub(super) fn ease_out(x: f32) -> f32 {
    cubic_bezier(0.0, 0.0, 0.58, 1.0, x)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_row_plain(
    window: &mut Window,
    row: &ShapedRow,
    origin_x: Pixels,
    row_top: Pixels,
    xform: LineXform,
    font_size: Pixels,
    color: Hsla,
    opacity: f32,
    blur: f32,
    edge: f32,
    switch_fade: LyricFade,
) -> u64 {
    let line_height = f32::from(row.height);
    let base = row_top
        + px(
            (line_height - f32::from(row.line.ascent) - f32::from(row.line.descent)) / 2.0
                + f32::from(row.line.ascent),
        );
    let color = Hsla {
        a: color.a * opacity * edge,
        ..color
    };
    let blur_px = blur + switch_fade.blur_em * f32::from(font_size);
    let glyph_scale = xform.scale * xform.content_scale;
    let mut painted = 0;
    for run in &row.line.runs {
        for glyph in &run.glyphs {
            painted += paint_glyph_blurred(
                window,
                point(xform.x(origin_x + glyph.position.x), xform.y(base)),
                run.font_id,
                glyph.id,
                glyph.is_emoji,
                font_size,
                glyph_scale,
                color,
                blur_px,
                SpriteMask::NONE,
            );
        }
    }
    painted
}

/// 字形基线原点的最终屏幕坐标。
///
/// `scale` = 行弹簧缩放 × 强调缩放；`center` 是字符盒中心（未缩放行内坐标），
/// 因为 AMLL 强调动画作用在字符 span 上且未设置 `transform-origin`，默认锚点
/// 是 `50% 50%`。行级缩放围绕 `xform` 的 left/right-center 锚点。
pub(super) fn transformed_glyph_origin(
    xform: LineXform,
    glyph_x: Pixels,
    baseline: Pixels,
    center_x: Pixels,
    center_y: Pixels,
    scale: f32,
    offset: gpui::Point<Pixels>,
) -> gpui::Point<Pixels> {
    point(
        xform.x(center_x) + (glyph_x - center_x + offset.x) * scale,
        xform.y(center_y) + (baseline - center_y + offset.y) * scale,
    )
}

/// 绘制一个字形。
///
/// `font_size` 是视觉行整形时使用的**基准字号**；`glyph_scale`（行弹簧缩放
/// × 强调缩放）通过 sprite 变换应用，而不是改变光栅化字号 —— 弹簧动画每帧
/// 都会产生新的分数字号，如果拿它当字形大小学会触发每帧重新光栅化，使
/// DirectWrite 的 hinting/抗锯齿在帧间变化（视觉上就是"抖"和"糊"）。
///
/// 模糊是字形光栅化阶段对 alpha 掩码做的真卷积（见 vendor/gpui）：每个
/// `(字形, 基准字号, σ 档位)` 只产生一次 sprite，位置的小数部分由变换矩阵
/// 平移承担；emoji 不走单色掩码路径。
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_glyph_blurred(
    window: &mut Window,
    origin: gpui::Point<Pixels>,
    font_id: gpui::FontId,
    glyph_id: gpui::GlyphId,
    is_emoji: bool,
    font_size: Pixels,
    glyph_scale: f32,
    color: Hsla,
    blur: f32,
    mask: SpriteMask,
) -> u64 {
    if is_emoji {
        // 彩色 emoji 无单色掩码，也没有 sprite 变换路径；按缩放后的字号
        // 重新光栅化（歌词里 emoji 很少，成本可忽略）。遮罩无法作用到彩色
        // 图元上，保持原样。
        let _ = window.paint_emoji(
            origin,
            font_id,
            glyph_id,
            px(f32::from(font_size) * glyph_scale),
        );
        return 1;
    }
    let blur = if blur > 0.05 { px(blur) } else { px(0.0) };
    let _ = window.paint_glyph_scaled_masked(
        origin,
        font_id,
        glyph_id,
        font_size,
        color,
        blur,
        glyph_scale,
        mask,
    );
    1
}

/// 整行扫光的渐变边界（行内坐标，px）。
///
/// 返回 `(light_from, dark_from)`：`light_from` 左侧完全是 `bright`，
/// `dark_from` 右侧完全是 `dark`，中间按 fragment x 线性过渡。
/// **一行只有一个前沿**，所有字形共用这组边界；前沿随时间从行首推进到
/// 行尾，渐变带因此会压在两个词/两个字之间，而不是每个词各自重新点亮。
///
/// 时间轴：每个词的播放时间内，前沿从上一词尾（行首词从 `首词起点 - fade`）
/// 推进到本词词尾；词与词之间的停顿前沿保持不动，空白处不会出现跳变。
/// 所有词结束后整行处于亮侧。
pub(super) fn row_sweep_bounds(
    words: &[WordSpan],
    line: &LyricLine,
    time: u64,
    fade: f32,
) -> Option<(f32, f32)> {
    let mut prev_end_time = 0u64;
    let mut prev_x1: Option<f32> = None;
    for span in words {
        let Some(word) = line.words.get(span.word_index) else {
            continue;
        };
        let x0 = f32::from(span.x);
        let x1 = x0 + f32::from(span.width);
        // 重叠/乱序的时间戳按上一词尾夹紧，保证前沿单调向前。
        let start = word.start_time.max(prev_end_time);
        let end = word.end_time.max(start);
        let from = prev_x1.unwrap_or(x0 - fade);
        if time < start {
            // 停顿区间：停在上一词尾（行首则停在整行左侧之外）。
            return Some((from, from + fade));
        }
        if time <= end {
            let progress = if end > start {
                (time - start) as f32 / (end - start) as f32
            } else {
                1.0
            };
            let front = from + (x1 - from) * progress;
            return Some((front, front + fade));
        }
        prev_end_time = end;
        prev_x1 = Some(x1);
    }
    // 没有可用词段时返回 None（由调用方回退到整块暗层）；否则所有词都
    // 已播完，整行在亮侧。
    prev_x1.map(|front| (front, front + fade))
}

/// 强调词逐字动画的一帧结果（全部为逻辑像素 / 无量纲）。
pub(super) struct EmphasisFrame {
    pub(super) scale: f32,
    pub(super) offset_x: f32,
    pub(super) offset_y: f32,
    /// AMLL `emphasize-word-float` 的附加上浮（composite: add）。
    pub(super) float_y: f32,
    /// `text-shadow` 的 alpha = `empEasing(x) * blur`。
    pub(super) glow: f32,
    /// `text-shadow` 模糊半径（em）：`min(0.3, blur * 0.3)`。
    pub(super) glow_sigma_em: f32,
}
