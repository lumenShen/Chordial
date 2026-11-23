//! gpui 文本整形、逐词几何与按行 LRU 缓存。
//!
//! 渲染层（[`crate::render`]）按帧需要的是「这一行由哪几个视觉行组成、每行里
//! 每个词的 x / 宽度、每个视觉行的高度」。本模块把这些结果一次性算好并缓存，
//! 绘制时只做 `paint_glyph`，不再触发任何布局查询。
//!
//! 与 AMLL 的排版语义对应关系：
//! - 行上下 padding = 视口高度 × [`LyricTextStyle::padding_y_ratio`]（dom-slim 的 2vh）；
//! - 左右 padding = 主字号 × [`LyricTextStyle::padding_x_em`]（1em）；
//! - 主行行高 = 主字号 × [`LyricTextStyle::line_height_ratio`]（1.2em）；
//! - 译文/音译字号 = 主字号 × [`LyricTextStyle::sub_font_size_ratio`]，且不低于 10px，
//!   行高 = 副字号 × [`LyricTextStyle::sub_line_height_ratio`]（1.5em）；
//! - 背景行字号 = 主字号 × [`LyricTextStyle::bg_line_scale`]（0.7）。
//!
//! # 性能约定
//! - 每行只整形一次：`ensure` / `ensure_range` 命中缓存时 O(1)（LRU touch 为
//!   O(容量)，容量 ≤ 256）；淘汰只丢弃 [`ShapedRow`]，保留 [`TextLayoutCache::heights`]
//!   里的高度，播放器的前缀和不会因为淘汰而跳动。
//! - 换行是**单遍**的：先用一次整形得到整行每个字符的 x（[`CharXs`]），再按
//!   已测量的片段贪心折行；不会对每个候选宽度重复整形，也不会反复拼接字符串。
//! - 每次实际整形都埋 `amll.text.shape` 检查点（items = 字符数）。
//!
//! # gpui 文本 API 的坑
//! - `WindowTextSystem::shape_line` 的 `text` **不能包含 `'\n'`**（debug 断言），
//!   所以所有输入（含译文/音译）都先做空白归一化；
//! - 空串整形在部分平台（cosmic-text）可能产生 0 个视觉行而 panic，因此空视觉行
//!   一律使用 `ShapedLine::default()`（0 字形、0 宽度），渲染层不得依赖它的
//!   `font_size/ascent`；
//! - `LineLayout::x_for_index` 是 O(字形数) 的线性扫描，逐词/逐字调用会退化成
//!   O(n²)，本模块改用一次构建的字节 → x 索引 [`CharXs`]；
//! - `Pixels` 的字段是私有的：只能 `px(f32)` 构造、`f32::from(px)` 读取。

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::Arc;

use chordal_core::perf::PerfScope;
use gpui::{
    Font, FontWeight, LineLayout, Pixels, ShapedLine, SharedString, TextRun, Window, px, white,
};

use crate::model::{LyricDocument, LyricLine};

/// 实际整形埋点名称（预算表由 background 代理统一维护）。
const SHAPE_CHECKPOINT: &str = "amll.text.shape";

/// 默认最多缓存多少行的整形结果。
const DEFAULT_MAX_SHAPED_LINES: usize = 256;
/// `set_max_shaped_lines` 的下限，避免误配导致每帧全量重排。
const MIN_MAX_SHAPED_LINES: usize = 16;
/// 最小可读字号：主行/副行都不会低于这个值。
const MIN_FONT_SIZE: f32 = 10.0;
/// 视口尺寸未就绪时的兜底值（`ensure` 在首次 `set_viewport` 之前被调用时使用）。
const FALLBACK_VIEWPORT_WIDTH: f32 = 1280.0;
const FALLBACK_VIEWPORT_HEIGHT: f32 = 720.0;
/// 浮点比较/换行余量：避免亚像素误差把刚好放得下的片段挤到下一行。
const WRAP_EPSILON: f32 = 0.01;

/// 歌词文本样式（全部单位：逻辑像素或比例）。
///
/// `PartialEq` 用于视图层样式去重：沉浸层每帧都会重算样式，但只有真正
/// 变化（字号/字重/行高等）时才允许丢弃整形缓存。
#[derive(Clone, Debug, PartialEq)]
pub struct LyricTextStyle {
    /// 字体族名（gpui 的 `Font::family`）。
    pub font_family: SharedString,
    /// 字重。
    pub font_weight: FontWeight,
    /// 主行基准字号（背景行会再乘 [`Self::bg_line_scale`]）。
    pub base_font_size: f32,
    /// 主行行高相对字号的倍数。
    pub line_height_ratio: f32,
    /// 译文/音译字号相对主字号的倍数（结果不低于 10px）。
    pub sub_font_size_ratio: f32,
    /// 译文/音译行高相对其字号的倍数。
    pub sub_line_height_ratio: f32,
    /// 左右内边距，单位是「主字号」。
    pub padding_x_em: f32,
    /// 上下内边距，相对视口高度。
    pub padding_y_ratio: f32,
    /// 歌词可用宽度占视口的比例（与 [`crate::player::LyricPlayerConfig`] 对齐；
    /// 布局宽度本身按视口宽度计算，此字段供渲染层/配置保持一致）。
    /// 背景行字号相对主字号的倍数。
    pub bg_line_scale: f32,
    /// 整行扫光渐变带宽度（单位：主字号 em）。
    ///
    /// 动态歌词的亮暗由**一条覆盖整行**的 mask 前沿推进：所有字形共用同一
    /// 组边界，渐变带会压在相邻的字/词之间。这个值决定软边能横跨多少相邻
    /// 字形 —— 默认 0.6（与 AMLL 的 `wordFadeWidth` 一致）；调到 1.5 左右
    /// 会明显跨 1–2 个 CJK 字。
    pub sweep_fade_em: f32,
    /// 容器顶部渐隐比例（AMLL 歌词容器 CSS mask 的 `transparent → black 10%`）。
    pub edge_fade_top: f32,
    /// 容器底部渐隐比例（横向布局 mask 是 `black 90% → transparent`；
    /// 竖向布局 mask 到底不渐隐，设为 0）。
    pub edge_fade_bottom: f32,
}

impl Default for LyricTextStyle {
    fn default() -> Self {
        Self {
            font_family: ".SystemUIFont".into(),
            font_weight: FontWeight::NORMAL,
            base_font_size: 24.0,
            line_height_ratio: 1.2,
            sub_font_size_ratio: 0.5,
            sub_line_height_ratio: 1.5,
            padding_x_em: 1.0,
            padding_y_ratio: 0.02,
            bg_line_scale: 0.7,
            sweep_fade_em: 0.6,
            edge_fade_top: 0.1,
            edge_fade_bottom: 0.1,
        }
    }
}

/// 行内一个词的几何信息。
#[derive(Debug, Clone)]
pub struct WordSpan {
    /// 对应 `LyricLine.words` 的下标。
    pub word_index: usize,
    /// 在该视觉行文本中的字节范围（已按换行裁剪，可能只是原词的一部分）。
    pub byte_range: Range<usize>,
    /// 该词在本视觉行片段开始前已经渲染的非空白字符数。
    ///
    /// 一个词可能跨多个视觉行；强调动画的逐字序号必须按**整词**连续计数，
    /// 因此这里保存前缀偏移，渲染层只需再累加本行内的非空白字形。
    pub char_offset: usize,
    /// 词在该视觉行内的起始 x（行局部坐标）。
    pub x: Pixels,
    /// 词在该视觉行内占用的宽度。
    pub width: Pixels,
}

