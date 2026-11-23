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

use std::ops::Range;
use std::sync::Arc;

use chordal_core::perf::PerfScope;
use gpui::{
    Font, FontWeight, LineLayout, Pixels, ShapedLine, SharedString, TextRun, Window, px, white,
};

use crate::model::{LyricDocument, LyricLine};

mod shape;

use shape::*;

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

/// LRU 链表空节点标记（行下标不会达到 u32::MAX）。
const LRU_NONE: u32 = u32::MAX;

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
    /// 以下字段在整形时一次算好：强调动画的时长/幅度/辉光/延迟只依赖分组
    /// 与行的时间戳，与具体字形和时间点无关。300Hz 下逐字形重算是纯浪费。
    pub duration_ms: f32,
    pub amount: f32,
    pub blur: f32,
    pub delay_ms: f32,
    pub float_duration_ms: f32,
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
    /// 按行下标的侵入式 LRU 双向链表（prev/next）。`touch` 为严格 O(1)：
    /// 可见行每帧都会被 touch，原来的 `VecDeque::position + remove` 是
    /// O(可见行数 × 缓存容量)（默认 256），是 300Hz 下的确定性热点。
    lru_prev: Vec<u32>,
    lru_next: Vec<u32>,
    lru_head: u32,
    lru_tail: u32,
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
            lru_prev: Vec::new(),
            lru_next: Vec::new(),
            lru_head: LRU_NONE,
            lru_tail: LRU_NONE,
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
        self.lru_prev.clear();
        self.lru_prev.resize(n, LRU_NONE);
        self.lru_next.clear();
        self.lru_next.resize(n, LRU_NONE);
        self.lru_head = LRU_NONE;
        self.lru_tail = LRU_NONE;
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
        self.lru_prev.fill(LRU_NONE);
        self.lru_next.fill(LRU_NONE);
        self.lru_head = LRU_NONE;
        self.lru_tail = LRU_NONE;
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

    /// 把一行移到 LRU 队尾，严格 O(1)。
    fn touch(&mut self, index: usize) {
        let i = index as u32;
        if self.lru_tail == i {
            return;
        }
        let prev = self.lru_prev[index];
        let next = self.lru_next[index];
        // 已被淘汰的行 prev/next 都是 NONE，且不在 head/tail 上。
        let linked = prev != LRU_NONE
            || next != LRU_NONE
            || self.lru_head == i
            || self.lru_tail == i;
        if linked {
            if prev != LRU_NONE {
                self.lru_next[prev as usize] = next;
            } else {
                self.lru_head = next;
            }
            if next != LRU_NONE {
                self.lru_prev[next as usize] = prev;
            } else {
                self.lru_tail = prev;
            }
        }
        self.lru_prev[index] = self.lru_tail;
        self.lru_next[index] = LRU_NONE;
        if self.lru_tail != LRU_NONE {
            self.lru_next[self.lru_tail as usize] = i;
        } else {
            self.lru_head = i;
        }
        self.lru_tail = i;
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
            debug_assert!(self.lru_head != LRU_NONE, "LRU list shorter than shaped_count");
            if self.lru_head == LRU_NONE {
                break;
            }
            let victim = self.lru_head as usize;
            let next = self.lru_next[victim];
            self.lru_head = next;
            if next != LRU_NONE {
                self.lru_prev[next as usize] = LRU_NONE;
            } else {
                self.lru_tail = LRU_NONE;
            }
            self.lru_prev[victim] = LRU_NONE;
            self.lru_next[victim] = LRU_NONE;
            if self.shapes.get(victim).is_some_and(|slot| slot.is_some()) {
                self.shapes[victim] = None;
                self.shaped_count -= 1;
            }
        }
    }
}


#[cfg(test)]
mod tests;