/// 一个强调分组（AMLL `chunkAndSplitLyricWords` 的等价物）。
///
/// AMLL 会先把词按空白 / CJK 字符切成 atom，再用 `Intl.Segmenter` 把没有空白的
/// 相邻 atom 合并成一个词段，最后以**合并后的整体时长**判断是否触发强调动画。
/// 网易云 YRC 常把一个英文单词拆成多段（`" su"` + `"gar"`），不合并就永远不会
/// 达到 1s 阈值；而 CJK 逐字歌词又不能简单合并，否则整句都会被强调。
#[derive(Debug, Clone)]
pub struct EmphasisChunk {
    /// 起始词下标（含）。
    pub word_start: usize,
    /// 结束词下标（不含）。
    pub word_end: usize,
    pub start_time: u64,
    pub end_time: u64,
    /// 是否触发强调词动画（缩放 + 位移 + 辉光）。
    pub emphasized: bool,
    /// 分组内的非空白字符数（用于 stagger 延迟与“≤7 字”规则）。
    pub char_count: usize,
    /// 每个词在分组内的首个非空白字符序号（长度 = `word_end - word_start`）。
    pub word_char_offsets: Vec<usize>,
}

/// 一个视觉行（自动换行后的片段）。
#[derive(Debug, Clone)]
pub struct ShapedRow {
    pub line: ShapedLine,
    pub words: Vec<WordSpan>,
    /// 该行实际宽度（最后一行是内容宽度，不是容器宽度）。
    pub width: Pixels,
    /// 该行行高（字号 × 行高倍数），不含行间 padding。
    pub height: Pixels,
    /// 是否为译文行。音译行是另一种副行，通过 [`LineShape::roman`] 区分，
    /// 这里的值为 `false`。
    pub is_translation: bool,
}

/// 一整行歌词的排版结果。
#[derive(Debug, Clone)]
pub struct LineShape {
    pub index: usize,
    /// 主歌词（含自动换行后的多个视觉行）。
    pub main: Vec<ShapedRow>,
    /// 译文视觉行；为空时是空 `Vec`。
    pub translation: Vec<ShapedRow>,
    /// 音译视觉行；为空时是空 `Vec`。
    pub roman: Vec<ShapedRow>,
    /// 含上下 padding 的总高度。
    pub height: Pixels,
    pub main_font_size: Pixels,
    pub sub_font_size: Pixels,
    pub padding_top: Pixels,
    pub padding_x: Pixels,
    /// 可用内容宽度（同一行内主/副文本共用）。
    pub content_width: Pixels,
    /// 强调分组（按词下标划分，覆盖整行；纯空白词不进入任何分组）。
    pub emphasis_chunks: Vec<EmphasisChunk>,
    /// 每个 `LyricLine.words` 下标所属的强调分组；`None` = 纯空白词。
    pub word_chunks: Vec<Option<usize>>,
}

/// 已测量、可参与换行的片段。
///
/// 主歌词按「词」切分；CJK 词与超长拉丁词会进一步按字符切分，使换行点更细。
/// 该类型与 [`wrap_fragments`] 一起构成可单测的换行核心。
#[derive(Debug, Clone, Copy)]
pub struct MeasuredFragment {
    /// 对应 `LyricLine.words` 下标（译文/音译为 0）。
    pub word_index: usize,
    /// 在整行文本中的字节范围。
    pub byte_start: usize,
    pub byte_end: usize,
    /// 在整行文本中的起始 x。
    pub x: Pixels,
    /// 片段宽度。
    pub width: Pixels,
    /// 是否允许在此片段之前换行。
    pub breakable_before: bool,
    /// 是否为空白片段（行首/行尾会被裁掉）。
    pub is_space: bool,
}

/// 贪心折行核心：返回每个视觉行占用的片段下标范围。
///
/// 规则（与 CSS `overflow-wrap: break-word` 接近）：
/// 1. 尽量放满 `max_width`；
/// 2. 放不下时，如果当前片段本身是合法断点（词首 / CJK 字符），就在这里断；
/// 3. 否则退回本行最近一个合法断点（把当前整个词移到下一行）；
/// 4. 整行都没有断点（超长拉丁词）时在当前位置硬断；
/// 5. 行首/行尾的空白片段会被裁掉；全空白输入返回一个空行（`0..0`）。
///
/// 复杂度 O(片段数)：断点回退时只会重扫移入下一行的片段，摊还线性。
pub fn wrap_fragments(fragments: &[MeasuredFragment], max_width: Pixels) -> Vec<Range<usize>> {
    let max_width = f32::from(max_width);
    wrap_fragments_f32(fragments, max_width)
}

/// 词级便捷版本：每个片段都是一个合法断点，返回每行的片段下标列表。
///
/// 与 [`wrap_fragments`] 的区别只在返回值形状；需要 CJK 逐字断行或超长词强断时
/// 请直接使用 [`wrap_fragments`]。
pub fn wrap_spans(spans: &[(usize, Range<usize>, Pixels)], max_width: Pixels) -> Vec<Vec<usize>> {
    let fragments: Vec<MeasuredFragment> = spans
        .iter()
        .map(|(word_index, range, width)| MeasuredFragment {
            word_index: *word_index,
            byte_start: range.start,
            byte_end: range.end,
            x: px(0.0),
            width: *width,
            breakable_before: true,
            is_space: false,
        })
        .collect();
    wrap_fragments(&fragments, max_width)
        .into_iter()
        .map(|row| row.collect())
        .collect()
}

fn wrap_fragments_f32(fragments: &[MeasuredFragment], max_width: f32) -> Vec<Range<usize>> {
    let n = fragments.len();
    if n == 0 {
        // 空输入也返回一个空视觉行，避免调用方为 0 行做特殊处理。
        return vec![Range::default()];
    }
    let max_width = if max_width.is_finite() {
        max_width.max(1.0)
    } else {
        1.0
    };

    let mut rows: Vec<Range<usize>> = Vec::with_capacity(n / 8 + 1);
    let mut i = 0usize;
    let mut row_start = 0usize;
    let mut row_width = 0.0f32;
    let mut has_content = false;
    // 本行最近一个合法断点（片段下标）以及断点之前已经占用的宽度。
    let mut candidate: Option<(usize, f32)> = None;

    while i < n {
        let frag = &fragments[i];
        let width = f32::from(frag.width).max(0.0);

        // 行首空白直接丢弃。
        if !has_content && frag.is_space {
            i += 1;
            row_start = i;
            continue;
        }
        // 空行上的第一个片段无条件放置（即使比可用宽度还宽，也允许溢出）。
        if !has_content {
            has_content = true;
            row_width = width;
            i += 1;
            continue;
        }
        if row_width + width <= max_width + WRAP_EPSILON {
            if frag.breakable_before {
                candidate = Some((i, row_width));
            }
            row_width += width;
            i += 1;
            continue;
        }

        // 溢出：当前片段本身是合法断点时，直接在此断行（保留整行已有内容）。
        if frag.breakable_before {
            let end = trim_row_end(fragments, row_start, i);
            debug_assert!(end > row_start, "row with content must be non-empty");
            rows.push(row_start..end);
            row_start = i;
            row_width = 0.0;
            has_content = false;
            candidate = None;
            continue;
        }

        // 否则退回最近一个断点（例如把放不下的整个词移到下一行）。
        if let Some((c, _)) = candidate.take().filter(|(c, _)| *c > row_start) {
            let end = trim_row_end(fragments, row_start, c);
            if end > row_start {
                rows.push(row_start..end);
            }
            i = c;
            row_start = c;
            row_width = 0.0;
            has_content = false;
            candidate = None;
            continue;
        }

        // 没有断点：硬断（超长拉丁词 / 连续无空白的字符）。
        let end = trim_row_end(fragments, row_start, i);
        if end > row_start {
            rows.push(row_start..end);
        }
        row_start = i;
        row_width = 0.0;
        has_content = false;
        candidate = None;
        // 不前进 i：下一轮把当前片段放进新行。
    }

    if has_content {
        let end = trim_row_end(fragments, row_start, n);
        if end > row_start {
            rows.push(row_start..end);
        }
    }
    if rows.is_empty() {
        rows.push(0..0);
    }
    rows
}

#[inline]
fn trim_row_end(fragments: &[MeasuredFragment], start: usize, mut end: usize) -> usize {
    while end > start && fragments[end - 1].is_space {
        end -= 1;
    }
    end
}

/// 字符级 x 索引：把 `LineLayout` 的字节 → x 查询从 O(字形数) 降到 O(log 字符数)。
///
/// 构建过程对字形序列只做一次单向扫描；之后按字节边界二分即可。
struct CharXs {
    /// (字符起始字节, 该字符的 x)，按字节升序。
    entries: Vec<(usize, f32)>,
    /// 整行宽度（用于最后一个字符之后的查询）。
    width: f32,
    /// 上一次查询的位置。调用方通常按字节升序访问，因此正向扫描是摊还 O(1) 的；
    /// 只有回退查询才二分。
    cursor: std::cell::Cell<usize>,
}

impl CharXs {
    fn build(text: &str, layout: &LineLayout) -> Self {
        let width = f32::from(layout.width);
        let mut entries = Vec::with_capacity(text.len() / 2 + 2);
        let mut run_i = 0usize;
        let mut glyph_i = 0usize;
        let runs = &layout.runs;

        for (byte, _) in text.char_indices() {
            // 前进到第一个 index >= byte 的字形（字形本来就按文本顺序排列）。
            loop {
                if run_i >= runs.len() {
                    break;
                }
                let run = &runs[run_i];
                if glyph_i < run.glyphs.len() {
                    if run.glyphs[glyph_i].index >= byte {
                        break;
                    }
                    glyph_i += 1;
                } else {
                    run_i += 1;
                    glyph_i = 0;
                }
            }
            let x = if run_i < runs.len() && glyph_i < runs[run_i].glyphs.len() {
                f32::from(runs[run_i].glyphs[glyph_i].position.x)
            } else {
                width
            };
            entries.push((byte, x));
        }

        Self {
            entries,
            width,
            cursor: std::cell::Cell::new(0),
        }
    }

    /// 字节边界处的 x；越界返回整行宽度。
    ///
    /// 游标只朝目标方向移动。本模块内部的查询都在同一 token / 同一行内往返，
    /// 总移动量是 O(字符数)（每个 token 最多来回三趟），因此整体仍是线性的。
    #[inline]
    fn x_at(&self, byte: usize) -> f32 {
        let entries = &self.entries;
        let len = entries.len();
        let mut idx = self.cursor.get().min(len);
        // 目标：第一个「起始字节 >= byte」的字符下标。
        loop {
            if idx < len && entries[idx].0 < byte {
                idx += 1;
            } else if idx > 0 && entries[idx - 1].0 >= byte {
                idx -= 1;
            } else {
                break;
            }
        }
        self.cursor.set(idx);
        if idx < len {
            entries[idx].1
        } else {
            self.width
        }
    }
}

/// 按行缓存的排版结果。
///
/// 内部结构固定为：
/// - `shapes`：按歌词行下标的 `Option<Arc<LineShape>>`；
/// - `heights`：每行总高度（未整形的是估算值），长度恒等于 `doc.lines.len()`；
/// - `lru`：整形过的行下标队列，队尾最近使用；每个下标只出现一次。
pub struct TextLayoutCache {
    style: LyricTextStyle,
    viewport_width: f32,
    viewport_height: f32,
    shapes: Vec<Option<Arc<LineShape>>>,
    /// 每行高度的估算元数据，用于视口/样式变化后不必重新读文档就能重算高度。
    hints: Vec<LineHint>,
    heights: Vec<f32>,
    lru: VecDeque<usize>,
    shaped_count: usize,
    max_shaped_lines: usize,
    generation: u64,
    /// 文档含对唱行时，AMLL 给所有行额外预留 15% 宽度（左/右内缩）。
    has_duet: bool,
}

/// 高度估算需要的轻量信息（不保存文本，避免重复占用歌词内存）。
#[derive(Debug, Clone, Copy, Default)]
struct LineHint {
    main_chars: u32,
    main_wide: u32,
    trans_chars: u32,
    roman_chars: u32,
    is_bg: bool,
}

impl LineHint {
    fn from_line(line: &LyricLine) -> Self {
        let mut main_chars = 0u32;
        let mut main_wide = 0u32;
        for word in &line.words {
            for ch in word.word.chars() {
                if ch.is_whitespace() || ch.is_control() {
                    continue;
                }
                main_chars = main_chars.saturating_add(1);
                if is_cjk(ch) {
                    main_wide = main_wide.saturating_add(1);
                }
            }
        }
        let count = |src: &str| -> u32 {
            src.chars()
                .filter(|ch| !ch.is_whitespace() && !ch.is_control())
                .count()
                .min(u32::MAX as usize) as u32
        };
        Self {
            main_chars,
            main_wide,
            trans_chars: count(&line.translated_lyric),
            roman_chars: count(&line.roman_lyric),
            is_bg: line.is_bg,
        }
    }
}

impl TextLayoutCache {
    pub fn new(style: LyricTextStyle) -> Self {
        Self {
            style,
            viewport_width: FALLBACK_VIEWPORT_WIDTH,
            viewport_height: FALLBACK_VIEWPORT_HEIGHT,
            shapes: Vec::new(),
            hints: Vec::new(),
            heights: Vec::new(),
            lru: VecDeque::new(),
            shaped_count: 0,
            max_shaped_lines: DEFAULT_MAX_SHAPED_LINES,
            generation: 0,
            has_duet: false,
        }
    }

    #[inline]
    pub fn style(&self) -> &LyricTextStyle {
        &self.style
    }

    /// 更新样式与视口。所有已整形行失效（原因：宽度/字号变化会改变换行）。
    pub fn set_style(&mut self, style: LyricTextStyle, viewport_width: f32, viewport_height: f32) {
        self.style = style;
        if viewport_width.is_finite() && viewport_width > 0.0 {
            self.viewport_width = viewport_width;
        }
        if viewport_height.is_finite() && viewport_height > 0.0 {
            self.viewport_height = viewport_height;
        }
        self.reset_shapes();
        self.recompute_heights();
    }

    /// 更新视口尺寸。尺寸不变时是空操作；变化时丢弃整形缓存并重算估算高度。
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return;
        }
        if (width - self.viewport_width).abs() < 0.01
            && (height - self.viewport_height).abs() < 0.01
        {
            return;
        }
        self.viewport_width = width;
        self.viewport_height = height;
        self.reset_shapes();
        self.recompute_heights();
    }

    /// 重置为一份新文档：清空整形缓存，按行生成高度估算。
    pub fn set_lyrics(&mut self, doc: &LyricDocument) {
        let n = doc.lines.len();
        self.has_duet = doc.has_duet;
        self.shapes.clear();
        self.shapes.resize_with(n, || None);
        self.lru.clear();
        self.shaped_count = 0;
        self.hints.clear();
        self.hints.reserve(n);
        for line in &doc.lines {
            self.hints.push(LineHint::from_line(line));
        }
        self.heights.clear();
        self.heights.reserve(n);
        for i in 0..n {
            let height = estimate_height(
                &self.style,
                self.viewport_width,
                self.viewport_height,
                self.has_duet,
                &self.hints[i],
            );
            self.heights.push(height);
        }
        self.generation = self.generation.wrapping_add(1);
    }

    /// 每行总高度（f32），长度 = `doc.lines.len()`。可作为播放器布局的前缀和输入。
    #[inline]
    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    /// 只读取得已整形行；不更新 LRU（由 [`Self::ensure`] 负责）。
    pub fn get(&self, index: usize) -> Option<Arc<LineShape>> {
        self.shapes.get(index).and_then(Option::as_ref).cloned()
    }

    /// 取得（必要时整形）某行。可能淘汰最久未使用的行；返回 `Arc` 克隆，避免
    /// 与缓存的可变借用冲突。
    pub fn ensure(
        &mut self,
        doc: &LyricDocument,
        index: usize,
        window: &mut Window,
    ) -> Option<Arc<LineShape>> {
        if index >= doc.lines.len() {
            return None;
        }
        // 调用方忘了 set_lyrics：按当前文档重建（只在长度不一致时发生）。
        if self.shapes.len() != doc.lines.len() {
            self.set_lyrics(doc);
        }
        if let Some(shape) = self.shapes[index].clone() {
            self.touch(index);
            return Some(shape);
        }

        let shape = Arc::new(build_line_shape(
            &self.style,
            self.viewport_width,
            self.viewport_height,
            self.has_duet,
            index,
            &doc.lines[index],
            window,
        ));
        self.heights[index] = f32::from(shape.height);
        self.store(index, shape.clone());
        Some(shape)
    }

    /// 批量确保一个区间已整形（只对当前可见范围调用）。
    pub fn ensure_range(&mut self, doc: &LyricDocument, range: Range<usize>, window: &mut Window) {
        if doc.lines.is_empty() {
            return;
        }
        let n = doc.lines.len();
        let start = range.start.min(n);
        let end = range.end.min(n);
        for i in start..end {
            if self.ensure(doc, i, window).is_none() {
                break;
            }
        }
    }

    #[inline]
    pub fn shaped_count(&self) -> usize {
        self.shaped_count
    }

    #[inline]
    pub fn max_shaped_lines(&self) -> usize {
        self.max_shaped_lines
    }

    /// 设置缓存行数上限（最小 16）。缩小时立即按 LRU 淘汰。
    pub fn set_max_shaped_lines(&mut self, cap: usize) {
        let cap = cap.max(MIN_MAX_SHAPED_LINES);
        if cap == self.max_shaped_lines {
            return;
        }
        self.max_shaped_lines = cap;
        self.evict_to_cap();
    }

    /// 丢弃全部整形结果（保留高度估算）。
    pub fn clear(&mut self) {
        self.reset_shapes();
    }

    /// 缓存布局的世代号：样式/视口/文档变化时递增，用于渲染层判断是否需要重建
    /// 与具体行内容无关的资源。
    #[inline]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    // ---- 内部维护 ----

    fn reset_shapes(&mut self) {
        for slot in &mut self.shapes {
            *slot = None;
        }
        self.lru.clear();
        self.shaped_count = 0;
        self.generation = self.generation.wrapping_add(1);
    }

    fn recompute_heights(&mut self) {
        self.heights.clear();
        self.heights.reserve(self.hints.len());
        for i in 0..self.hints.len() {
            let height = estimate_height(
                &self.style,
                self.viewport_width,
                self.viewport_height,
                self.has_duet,
                &self.hints[i],
            );
            self.heights.push(height);
        }
    }

    fn touch(&mut self, index: usize) {
        if let Some(pos) = self.lru.iter().position(|&i| i == index) {
            self.lru.remove(pos);
        }
        self.lru.push_back(index);
    }

    fn store(&mut self, index: usize, shape: Arc<LineShape>) {
        if self.shapes[index].is_none() {
            self.shaped_count += 1;
        }
        self.shapes[index] = Some(shape);
        self.touch(index);
        self.evict_to_cap();
    }

    fn evict_to_cap(&mut self) {
        while self.shaped_count > self.max_shaped_lines {
            let Some(victim) = self.lru.pop_front() else {
                debug_assert!(false, "LRU queue shorter than shaped_count");
                break;
            };
            if self.shapes.get(victim).is_some_and(|slot| slot.is_some()) {
                self.shapes[victim] = None;
                self.shaped_count -= 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 字符分类
// ---------------------------------------------------------------------------

/// 是否为 CJK / 全角字符（这些字符之间允许逐字换行，且宽度约为 1em）。
fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF      // Hangul Jamo
        | 0x2E80..=0x2FDF    // CJK Radicals / Kangxi
        | 0x2FF0..=0x2FFF    // Ideographic description
        | 0x3000..=0x303F    // CJK Symbols and Punctuation
        | 0x3040..=0x30FF    // Hiragana / Katakana
        | 0x3100..=0x312F    // Bopomofo
        | 0x3130..=0x318F    // Hangul Compatibility Jamo
        | 0x31A0..=0x31BF    // Bopomofo Extended
        | 0x31F0..=0x31FF    // Katakana Phonetic Extensions
        | 0x3400..=0x4DBF    // CJK Extension A
        | 0x4E00..=0x9FFF    // CJK Unified Ideographs
        | 0xA960..=0xA97F    // Hangul Jamo Extended-A
        | 0xAC00..=0xD7AF    // Hangul Syllables
        | 0xF900..=0xFAFF    // CJK Compatibility Ideographs
        | 0xFE10..=0xFE1F    // Vertical forms
        | 0xFE30..=0xFE4F    // CJK Compatibility Forms
        | 0xFF00..=0xFFEF    // Halfwidth/Fullwidth Forms
        | 0x1B000..=0x1B16F  // Kana Supplement / Extended
        | 0x20000..=0x2FA1F  // CJK Extension B+
        | 0x30000..=0x3134F  // CJK Extension G
    )
}

// ---------------------------------------------------------------------------
// 度量
// ---------------------------------------------------------------------------

#[inline]
fn main_font_size(style: &LyricTextStyle, is_bg: bool) -> f32 {
    let scale = if is_bg {
        style.bg_line_scale.max(0.05)
    } else {
        1.0
    };
    let size = style.base_font_size.max(0.0) * scale;
    if size.is_finite() {
        size.max(MIN_FONT_SIZE)
    } else {
        MIN_FONT_SIZE
    }
}

#[inline]
fn sub_font_size(style: &LyricTextStyle, main: f32) -> f32 {
    let size = main * style.sub_font_size_ratio.max(0.05);
    if size.is_finite() {
        size.max(MIN_FONT_SIZE)
    } else {
        MIN_FONT_SIZE
    }
}

#[inline]
fn padding_x(style: &LyricTextStyle, main: f32) -> f32 {
    (main * style.padding_x_em.max(0.0)).max(0.0)
}

#[inline]
fn padding_top(style: &LyricTextStyle, viewport_height: f32) -> f32 {
    (viewport_height.max(0.0) * style.padding_y_ratio.max(0.0)).max(0.0)
}

#[inline]
fn content_width(viewport_width: f32, padding_x: f32, has_duet: bool) -> f32 {
    let reserve = if has_duet { viewport_width * 0.15 } else { 0.0 };
    (viewport_width - 2.0 * padding_x - reserve).max(1.0)
}

fn estimate_rows(chars: u32, wide: u32, font_size: f32, width: f32) -> usize {
    if chars == 0 {
        return 0;
    }
    let narrow = chars.saturating_sub(wide) as f32;
    // 拉丁字符平均约 0.55em，CJK 约 1em。
    let estimated = (narrow * 0.55 + wide as f32) * font_size;
    let rows = (estimated / width.max(1.0)).ceil();
    if rows.is_finite() {
        (rows as usize).max(1)
    } else {
        1
    }
}

/// 未整形行的估算高度：必须 > 0，播放器前缀和依赖它。
fn estimate_height(
    style: &LyricTextStyle,
    viewport_width: f32,
    viewport_height: f32,
    has_duet: bool,
    hint: &LineHint,
) -> f32 {
    let main = main_font_size(style, hint.is_bg);
    let sub = sub_font_size(style, main);
    let pad_x = padding_x(style, main);
    let width = content_width(viewport_width, pad_x, has_duet);
    let pad_y = padding_top(style, viewport_height);

    let main_rows = estimate_rows(hint.main_chars, hint.main_wide, main, width).max(1);
    let sub_rows = estimate_rows(hint.trans_chars, 0, sub, width)
        + estimate_rows(hint.roman_chars, 0, sub, width);

    let height = 2.0 * pad_y
        + main_rows as f32 * main * style.line_height_ratio.max(0.0)
        + sub_rows as f32 * sub * style.sub_line_height_ratio.max(0.0);
    if height.is_finite() {
        height.max(1.0)
    } else {
        1.0
    }
}

#[inline]
fn font_for(style: &LyricTextStyle) -> Font {
    let mut font = gpui::font(style.font_family.clone());
    font.weight = style.font_weight;
    font
}

// ---------------------------------------------------------------------------
// 单行排版
// ---------------------------------------------------------------------------

fn build_line_shape(
    style: &LyricTextStyle,
    viewport_width: f32,
    viewport_height: f32,
    has_duet: bool,
    index: usize,
    line: &LyricLine,
    window: &Window,
) -> LineShape {
    let main_size = main_font_size(style, line.is_bg);
    let sub_size = sub_font_size(style, main_size);
    let pad_x = padding_x(style, main_size);
    let pad_y = padding_top(style, viewport_height);
    let width = content_width(viewport_width, pad_x, has_duet);
    let font = font_for(style);

    let main = shape_main_rows(
        window,
        line,
        &font,
        main_size,
        width,
        style.line_height_ratio,
    );
    let sub_height = sub_size * style.sub_line_height_ratio.max(0.0);
    let translation = shape_sub_rows(
        window,
        &line.translated_lyric,
        &font,
        sub_size,
        width,
        sub_height,
        true,
    );
    let roman = shape_sub_rows(
        window,
        &line.roman_lyric,
        &font,
        sub_size,
        width,
        sub_height,
        false,
    );

    let mut height = 2.0 * pad_y
        + main.len() as f32 * main_size * style.line_height_ratio.max(0.0)
        + (translation.len() + roman.len()) as f32 * sub_height;
    if !height.is_finite() || height <= 0.0 {
        height = main_size.max(1.0);
    }

    let (emphasis_chunks, word_chunks) = build_emphasis_chunks(line);

    LineShape {
        index,
        main,
        translation,
        roman,
        height: px(height),
        main_font_size: px(main_size),
        sub_font_size: px(sub_size),
        padding_top: px(pad_y),
        padding_x: px(pad_x),
        content_width: px(width),
        emphasis_chunks,
        word_chunks,
    }
}

/// 构建强调分组：与 AMLL `chunkAndSplitLyricWords` 的**可见行为**等价。
///
/// - 英文等非 CJK 文本：没有空白的相邻词合并为一个分组，用合并时长判断是否
///   达到 1s、2..=7 个字符；同时保留 AMLL 的「其中任一词自身达标」规则。
/// - CJK 文本：AMLL 会先拆成单字 atom，因此一个多字词的强调阈值是**单字时长**
///   ≥ 1s，而不是整词时长。这里按每字均分时长近似。
///
/// 纯空白词不进入分组（AMLL 中它们是独立的文本节点）。
fn build_emphasis_chunks(line: &LyricLine) -> (Vec<EmphasisChunk>, Vec<Option<usize>>) {
    let n = line.words.len();
    let mut chunks = Vec::new();
    let mut word_chunks = vec![None; n];
    let mut i = 0usize;

    while i < n {
        let text = line.words[i].word.trim();
        if text.is_empty() {
            i += 1;
            continue;
        }

        let cjk = is_cjk_text(text);
        let mut end = i + 1;
        if !cjk {
            // 只合并“视觉上仍是同一个词”的相邻片段：两侧都没有空白。
            while end < n {
                let prev = &line.words[end - 1].word;
                let next = &line.words[end].word;
                let next_trim = next.trim();
                if next_trim.is_empty()
                    || is_cjk_text(next_trim)
                    || prev.ends_with(char::is_whitespace)
                    || next.starts_with(char::is_whitespace)
                {
                    break;
                }
                end += 1;
            }
        }

        let mut start_time = u64::MAX;
        let mut end_time = 0u64;
        let mut char_count = 0usize;
        let mut word_char_offsets = Vec::with_capacity(end - i);
        let mut merged = String::new();
        for word in &line.words[i..end] {
            start_time = start_time.min(word.start_time);
            end_time = end_time.max(word.end_time);
            word_char_offsets.push(char_count);
            let chars = word.word.trim().chars().count();
            char_count += chars;
            merged.push_str(word.word.trim());
        }
        let duration = end_time.saturating_sub(start_time);

        let mut emphasized = line.words[i..end].iter().any(should_emphasize_group_word);
        if is_cjk_text(&merged) {
            if char_count > 0 {
                // 单字均分时长 ≥ 1s（AMLL 拆字后的 atom 时长）。
                emphasized |= duration / char_count as u64 >= 1000;
            }
        } else {
            emphasized |= duration >= 1000 && (2..=7).contains(&char_count);
        }

        let chunk_index = chunks.len();
        for slot in &mut word_chunks[i..end] {
            *slot = Some(chunk_index);
        }
        chunks.push(EmphasisChunk {
            word_start: i,
            word_end: end,
            start_time,
            end_time,
            emphasized,
            char_count,
            word_char_offsets,
        });
        i = end;
    }

    (chunks, word_chunks)
}

fn is_cjk_text(text: &str) -> bool {
    let mut any = false;
    for ch in text.chars().filter(|c| !c.is_whitespace()) {
        any = true;
        if !is_cjk(ch) {
            return false;
        }
    }
    any
}

/// AMLL `LyricLineBase.shouldEmphasize` 的单 atom 版本（CJK 多字词见上方说明）。
fn should_emphasize_group_word(word: &crate::model::LyricWord) -> bool {
    let text = word.word.trim();
    let chars = text.chars().count();
    if chars == 0 {
        return false;
    }
    let duration = word.end_time.saturating_sub(word.start_time);
    if is_cjk_text(text) {
        chars == 1 && duration >= 1000
    } else {
        duration >= 1000 && (2..=7).contains(&chars)
    }
}

/// 整形单个视觉行。空文本绝不进入 gpui（见模块文档）。
fn shape_one(window: &Window, text: SharedString, font: &Font, font_size: f32) -> ShapedLine {
    let scope = PerfScope::enter(SHAPE_CHECKPOINT);
    let len = text.len();
    let run = TextRun {
        len,
        font: font.clone(),
        color: white(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped =
        window
            .text_system()
            .shape_line(text, px(font_size), std::slice::from_ref(&run), None);
    scope.finish(len as u64);
    shaped
}

fn empty_row(height: f32) -> ShapedRow {
    ShapedRow {
        line: ShapedLine::default(),
        words: Vec::new(),
        width: px(0.0),
        height: px(height.max(1.0)),
        is_translation: false,
    }
}

type WordRanges = Vec<Range<usize>>;

/// 主歌词拼接 + 词在整行文本中的字节范围。
///
/// 空白（含 `\n`/`\t` 等控制字符）统一压成单个空格，避免 `shape_line` 的换行断言。
fn build_main_text(line: &LyricLine) -> (String, WordRanges) {
    let capacity = line
        .words
        .iter()
        .map(|word| word.word.len())
        .sum::<usize>()
        .max(8);
    let mut text = String::with_capacity(capacity);
    let mut ranges = Vec::with_capacity(line.words.len());
    let mut last_space = false;

    for word in &line.words {
        let start = text.len();
        for ch in word.word.chars() {
            if ch.is_whitespace() || ch.is_control() {
                if !last_space {
                    text.push(' ');
                    last_space = true;
                }
            } else {
                text.push(ch);
                last_space = false;
            }
        }
        ranges.push(start..text.len());
    }

    (text, ranges)
}

/// 译文/音译的空白归一化（保留首尾空格，换行时再裁）。
fn normalize_plain(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut last_space = false;
    for ch in src.chars() {
        if ch.is_whitespace() || ch.is_control() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out
}

fn row_byte_range(row: &Range<usize>, fragments: &[MeasuredFragment]) -> (usize, usize) {
    if row.start >= row.end || row.end > fragments.len() {
        (0, 0)
    } else {
        (
            fragments[row.start].byte_start,
            fragments[row.end - 1].byte_end,
        )
    }
}

fn word_index_at(word_ranges: Option<&[Range<usize>]>, cursor: &mut usize, byte: usize) -> usize {
    let Some(ranges) = word_ranges else {
        return 0;
    };
    if ranges.is_empty() {
        return 0;
    }
    let mut c = (*cursor).min(ranges.len() - 1);
    while c + 1 < ranges.len() && ranges[c].end <= byte {
        c += 1;
    }
    *cursor = c;
    c
}

/// 构建可换行片段：空白单独成段；CJK 词与超长拉丁词按字符切分。
fn build_fragments(
    text: &str,
    xs: &CharXs,
    word_ranges: Option<&[Range<usize>]>,
    max_width: f32,
) -> Vec<MeasuredFragment> {
    let mut fragments = Vec::with_capacity(text.len());
    let mut word_cursor = 0usize;
    let mut i = 0usize;
    // 行首视为「前面有空白」，允许第一个词成为断点。
    let mut prev_space = true;
    let mut prev_wide = false;

    while i < text.len() {
        let ch = text[i..]
            .chars()
            .next()
            .expect("byte index is a char boundary");
        if ch.is_whitespace() || ch.is_control() {
            let start = i;
            let mut end = i;
            while end < text.len() {
                let c = text[end..].chars().next().unwrap();
                if c.is_whitespace() || c.is_control() {
                    end += c.len_utf8();
                } else {
                    break;
                }
            }
            let x0 = xs.x_at(start);
            let x1 = xs.x_at(end);
            fragments.push(MeasuredFragment {
                word_index: word_index_at(word_ranges, &mut word_cursor, start),
                byte_start: start,
                byte_end: end,
                x: px(x0),
                width: px((x1 - x0).max(0.0)),
                breakable_before: false,
                is_space: true,
            });
            prev_space = true;
            prev_wide = false;
            i = end;
            continue;
        }

        // 非空白 token：先测量整词，决定是否按字符细分。
        let token_start = i;
        let mut token_end = i;
        let mut has_wide = false;
        let mut last_ch = ch;
        while token_end < text.len() {
            let c = text[token_end..].chars().next().unwrap();
            if c.is_whitespace() || c.is_control() {
                break;
            }
            has_wide |= is_cjk(c);
            last_ch = c;
            token_end += c.len_utf8();
        }
        let token_x = xs.x_at(token_start);
        let token_x_end = xs.x_at(token_end);
        let token_width = (token_x_end - token_x).max(0.0);
        let word_index = word_index_at(word_ranges, &mut word_cursor, token_start);
        let break_at_start = prev_space || prev_wide;

        if has_wide || token_width > max_width {
            let mut pos = token_start;
            let mut first = true;
            while pos < token_end {
                let c = text[pos..].chars().next().unwrap();
                let next = pos + c.len_utf8();
                let x0 = xs.x_at(pos);
                let x1 = xs.x_at(next);
                // 词首：前面是空白或 CJK 时可断；词内：仅 CJK 字符之间可断。
                let breakable = if first {
                    break_at_start
                } else {
                    has_wide && is_cjk(c)
                };
                fragments.push(MeasuredFragment {
                    word_index,
                    byte_start: pos,
                    byte_end: next,
                    x: px(x0),
                    width: px((x1 - x0).max(0.0)),
                    breakable_before: breakable,
                    is_space: false,
                });
                first = false;
                pos = next;
            }
        } else {
            fragments.push(MeasuredFragment {
                word_index,
                byte_start: token_start,
                byte_end: token_end,
                x: px(token_x),
                width: px(token_width),
                breakable_before: break_at_start,
                is_space: false,
            });
        }

        prev_space = false;
        prev_wide = is_cjk(last_ch);
        i = token_end;
    }

    fragments
}

/// 从 row 文本的范围计算该视觉行内每个词的几何。
///
/// 词可能跨视觉行：此时每一行都会得到一个 `WordSpan`（`byte_range` 是裁剪后的
/// 部分），渲染层按 `word_index` 各自计算 mask 进度即可。
fn word_spans(
    text: &str,
    word_ranges: &[Range<usize>],
    row_start: usize,
    row_end: usize,
    xs: &CharXs,
) -> Vec<WordSpan> {
    let mut spans = Vec::with_capacity(8);
    for (word_index, range) in word_ranges.iter().enumerate() {
        let start = range.start.max(row_start);
        let end = range.end.min(row_end);
        if start >= end {
            continue;
        }
        let local_start = start - row_start;
        let local_end = end - row_start;
        let x0 = xs.x_at(local_start);
        let x1 = xs.x_at(local_end);
        // 词内跨视觉行时，强调动画的 stagger 序号要延续整词计数。
        let char_offset = text
            .get(range.start..start)
            .map(|s| s.chars().filter(|c| !c.is_whitespace()).count())
            .unwrap_or(0);
        spans.push(WordSpan {
            word_index,
            byte_range: local_start..local_end,
            char_offset,
            x: px(x0),
            width: px((x1 - x0).max(0.0)),
        });
    }
    spans
}

fn shape_main_rows(
    window: &Window,
    line: &LyricLine,
    font: &Font,
    font_size: f32,
    max_width: f32,
    line_height_ratio: f32,
) -> Vec<ShapedRow> {
    let row_height = (font_size * line_height_ratio.max(0.0)).max(1.0);
    let (raw_text, word_ranges) = build_main_text(line);
    if raw_text.trim().is_empty() {
        return vec![empty_row(row_height)];
    }

    let text = SharedString::new(raw_text);
    let text_len = text.len();
    let full = shape_one(window, text.clone(), font, font_size);
    let full_xs = CharXs::build(&text, &full);
    let fragments = build_fragments(&text, &full_xs, Some(&word_ranges), max_width);
    let rows = wrap_fragments(&fragments, px(max_width));

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (start, end) = row_byte_range(&row, &fragments);
        if start >= end {
            out.push(empty_row(row_height));
            continue;
        }
        if start == 0 && end == text_len {
            out.push(ShapedRow {
                words: word_spans(&text, &word_ranges, 0, text_len, &full_xs),
                width: full.width,
                height: px(row_height),
                is_translation: false,
                line: full.clone(),
            });
        } else {
            let row_text = SharedString::new(&text[start..end]);
            let shaped = shape_one(window, row_text.clone(), font, font_size);
            let xs = CharXs::build(&row_text, &shaped);
            out.push(ShapedRow {
                words: word_spans(&text, &word_ranges, start, end, &xs),
                width: shaped.width,
                height: px(row_height),
                is_translation: false,
                line: shaped,
            });
        }
    }

    if out.is_empty() {
        out.push(empty_row(row_height));
    }
    out
}

fn shape_sub_rows(
    window: &Window,
    source: &str,
    font: &Font,
    font_size: f32,
    max_width: f32,
    row_height: f32,
    is_translation: bool,
) -> Vec<ShapedRow> {
    let normalized = normalize_plain(source);
    if normalized.trim().is_empty() {
        return Vec::new();
    }
    let row_height = row_height.max(1.0);
    let text = SharedString::new(normalized);
    let text_len = text.len();
    let full = shape_one(window, text.clone(), font, font_size);

    if f32::from(full.width) <= max_width + WRAP_EPSILON {
        return vec![ShapedRow {
            line: full.clone(),
            words: Vec::new(),
            width: full.width,
            height: px(row_height),
            is_translation,
        }];
    }

    let full_xs = CharXs::build(&text, &full);
    let fragments = build_fragments(&text, &full_xs, None, max_width);
    let rows = wrap_fragments(&fragments, px(max_width));

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (start, end) = row_byte_range(&row, &fragments);
        if start >= end {
            out.push(empty_row(row_height));
            continue;
        }
        if start == 0 && end == text_len {
            out.push(ShapedRow {
                line: full.clone(),
                words: Vec::new(),
                width: full.width,
                height: px(row_height),
                is_translation,
            });
        } else {
            let row_text = SharedString::new(&text[start..end]);
            let shaped = shape_one(window, row_text, font, font_size);
            out.push(ShapedRow {
                width: shaped.width,
                line: shaped,
                words: Vec::new(),
                height: px(row_height),
                is_translation,
            });
        }
    }

    if out.is_empty() {
        out.push(empty_row(row_height));
    }
    out
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LyricWord;

    fn frag(word: usize, width: f32, breakable: bool) -> MeasuredFragment {
        MeasuredFragment {
            word_index: word,
            byte_start: 0,
            byte_end: 0,
            x: px(0.0),
            width: px(width),
            breakable_before: breakable,
            is_space: false,
        }
    }

    fn space(width: f32) -> MeasuredFragment {
        MeasuredFragment {
            word_index: 0,
            byte_start: 0,
            byte_end: 0,
            x: px(0.0),
            width: px(width),
            breakable_before: false,
            is_space: true,
        }
    }

    fn rows(fragments: &[MeasuredFragment], width: f32) -> Vec<(usize, usize)> {
        wrap_fragments(fragments, px(width))
            .into_iter()
            .map(|row| (row.start, row.end))
            .collect()
    }

    #[test]
    fn empty_input_has_one_empty_row() {
        assert_eq!(rows(&[], 100.0), vec![(0, 0)]);
    }

    #[test]
    fn single_fragment_no_wrap() {
        let fragments = [frag(0, 40.0, true)];
        assert_eq!(rows(&fragments, 100.0), vec![(0, 1)]);
    }

    #[test]
    fn multiple_words_wrap_at_space() {
        let fragments = [frag(0, 30.0, true), space(10.0), frag(1, 30.0, true)];
        assert_eq!(rows(&fragments, 100.0), vec![(0, 3)]);
        // 50px 放不下两个词：在第二个词前断行，行尾空格被裁掉。
        assert_eq!(rows(&fragments, 50.0), vec![(0, 1), (2, 3)]);
    }

    #[test]
    fn overlong_latin_word_hard_breaks() {
        let mut fragments = Vec::new();
        fragments.push(frag(0, 10.0, true));
        for _ in 0..7 {
            fragments.push(frag(0, 10.0, false));
        }
        assert_eq!(rows(&fragments, 50.0), vec![(0, 5), (5, 8)]);
    }

    #[test]
    fn cjk_breaks_between_chars() {
        let fragments: Vec<_> = (0..5).map(|_| frag(0, 20.0, true)).collect();
        assert_eq!(rows(&fragments, 45.0), vec![(0, 2), (2, 4), (4, 5)]);
    }

    #[test]
    fn whitespace_only_input_is_empty_row() {
        let fragments = [space(10.0), space(10.0)];
        assert_eq!(rows(&fragments, 100.0), vec![(0, 0)]);
    }

    #[test]
    fn leading_and_trailing_spaces_are_trimmed() {
        let fragments = [
            space(10.0),
            frag(0, 20.0, true),
            space(10.0),
            frag(1, 20.0, true),
            space(10.0),
        ];
        assert_eq!(rows(&fragments, 100.0), vec![(1, 4)]);
    }

    #[test]
    fn wrap_spans_convenience_matches_word_level() {
        let spans = [(0usize, 0..3usize, px(30.0)), (1, 3..6, px(30.0))];
        assert_eq!(wrap_spans(&spans, px(50.0)), vec![vec![0], vec![1]]);
        assert_eq!(wrap_spans(&spans, px(100.0)), vec![vec![0, 1]]);
    }

    #[test]
    fn main_text_collapses_whitespace_and_keeps_word_ranges() {
        let line = LyricLine::new(vec![
            LyricWord::new("Hello", 0, 100),
            LyricWord::new("  big\nworld", 100, 200),
        ]);
        let (text, ranges) = build_main_text(&line);
        assert_eq!(text, "Hello big world");
        assert_eq!(&text[ranges[0].clone()], "Hello");
        assert_eq!(&text[ranges[1].clone()], " big world");
    }

    fn doc_with(n: usize) -> LyricDocument {
        let lines = (0..n)
            .map(|i| {
                let mut line = LyricLine::new(vec![LyricWord::new(
                    format!("line {i} text"),
                    i as u64 * 1000,
                    i as u64 * 1000 + 900,
                )]);
                line.translated_lyric = format!("translation {i}");
                line
            })
            .collect();
        LyricDocument {
            lines,
            is_dynamic: true,
            has_duet: false,
        }
    }

    fn cache_with(n: usize) -> TextLayoutCache {
        let mut cache = TextLayoutCache::new(LyricTextStyle::default());
        cache.set_viewport(1280.0, 720.0);
        cache.set_lyrics(&doc_with(n));
        cache
    }

    #[test]
    fn heights_are_estimated_and_non_zero() {
        let cache = cache_with(7);
        assert_eq!(cache.heights().len(), 7);
        assert!(cache.heights().iter().all(|height| *height > 0.0));
        assert_eq!(cache.shaped_count(), 0);
    }

    #[test]
    fn metrics_follow_amll_ratios() {
        let style = LyricTextStyle::default();
        assert_eq!(main_font_size(&style, false), 24.0);
        assert!((main_font_size(&style, true) - 16.8).abs() < 0.001);
        assert_eq!(sub_font_size(&style, 24.0), 12.0);
        // 背景行副字号 16.8 * 0.5 = 8.4，被抬到 10px 下限。
        assert_eq!(sub_font_size(&style, main_font_size(&style, true)), 10.0);
        assert_eq!(padding_x(&style, 24.0), 24.0);
        assert_eq!(content_width(1280.0, 24.0, false), 1232.0);
        // 内容宽度永远 >= 1。
        assert_eq!(content_width(10.0, 100.0, false), 1.0);

        let hint = LineHint {
            main_chars: 10,
            main_wide: 0,
            trans_chars: 5,
            roman_chars: 0,
            is_bg: false,
        };
        let height = estimate_height(&style, 1280.0, 720.0, false, &hint);
        // 2 * padding + 1 行主歌词 + 1 行副歌词。
        let expected = 2.0 * 720.0 * 0.02 + 24.0 * 1.2 + 12.0 * 1.5;
        assert!((height - expected).abs() < 0.01, "{height} vs {expected}");
    }

    #[test]
    fn empty_lyrics_have_no_heights() {
        let mut cache = TextLayoutCache::new(LyricTextStyle::default());
        cache.set_lyrics(&LyricDocument::default());
        assert!(cache.heights().is_empty());
        assert_eq!(cache.shaped_count(), 0);
    }

    #[test]
    fn viewport_change_invalidates_shapes_but_keeps_heights() {
        let mut cache = cache_with(32);
        for i in 0..8 {
            cache.insert_fake(i);
        }
        assert_eq!(cache.shaped_count(), 8);
        let generation = cache.generation();

        cache.set_viewport(640.0, 480.0);
        assert_eq!(cache.shaped_count(), 0);
        assert!(cache.get(0).is_none());
        assert_ne!(cache.generation(), generation);
        assert_eq!(cache.heights().len(), 32);
        assert!(cache.heights().iter().all(|height| *height > 0.0));

        // 相同尺寸不应再次清空。
        cache.insert_fake(0);
        let generation = cache.generation();
        cache.set_viewport(640.0, 480.0);
        assert_eq!(cache.generation(), generation);
        assert!(cache.get(0).is_some());
    }

    #[test]
    fn style_change_recomputes_heights() {
        let mut cache = cache_with(4);
        let before = cache.heights().to_vec();
        let mut style = cache.style().clone();
        style.base_font_size = 48.0;
        cache.set_style(style, 1280.0, 720.0);
        assert_eq!(cache.heights().len(), 4);
        assert!(
            cache
                .heights()
                .iter()
                .zip(before.iter())
                .all(|(new, old)| *new > *old),
            "larger font must estimate a taller line"
        );
    }

    #[test]
    fn lru_eviction_keeps_heights_and_cap() {
        let mut cache = cache_with(64);
        let heights_before = cache.heights().to_vec();
        cache.set_max_shaped_lines(16);
        for i in 0..40 {
            cache.insert_fake(i);
        }
        assert_eq!(cache.max_shaped_lines(), 16);
        assert_eq!(cache.shaped_count(), 16);
        assert!(cache.get(0).is_none());
        assert!(cache.get(23).is_none());
        assert!(cache.get(24).is_some());
        assert!(cache.get(39).is_some());
        assert_eq!(cache.heights().len(), 64);
        assert!(cache.heights().iter().all(|height| *height > 0.0));
        // 淘汰只丢弃整形结果，不改变播放器使用的高度。
        assert_eq!(cache.heights(), &heights_before[..]);

        // 再次使用旧行时重新整形并淘汰队首。
        cache.insert_fake(5);
        assert!(cache.get(5).is_some());
        assert_eq!(cache.shaped_count(), 16);
    }

    #[test]
    fn max_shaped_lines_has_a_floor() {
        let mut cache = cache_with(2);
        cache.set_max_shaped_lines(1);
        assert_eq!(cache.max_shaped_lines(), MIN_MAX_SHAPED_LINES);
    }

    #[test]
    fn emphasis_merges_adjacent_latin_fragments() {
        // YRC 常见：一个英文单词被拆成多段、之间没有空格。
        let line = LyricLine {
            words: vec![
                LyricWord::new(" su", 0, 500),
                LyricWord::new("gar", 500, 1_200),
                LyricWord::new(" so", 1_200, 1_700),
                LyricWord::new(" sweet", 1_700, 2_400),
            ],
            start_time: 0,
            end_time: 2_400,
            ..Default::default()
        };
        let (chunks, word_chunks) = build_emphasis_chunks(&line);
        // " sugar" 合并（1.2s, 5 字）；" so"+"sweet" 因首词以空格开头而不合并。
        assert_eq!(chunks.len(), 3);
        assert!(chunks[0].emphasized);
        assert_eq!(chunks[0].word_start, 0);
        assert_eq!(chunks[0].word_end, 2);
        assert_eq!(chunks[0].char_count, 5);
        assert_eq!(chunks[0].word_char_offsets, vec![0, 2]);
        assert!(!chunks[1].emphasized);
        assert!(!chunks[2].emphasized);
        assert_eq!(word_chunks[1], Some(0));
    }

    #[test]
    fn emphasis_does_not_merge_cjk_runs() {
        // CJK 逐字歌词不能把整句合并：每个字的时长都远小于 1s。
        let line = LyricLine {
            words: vec![
                LyricWord::new("我", 0, 200),
                LyricWord::new("们", 200, 400),
                LyricWord::new("走", 400, 600),
            ],
            start_time: 0,
            end_time: 600,
            ..Default::default()
        };
        let (chunks, _) = build_emphasis_chunks(&line);
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|chunk| !chunk.emphasized));
    }

    #[test]
    fn emphasis_uses_per_char_duration_for_multi_cjk_word() {
        // AMLL 会把多字 CJK 词拆成单字 atom：3 个字 / 2.4s = 每字 0.8s，不强调。
        let line = LyricLine {
            words: vec![LyricWord::new(" landslide ", 0, 2_400)],
            start_time: 0,
            end_time: 2_400,
            ..Default::default()
        };
        let (chunks, _) = build_emphasis_chunks(&line);
        assert_eq!(chunks.len(), 1);
        assert!(!chunks[0].emphasized);

        let line = LyricLine {
            words: vec![LyricWord::new("大海", 0, 2_400)],
            start_time: 0,
            end_time: 2_400,
            ..Default::default()
        };
        let (chunks, _) = build_emphasis_chunks(&line);
        assert!(chunks[0].emphasized, "2 chars / 2.4s = 1.2s per char");
    }

    impl TextLayoutCache {
        /// 测试用：模拟「整形完成」，槽位与 LRU 行为与 `ensure` 一致（高度不动）。
        fn insert_fake(&mut self, index: usize) {
            let shape = Arc::new(LineShape {
                index,
                main: Vec::new(),
                translation: Vec::new(),
                roman: Vec::new(),
                height: px(1.0),
                main_font_size: px(10.0),
                sub_font_size: px(10.0),
                padding_top: px(0.0),
                padding_x: px(0.0),
                content_width: px(100.0),
                emphasis_chunks: Vec::new(),
                word_chunks: Vec::new(),
            });
            self.store(index, shape);
        }
    }
}
