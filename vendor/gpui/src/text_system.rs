mod font_fallbacks;
mod font_features;
mod line;
mod line_layout;
mod line_wrapper;

pub use font_fallbacks::*;
pub use font_features::*;
pub use line::*;
pub use line_layout::*;
pub use line_wrapper::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Bounds, DevicePixels, Hsla, Pixels, PlatformTextSystem, Point, Result, SharedString, Size,
    StrikethroughStyle, UnderlineStyle, px,
};
use anyhow::{Context as _, anyhow};
use collections::FxHashMap;
use core::fmt;
use derive_more::{Add, Deref, FromStr, Sub};
use itertools::Itertools;
use parking_lot::{Mutex, RwLock, RwLockUpgradableReadGuard};
use smallvec::{SmallVec, smallvec};
use std::{
    borrow::Cow,
    cmp,
    fmt::{Debug, Display, Formatter},
    hash::{Hash, Hasher},
    ops::{Deref, DerefMut, Range},
    sync::Arc,
};

/// An opaque identifier for a specific font.
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
#[repr(C)]
pub struct FontId(pub usize);

/// An opaque identifier for a specific font family.
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct FontFamilyId(pub usize);

pub(crate) const SUBPIXEL_VARIANTS_X: u8 = 4;

pub(crate) const SUBPIXEL_VARIANTS_Y: u8 =
    if cfg!(target_os = "windows") || cfg!(target_os = "linux") {
        1
    } else {
        SUBPIXEL_VARIANTS_X
    };

/// Vertical subpixel bins used by [`RenderGlyphParams::subpixel_positioning`].
///
/// The default Windows/Linux value of one bin snaps glyphs to whole device
/// pixels in y, which is right for static UI text but makes text that moves
/// every frame (spring-animated lyrics) visibly jump by a pixel at a time.
/// Opting into smooth positioning trades a bounded 4× atlas key increase for
/// continuous vertical motion.
pub(crate) const SUBPIXEL_VARIANTS_Y_PRECISE: u8 = 4;

/// The GPUI text rendering sub system.
pub struct TextSystem {
    platform_text_system: Arc<dyn PlatformTextSystem>,
    font_ids_by_font: RwLock<FxHashMap<Font, Result<FontId>>>,
    font_metrics: RwLock<FxHashMap<FontId, FontMetrics>>,
    raster_bounds: RwLock<FxHashMap<RenderGlyphParams, Bounds<DevicePixels>>>,
    wrapper_pool: Mutex<FxHashMap<FontIdWithSize, Vec<LineWrapper>>>,
    font_runs_pool: Mutex<Vec<Vec<FontRun>>>,
    fallback_font_stack: SmallVec<[Font; 2]>,
}

impl TextSystem {
    pub(crate) fn new(platform_text_system: Arc<dyn PlatformTextSystem>) -> Self {
        TextSystem {
            platform_text_system,
            font_metrics: RwLock::default(),
            raster_bounds: RwLock::default(),
            font_ids_by_font: RwLock::default(),
            wrapper_pool: Mutex::default(),
            font_runs_pool: Mutex::default(),
            fallback_font_stack: smallvec![
                // TODO: Remove this when Linux have implemented setting fallbacks.
                font(".ZedMono"),
                font(".ZedSans"),
                font("Helvetica"),
                font("Segoe UI"),     // Windows
                font("Ubuntu"),       // Gnome (Ubuntu)
                font("Adwaita Sans"), // Gnome 47
                font("Cantarell"),    // Gnome
                font("Noto Sans"),    // KDE
                font("DejaVu Sans"),
                font("Arial"), // macOS, Windows
            ],
        }
    }

    /// Get a list of all available font names from the operating system.
    pub fn all_font_names(&self) -> Vec<String> {
        let mut names = self.platform_text_system.all_font_names();
        names.extend(
            self.fallback_font_stack
                .iter()
                .map(|font| font.family.to_string()),
        );
        names.push(".SystemUIFont".to_string());
        names.sort();
        names.dedup();
        names
    }

    /// Add a font's data to the text system.
    pub fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        self.platform_text_system.add_fonts(fonts)
    }

    /// Get the FontId for the configure font family and style.
    fn font_id(&self, font: &Font) -> Result<FontId> {
        fn clone_font_id_result(font_id: &Result<FontId>) -> Result<FontId> {
            match font_id {
                Ok(font_id) => Ok(*font_id),
                Err(err) => Err(anyhow!("{err}")),
            }
        }

        let font_id = self
            .font_ids_by_font
            .read()
            .get(font)
            .map(clone_font_id_result);
        if let Some(font_id) = font_id {
            font_id
        } else {
            let font_id = self.platform_text_system.font_id(font);
            self.font_ids_by_font
                .write()
                .insert(font.clone(), clone_font_id_result(&font_id));
            font_id
        }
    }

    /// Get the Font for the Font Id.
    pub fn get_font_for_id(&self, id: FontId) -> Option<Font> {
        let lock = self.font_ids_by_font.read();
        lock.iter()
            .filter_map(|(font, result)| match result {
                Ok(font_id) if *font_id == id => Some(font.clone()),
                _ => None,
            })
            .next()
    }

    /// Resolves the specified font, falling back to the default font stack if
    /// the font fails to load.
    ///
    /// # Panics
    ///
    /// Panics if the font and none of the fallbacks can be resolved.
    pub fn resolve_font(&self, font: &Font) -> FontId {
        if let Ok(font_id) = self.font_id(font) {
            return font_id;
        }
        for fallback in &self.fallback_font_stack {
            if let Ok(font_id) = self.font_id(fallback) {
                return font_id;
            }
        }

        panic!(
            "failed to resolve font '{}' or any of the fallbacks: {}",
            font.family,
            self.fallback_font_stack
                .iter()
                .map(|fallback| &fallback.family)
                .join(", ")
        );
    }

    /// Get the bounding box for the given font and font size.
    /// A font's bounding box is the smallest rectangle that could enclose all glyphs
    /// in the font. superimposed over one another.
    pub fn bounding_box(&self, font_id: FontId, font_size: Pixels) -> Bounds<Pixels> {
        self.read_metrics(font_id, |metrics| metrics.bounding_box(font_size))
    }

    /// Get the typographic bounds for the given character, in the given font and size.
    pub fn typographic_bounds(
        &self,
        font_id: FontId,
        font_size: Pixels,
        character: char,
    ) -> Result<Bounds<Pixels>> {
        let glyph_id = self
            .platform_text_system
            .glyph_for_char(font_id, character)
            .with_context(|| format!("glyph not found for character '{character}'"))?;
        let bounds = self
            .platform_text_system
            .typographic_bounds(font_id, glyph_id)?;
        Ok(self.read_metrics(font_id, |metrics| {
            (bounds / metrics.units_per_em as f32 * font_size.0).map(px)
        }))
    }

    /// Get the advance width for the given character, in the given font and size.
    pub fn advance(&self, font_id: FontId, font_size: Pixels, ch: char) -> Result<Size<Pixels>> {
        let glyph_id = self
            .platform_text_system
            .glyph_for_char(font_id, ch)
            .with_context(|| format!("glyph not found for character '{ch}'"))?;
        let result = self.platform_text_system.advance(font_id, glyph_id)?
            / self.units_per_em(font_id) as f32;

        Ok(result * font_size)
    }

    /// Returns the width of an `em`.
    ///
    /// Uses the width of the `m` character in the given font and size.
    pub fn em_width(&self, font_id: FontId, font_size: Pixels) -> Result<Pixels> {
        Ok(self.typographic_bounds(font_id, font_size, 'm')?.size.width)
    }

    /// Returns the advance width of an `em`.
    ///
    /// Uses the advance width of the `m` character in the given font and size.
    pub fn em_advance(&self, font_id: FontId, font_size: Pixels) -> Result<Pixels> {
        Ok(self.advance(font_id, font_size, 'm')?.width)
    }

    /// Returns the width of an `ch`.
    ///
    /// Uses the width of the `0` character in the given font and size.
    pub fn ch_width(&self, font_id: FontId, font_size: Pixels) -> Result<Pixels> {
        Ok(self.typographic_bounds(font_id, font_size, '0')?.size.width)
    }

    /// Returns the advance width of an `ch`.
    ///
    /// Uses the advance width of the `0` character in the given font and size.
    pub fn ch_advance(&self, font_id: FontId, font_size: Pixels) -> Result<Pixels> {
        Ok(self.advance(font_id, font_size, '0')?.width)
    }

    /// Get the number of font size units per 'em square',
    /// Per MDN: "an abstract square whose height is the intended distance between
    /// lines of type in the same type size"
    pub fn units_per_em(&self, font_id: FontId) -> u32 {
        self.read_metrics(font_id, |metrics| metrics.units_per_em)
    }

    /// Get the height of a capital letter in the given font and size.
    pub fn cap_height(&self, font_id: FontId, font_size: Pixels) -> Pixels {
        self.read_metrics(font_id, |metrics| metrics.cap_height(font_size))
    }

    /// Get the height of the x character in the given font and size.
    pub fn x_height(&self, font_id: FontId, font_size: Pixels) -> Pixels {
        self.read_metrics(font_id, |metrics| metrics.x_height(font_size))
    }

    /// Get the recommended distance from the baseline for the given font
    pub fn ascent(&self, font_id: FontId, font_size: Pixels) -> Pixels {
        self.read_metrics(font_id, |metrics| metrics.ascent(font_size))
    }

    /// Get the recommended distance below the baseline for the given font,
    /// in single spaced text.
    pub fn descent(&self, font_id: FontId, font_size: Pixels) -> Pixels {
        self.read_metrics(font_id, |metrics| metrics.descent(font_size))
    }

    /// Get the recommended baseline offset for the given font and line height.
    pub fn baseline_offset(
        &self,
        font_id: FontId,
        font_size: Pixels,
        line_height: Pixels,
    ) -> Pixels {
        let ascent = self.ascent(font_id, font_size);
        let descent = self.descent(font_id, font_size);
        let padding_top = (line_height - ascent - descent) / 2.;
        padding_top + ascent
    }

    fn read_metrics<T>(&self, font_id: FontId, read: impl FnOnce(&FontMetrics) -> T) -> T {
        let lock = self.font_metrics.upgradable_read();

        if let Some(metrics) = lock.get(&font_id) {
            read(metrics)
        } else {
            let mut lock = RwLockUpgradableReadGuard::upgrade(lock);
            let metrics = lock
                .entry(font_id)
                .or_insert_with(|| self.platform_text_system.font_metrics(font_id));
            read(metrics)
        }
    }

    /// Returns a handle to a line wrapper, for the given font and font size.
    pub fn line_wrapper(self: &Arc<Self>, font: Font, font_size: Pixels) -> LineWrapperHandle {
        let lock = &mut self.wrapper_pool.lock();
        let font_id = self.resolve_font(&font);
        let wrappers = lock
            .entry(FontIdWithSize { font_id, font_size })
            .or_default();
        let wrapper = wrappers.pop().unwrap_or_else(|| {
            LineWrapper::new(font_id, font_size, self.platform_text_system.clone())
        });

        LineWrapperHandle {
            wrapper: Some(wrapper),
            text_system: self.clone(),
        }
    }

    /// Get the rasterized size and location of a specific, rendered glyph.
    pub(crate) fn raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        let raster_bounds = self.raster_bounds.upgradable_read();
        if let Some(bounds) = raster_bounds.get(params) {
            Ok(*bounds)
        } else {
            let mut raster_bounds = RwLockUpgradableReadGuard::upgrade(raster_bounds);
            let mut bounds = self.platform_text_system.glyph_raster_bounds(params)?;
            // Blurred glyphs need a zero-filled margin for the kernel to decay
            // in; the platform rasterizer renders the glyph into this expanded
            // rect and the resulting bitmap is convolved below.
            if params.blur_radius.0 > 0.0 && bounds.size.width.0 > 0 && bounds.size.height.0 > 0 {
                let pad = blur_padding(params.blur_radius.0);
                bounds.origin.x = DevicePixels(bounds.origin.x.0 - pad);
                bounds.origin.y = DevicePixels(bounds.origin.y.0 - pad);
                bounds.size.width = DevicePixels(bounds.size.width.0 + 2 * pad);
                bounds.size.height = DevicePixels(bounds.size.height.0 + 2 * pad);
            }
            raster_bounds.insert(params.clone(), bounds);
            Ok(bounds)
        }
    }

    pub(crate) fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        let raster_bounds = self.raster_bounds(params)?;
        let (size, mut bytes) = self
            .platform_text_system
            .rasterize_glyph(params, raster_bounds)?;
        if params.blur_radius.0 > 0.0 && !params.is_emoji {
            blur_alpha_mask(
                &mut bytes,
                size.width.0.max(0) as usize,
                size.height.0.max(0) as usize,
                params.blur_radius.0,
            );
        }
        Ok((size, bytes))
    }
}

/// Support of the blur kernel, in device pixels. Skia uses 3σ as well.
fn blur_padding(sigma: f32) -> i32 {
    (sigma * 3.0).ceil() as i32
}

/// Blur a single-channel glyph alpha mask in place.
///
/// Mirrors Skia's `SkBlurEngine`: an exact separable Gaussian for σ < 2, and
/// Kovesi's three-box approximation above that (`w ≈ sqrt(4σ² + 1)`, mixed
/// between the two nearest odd widths to hit the requested variance). Both
/// are O(1) per pixel per pass and cached in the glyph atlas, so this runs at
/// most once per (glyph, size, blur bucket).
fn blur_alpha_mask(mask: &mut [u8], width: usize, height: usize, sigma: f32) {
    if width == 0 || height == 0 || mask.len() < width * height || sigma <= 0.0 {
        return;
    }
    if sigma < 2.0 {
        gaussian_blur_alpha(mask, width, height, sigma);
    } else {
        three_box_blur_alpha(mask, width, height, sigma);
    }
}

fn gaussian_blur_alpha(mask: &mut [u8], width: usize, height: usize, sigma: f32) {
    let radius = blur_padding(sigma) as usize;
    let mut kernel = Vec::with_capacity(radius + 1);
    let mut sum = 0.0f32;
    for i in 0..=radius {
        let weight = (-(i as f32 * i as f32) / (2.0 * sigma * sigma)).exp();
        kernel.push(weight);
        sum += if i == 0 { weight } else { 2.0 * weight };
    }
    for weight in &mut kernel {
        *weight /= sum;
    }

    let mut scratch = vec![0.0f32; width * height];
    for y in 0..height {
        let row = &mask[y * width..(y + 1) * width];
        let out = &mut scratch[y * width..(y + 1) * width];
        for x in 0..width {
            let mut acc = row[x] as f32 * kernel[0];
            for i in 1..=radius {
                let left = x.saturating_sub(i);
                let right = (x + i).min(width - 1);
                acc += (row[left] as f32 + row[right] as f32) * kernel[i];
            }
            out[x] = acc;
        }
    }

    for x in 0..width {
        for y in 0..height {
            let mut acc = scratch[y * width + x] * kernel[0];
            for i in 1..=radius {
                let top = y.saturating_sub(i);
                let bottom = (y + i).min(height - 1);
                acc += (scratch[top * width + x] + scratch[bottom * width + x]) * kernel[i];
            }
            mask[y * width + x] = acc.round().clamp(0.0, 255.0) as u8;
        }
    }
}

fn three_box_blur_alpha(mask: &mut [u8], width: usize, height: usize, sigma: f32) {
    let ideal = (4.0 * sigma * sigma + 1.0).sqrt();
    let mut lower = ideal.floor() as i32;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1);
    let upper = lower + 2;
    let variance = |w: i32| ((w * w - 1) as f32) / 12.0;
    let (variance_lower, variance_upper) = (variance(lower), variance(upper));
    let spread = variance_upper - variance_lower;
    let passes_lower = if spread <= f32::EPSILON {
        0
    } else {
        (((3.0 * variance_upper - sigma * sigma) / spread).round() as i32).clamp(0, 3)
    };

    // Keep the whole three-pass chain in f32. Rounding back to u8 between
    // passes loses a large fraction of the tail mass (small values round to
    // zero), which makes the blur look darker than CSS.
    let mut buffer: Vec<f32> = mask.iter().map(|&value| value as f32).collect();
    let mut scratch = vec![0.0f32; width * height];
    for _ in 0..passes_lower {
        box_blur_f32(&mut buffer, &mut scratch, width, height, lower as usize);
    }
    for _ in 0..(3 - passes_lower) {
        box_blur_f32(&mut buffer, &mut scratch, width, height, upper as usize);
    }
    for (output, value) in mask.iter_mut().zip(buffer) {
        *output = value.round().clamp(0.0, 255.0) as u8;
    }
}

fn box_blur_f32(
    buffer: &mut [f32],
    scratch: &mut [f32],
    width: usize,
    height: usize,
    box_width: usize,
) {
    let radius = box_width / 2;
    if radius == 0 {
        return;
    }

    for y in 0..height {
        let row = &buffer[y * width..(y + 1) * width];
        let out = &mut scratch[y * width..(y + 1) * width];
        let mut sum = 0.0f32;
        for x in 0..=radius.min(width - 1) {
            sum += row[x];
        }
        for x in 0..width {
            out[x] = sum / box_width as f32;
            let add = x + radius + 1;
            if add < width {
                sum += row[add];
            }
            if x >= radius {
                sum -= row[x - radius];
            }
        }
    }

    for x in 0..width {
        let mut sum = 0.0f32;
        for y in 0..=radius.min(height - 1) {
            sum += scratch[y * width + x];
        }
        for y in 0..height {
            buffer[y * width + x] = sum / box_width as f32;
            let add = y + radius + 1;
            if add < height {
                sum += scratch[add * width + x];
            }
            if y >= radius {
                sum -= scratch[(y - radius) * width + x];
            }
        }
    }
}

/// The GPUI text layout subsystem.
#[derive(Deref)]
pub struct WindowTextSystem {
    line_layout_cache: LineLayoutCache,
    #[deref]
    text_system: Arc<TextSystem>,
}

impl WindowTextSystem {
    pub(crate) fn new(text_system: Arc<TextSystem>) -> Self {
        Self {
            line_layout_cache: LineLayoutCache::new(text_system.platform_text_system.clone()),
            text_system,
        }
    }

    pub(crate) fn layout_index(&self) -> LineLayoutIndex {
        self.line_layout_cache.layout_index()
    }

    pub(crate) fn reuse_layouts(&self, index: Range<LineLayoutIndex>) {
        self.line_layout_cache.reuse_layouts(index)
    }

    pub(crate) fn truncate_layouts(&self, index: LineLayoutIndex) {
        self.line_layout_cache.truncate_layouts(index)
    }

    /// Shape the given line, at the given font_size, for painting to the screen.
    /// Subsets of the line can be styled independently with the `runs` parameter.
    ///
    /// Note that this method can only shape a single line of text. It will panic
    /// if the text contains newlines. If you need to shape multiple lines of text,
    /// use [`Self::shape_text`] instead.
    pub fn shape_line(
        &self,
        text: SharedString,
        font_size: Pixels,
        runs: &[TextRun],
        force_width: Option<Pixels>,
    ) -> ShapedLine {
        debug_assert!(
            text.find('\n').is_none(),
            "text argument should not contain newlines"
        );

        let mut decoration_runs = SmallVec::<[DecorationRun; 32]>::new();
        for run in runs {
            if let Some(last_run) = decoration_runs.last_mut()
                && last_run.color == run.color
                && last_run.underline == run.underline
                && last_run.strikethrough == run.strikethrough
                && last_run.background_color == run.background_color
            {
                last_run.len += run.len as u32;
                continue;
            }
            decoration_runs.push(DecorationRun {
                len: run.len as u32,
                color: run.color,
                background_color: run.background_color,
                underline: run.underline,
                strikethrough: run.strikethrough,
            });
        }

        let layout = self.layout_line(&text, font_size, runs, force_width);

        ShapedLine {
            layout,
            text,
            decoration_runs,
        }
    }

    /// Shape a multi line string of text, at the given font_size, for painting to the screen.
    /// Subsets of the text can be styled independently with the `runs` parameter.
    /// If `wrap_width` is provided, the line breaks will be adjusted to fit within the given width.
    pub fn shape_text(
        &self,
        text: SharedString,
        font_size: Pixels,
        runs: &[TextRun],
        wrap_width: Option<Pixels>,
        line_clamp: Option<usize>,
    ) -> Result<SmallVec<[WrappedLine; 1]>> {
        let mut runs = runs.iter().filter(|run| run.len > 0).cloned().peekable();
        let mut font_runs = self.font_runs_pool.lock().pop().unwrap_or_default();

        let mut lines = SmallVec::new();
        let mut line_start = 0;
        let mut max_wrap_lines = line_clamp.unwrap_or(usize::MAX);
        let mut wrapped_lines = 0;

        let mut process_line = |line_text: SharedString| {
            font_runs.clear();
            let line_end = line_start + line_text.len();

            let mut decoration_runs = SmallVec::<[DecorationRun; 32]>::new();
            let mut run_start = line_start;
            while run_start < line_end {
                let Some(run) = runs.peek_mut() else {
                    break;
                };

                let run_len_within_line = cmp::min(line_end, run_start + run.len) - run_start;

                let decoration_changed = if let Some(last_run) = decoration_runs.last_mut()
                    && last_run.color == run.color
                    && last_run.underline == run.underline
                    && last_run.strikethrough == run.strikethrough
                    && last_run.background_color == run.background_color
                {
                    last_run.len += run_len_within_line as u32;
                    false
                } else {
                    decoration_runs.push(DecorationRun {
                        len: run_len_within_line as u32,
                        color: run.color,
                        background_color: run.background_color,
                        underline: run.underline,
                        strikethrough: run.strikethrough,
                    });
                    true
                };

                let font_id = self.resolve_font(&run.font);
                if let Some(font_run) = font_runs.last_mut()
                    && font_id == font_run.font_id
                    && !decoration_changed
                {
                    font_run.len += run_len_within_line;
                } else {
                    font_runs.push(FontRun {
                        len: run_len_within_line,
                        font_id,
                    });
                }

                if run_len_within_line == run.len {
                    runs.next();
                } else {
                    // Preserve the remainder of the run for the next line
                    run.len -= run_len_within_line;
                }
                run_start += run_len_within_line;
            }

            let layout = self.line_layout_cache.layout_wrapped_line(
                &line_text,
                font_size,
                &font_runs,
                wrap_width,
                Some(max_wrap_lines - wrapped_lines),
            );
            wrapped_lines += layout.wrap_boundaries.len();

            lines.push(WrappedLine {
                layout,
                decoration_runs,
                text: line_text,
            });

            // Skip `\n` character.
            line_start = line_end + 1;
            if let Some(run) = runs.peek_mut() {
                run.len -= 1;
                if run.len == 0 {
                    runs.next();
                }
            }
        };

        let mut split_lines = text.split('\n');
        let mut processed = false;

        if let Some(first_line) = split_lines.next()
            && let Some(second_line) = split_lines.next()
        {
            processed = true;
            process_line(first_line.to_string().into());
            process_line(second_line.to_string().into());
            for line_text in split_lines {
                process_line(line_text.to_string().into());
            }
        }

        if !processed {
            process_line(text);
        }

        self.font_runs_pool.lock().push(font_runs);

        Ok(lines)
    }

    pub(crate) fn finish_frame(&self) {
        self.line_layout_cache.finish_frame()
    }

    /// Layout the given line of text, at the given font_size.
    /// Subsets of the line can be styled independently with the `runs` parameter.
    /// Generally, you should prefer to use [`Self::shape_line`] instead, which
    /// can be painted directly.
    pub fn layout_line(
        &self,
        text: &str,
        font_size: Pixels,
        runs: &[TextRun],
        force_width: Option<Pixels>,
    ) -> Arc<LineLayout> {
        let mut last_run = None::<&TextRun>;
        let mut last_font: Option<FontId> = None;
        let mut font_runs = self.font_runs_pool.lock().pop().unwrap_or_default();
        font_runs.clear();

        for run in runs.iter() {
            let decoration_changed = if let Some(last_run) = last_run
                && last_run.color == run.color
                && last_run.underline == run.underline
                && last_run.strikethrough == run.strikethrough
            // we do not consider differing background color relevant, as it does not affect glyphs
            // && last_run.background_color == run.background_color
            {
                false
            } else {
                last_run = Some(run);
                true
            };

            if let Some(font_run) = font_runs.last_mut()
                && Some(font_run.font_id) == last_font
                && !decoration_changed
            {
                font_run.len += run.len;
            } else {
                let font_id = self.resolve_font(&run.font);
                last_font = Some(font_id);
                font_runs.push(FontRun {
                    len: run.len,
                    font_id,
                });
            }
        }

        let layout = self.line_layout_cache.layout_line(
            &SharedString::new(text),
            font_size,
            &font_runs,
            force_width,
        );

        self.font_runs_pool.lock().push(font_runs);

        layout
    }
}

#[derive(Hash, Eq, PartialEq)]
struct FontIdWithSize {
    font_id: FontId,
    font_size: Pixels,
}

/// A handle into the text system, which can be used to compute the wrapped layout of text
pub struct LineWrapperHandle {
    wrapper: Option<LineWrapper>,
    text_system: Arc<TextSystem>,
}

impl Drop for LineWrapperHandle {
    fn drop(&mut self) {
        let mut state = self.text_system.wrapper_pool.lock();
        let wrapper = self.wrapper.take().unwrap();
        state
            .get_mut(&FontIdWithSize {
                font_id: wrapper.font_id,
                font_size: wrapper.font_size,
            })
            .unwrap()
            .push(wrapper);
    }
}

impl Deref for LineWrapperHandle {
    type Target = LineWrapper;

    fn deref(&self) -> &Self::Target {
        self.wrapper.as_ref().unwrap()
    }
}

impl DerefMut for LineWrapperHandle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.wrapper.as_mut().unwrap()
    }
}

/// The degree of blackness or stroke thickness of a font. This value ranges from 100.0 to 900.0,
/// with 400.0 as normal.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize, Add, Sub, FromStr)]
#[serde(transparent)]
pub struct FontWeight(pub f32);

impl Display for FontWeight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<f32> for FontWeight {
    fn from(weight: f32) -> Self {
        FontWeight(weight)
    }
}

impl Default for FontWeight {
    #[inline]
    fn default() -> FontWeight {
        FontWeight::NORMAL
    }
}

impl Hash for FontWeight {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(u32::from_be_bytes(self.0.to_be_bytes()));
    }
}

impl Eq for FontWeight {}

impl FontWeight {
    /// Thin weight (100), the thinnest value.
    pub const THIN: FontWeight = FontWeight(100.0);
    /// Extra light weight (200).
    pub const EXTRA_LIGHT: FontWeight = FontWeight(200.0);
    /// Light weight (300).
    pub const LIGHT: FontWeight = FontWeight(300.0);
    /// Normal (400).
    pub const NORMAL: FontWeight = FontWeight(400.0);
    /// Medium weight (500, higher than normal).
    pub const MEDIUM: FontWeight = FontWeight(500.0);
    /// Semibold weight (600).
    pub const SEMIBOLD: FontWeight = FontWeight(600.0);
    /// Bold weight (700).
    pub const BOLD: FontWeight = FontWeight(700.0);
    /// Extra-bold weight (800).
    pub const EXTRA_BOLD: FontWeight = FontWeight(800.0);
    /// Black weight (900), the thickest value.
    pub const BLACK: FontWeight = FontWeight(900.0);

    /// All of the font weights, in order from thinnest to thickest.
    pub const ALL: [FontWeight; 9] = [
        Self::THIN,
        Self::EXTRA_LIGHT,
        Self::LIGHT,
        Self::NORMAL,
        Self::MEDIUM,
        Self::SEMIBOLD,
        Self::BOLD,
        Self::EXTRA_BOLD,
        Self::BLACK,
    ];
}

impl schemars::JsonSchema for FontWeight {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FontWeight".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        use schemars::json_schema;
        json_schema!({
            "type": "number",
            "minimum": Self::THIN,
            "maximum": Self::BLACK,
            "default": Self::default(),
            "description": "Font weight value between 100 (thin) and 900 (black)"
        })
    }
}

/// Allows italic or oblique faces to be selected.
#[derive(Clone, Copy, Eq, PartialEq, Debug, Hash, Default, Serialize, Deserialize, JsonSchema)]
pub enum FontStyle {
    /// A face that is neither italic not obliqued.
    #[default]
    Normal,
    /// A form that is generally cursive in nature.
    Italic,
    /// A typically-sloped version of the regular face.
    Oblique,
}

impl Display for FontStyle {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

/// A styled run of text, for use in [`crate::TextLayout`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRun {
    /// A number of utf8 bytes
    pub len: usize,
    /// The font to use for this run.
    pub font: Font,
    /// The color
    pub color: Hsla,
    /// The background color (if any)
    pub background_color: Option<Hsla>,
    /// The underline style (if any)
    pub underline: Option<UnderlineStyle>,
    /// The strikethrough style (if any)
    pub strikethrough: Option<StrikethroughStyle>,
}

#[cfg(all(target_os = "macos", test))]
impl TextRun {
    fn with_len(&self, len: usize) -> Self {
        let mut this = self.clone();
        this.len = len;
        this
    }
}

/// An identifier for a specific glyph, as returned by [`WindowTextSystem::layout_line`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[repr(C)]
pub struct GlyphId(pub(crate) u32);

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderGlyphParams {
    pub(crate) font_id: FontId,
    pub(crate) glyph_id: GlyphId,
    pub(crate) font_size: Pixels,
    pub(crate) subpixel_variant: Point<u8>,
    pub(crate) scale_factor: f32,
    pub(crate) is_emoji: bool,
    /// Gaussian blur σ applied to the rasterized glyph mask, in **device
    /// pixels**, quantized to 1/2 px by `paint_glyph_with_blur`. Zero means a
    /// sharp glyph.
    pub(crate) blur_radius: Pixels,
    /// Rasterize for continuous sub-pixel motion: use
    /// [`SUBPIXEL_VARIANTS_Y_PRECISE`] instead of the platform default, and
    /// disable vertical grid fitting (DirectWrite otherwise snaps the
    /// fractional baseline back onto the pixel grid).
    ///
    /// This is opt-in so static UI text keeps its crisp, grid-fitted
    /// rendering; animated text (lyrics) uses it via
    /// [`crate::Window::paint_glyph_scaled`].
    pub(crate) subpixel_positioning: bool,
}

impl Eq for RenderGlyphParams {}

impl Hash for RenderGlyphParams {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.font_id.0.hash(state);
        self.glyph_id.0.hash(state);
        self.font_size.0.to_bits().hash(state);
        self.subpixel_variant.hash(state);
        self.scale_factor.to_bits().hash(state);
        self.is_emoji.hash(state);
        self.blur_radius.0.to_bits().hash(state);
        self.subpixel_positioning.hash(state);
    }
}

impl RenderGlyphParams {
    /// Number of sub-pixel bins used on each axis when this glyph is
    /// rasterized. Must be used by platform rasterizers to convert
    /// [`Self::subpixel_variant`] into a device-pixel offset.
    pub(crate) fn subpixel_variant_counts(&self) -> (u8, u8) {
        if self.subpixel_positioning {
            (SUBPIXEL_VARIANTS_X, SUBPIXEL_VARIANTS_Y_PRECISE)
        } else {
            (SUBPIXEL_VARIANTS_X, SUBPIXEL_VARIANTS_Y)
        }
    }

    /// Sub-pixel offset of the glyph, in device pixels.
    pub(crate) fn subpixel_shift(&self) -> Point<f32> {
        let (variants_x, variants_y) = self.subpixel_variant_counts();
        Point {
            x: self.subpixel_variant.x as f32 / variants_x as f32,
            y: self.subpixel_variant.y as f32 / variants_y as f32,
        }
    }
}

/// The configuration details for identifying a specific font.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Font {
    /// The font family name.
    ///
    /// The special name ".SystemUIFont" is used to identify the system UI font, which varies based on platform.
    pub family: SharedString,

    /// The font features to use.
    pub features: FontFeatures,

    /// The fallbacks fonts to use.
    pub fallbacks: Option<FontFallbacks>,

    /// The font weight.
    pub weight: FontWeight,

    /// The font style.
    pub style: FontStyle,
}

/// Get a [`Font`] for a given name.
pub fn font(family: impl Into<SharedString>) -> Font {
    Font {
        family: family.into(),
        features: FontFeatures::default(),
        weight: FontWeight::default(),
        style: FontStyle::default(),
        fallbacks: None,
    }
}

impl Font {
    /// Set this Font to be bold
    pub fn bold(mut self) -> Self {
        self.weight = FontWeight::BOLD;
        self
    }

    /// Set this Font to be italic
    pub fn italic(mut self) -> Self {
        self.style = FontStyle::Italic;
        self
    }
}

/// A struct for storing font metrics.
/// It is used to define the measurements of a typeface.
#[derive(Clone, Copy, Debug)]
pub struct FontMetrics {
    /// The number of font units that make up the "em square",
    /// a scalable grid for determining the size of a typeface.
    pub(crate) units_per_em: u32,

    /// The vertical distance from the baseline of the font to the top of the glyph covers.
    pub(crate) ascent: f32,

    /// The vertical distance from the baseline of the font to the bottom of the glyph covers.
    pub(crate) descent: f32,

    /// The recommended additional space to add between lines of type.
    pub(crate) line_gap: f32,

    /// The suggested position of the underline.
    pub(crate) underline_position: f32,

    /// The suggested thickness of the underline.
    pub(crate) underline_thickness: f32,

    /// The height of a capital letter measured from the baseline of the font.
    pub(crate) cap_height: f32,

    /// The height of a lowercase x.
    pub(crate) x_height: f32,

    /// The outer limits of the area that the font covers.
    /// Corresponds to the xMin / xMax / yMin / yMax values in the OpenType `head` table
    pub(crate) bounding_box: Bounds<f32>,
}

impl FontMetrics {
    /// Returns the vertical distance from the baseline of the font to the top of the glyph covers in pixels.
    pub fn ascent(&self, font_size: Pixels) -> Pixels {
        Pixels((self.ascent / self.units_per_em as f32) * font_size.0)
    }

    /// Returns the vertical distance from the baseline of the font to the bottom of the glyph covers in pixels.
    pub fn descent(&self, font_size: Pixels) -> Pixels {
        Pixels((self.descent / self.units_per_em as f32) * font_size.0)
    }

    /// Returns the recommended additional space to add between lines of type in pixels.
    pub fn line_gap(&self, font_size: Pixels) -> Pixels {
        Pixels((self.line_gap / self.units_per_em as f32) * font_size.0)
    }

    /// Returns the suggested position of the underline in pixels.
    pub fn underline_position(&self, font_size: Pixels) -> Pixels {
        Pixels((self.underline_position / self.units_per_em as f32) * font_size.0)
    }

    /// Returns the suggested thickness of the underline in pixels.
    pub fn underline_thickness(&self, font_size: Pixels) -> Pixels {
        Pixels((self.underline_thickness / self.units_per_em as f32) * font_size.0)
    }

    /// Returns the height of a capital letter measured from the baseline of the font in pixels.
    pub fn cap_height(&self, font_size: Pixels) -> Pixels {
        Pixels((self.cap_height / self.units_per_em as f32) * font_size.0)
    }

    /// Returns the height of a lowercase x in pixels.
    pub fn x_height(&self, font_size: Pixels) -> Pixels {
        Pixels((self.x_height / self.units_per_em as f32) * font_size.0)
    }

    /// Returns the outer limits of the area that the font covers in pixels.
    pub fn bounding_box(&self, font_size: Pixels) -> Bounds<Pixels> {
        (self.bounding_box / self.units_per_em as f32 * font_size.0).map(px)
    }
}

#[allow(unused)]
pub(crate) fn font_name_with_fallbacks<'a>(name: &'a str, system: &'a str) -> &'a str {
    // Note: the "Zed Plex" fonts were deprecated as we are not allowed to use "Plex"
    // in a derived font name. They are essentially indistinguishable from IBM Plex/Lilex,
    // and so retained here for backward compatibility.
    match name {
        ".SystemUIFont" => system,
        ".ZedSans" | "Zed Plex Sans" => "IBM Plex Sans",
        ".ZedMono" | "Zed Plex Mono" => "Lilex",
        _ => name,
    }
}

#[cfg(test)]
mod blur_alpha_tests {
    use super::*;

    fn delta_mask(size: usize) -> Vec<u8> {
        let mut mask = vec![0u8; size * size];
        mask[(size / 2) * size + size / 2] = 255;
        mask
    }

    fn block_mask(size: usize, block: usize) -> Vec<u8> {
        let mut mask = vec![0u8; size * size];
        let start = (size - block) / 2;
        for y in start..start + block {
            for x in start..start + block {
                mask[y * size + x] = 255;
            }
        }
        mask
    }

    fn mass(mask: &[u8]) -> i64 {
        mask.iter().map(|&value| value as i64).sum()
    }

    #[test]
    fn gaussian_is_symmetric_and_preserves_mass() {
        let size = 64;
        let center = size / 2;
        let mut mask = delta_mask(size);
        blur_alpha_mask(&mut mask, size, size, 1.5);

        // 8-bit quantization drops the sub-1/255 tail, so allow a few levels.
        assert!((mass(&mask) - 255).abs() <= 12, "mass={}", mass(&mask));
        for (x, y) in [
            (center - 3, center),
            (center + 3, center),
            (center, center - 4),
            (center, center + 4),
            (center - 2, center + 2),
        ] {
            let mirrored = (2 * center - y) * size + (2 * center - x);
            assert_eq!(mask[y * size + x], mask[mirrored], "at ({x},{y})");
        }
    }

    #[test]
    fn three_box_is_symmetric_and_spreads() {
        let size = 96;
        let center = size / 2;
        for sigma in [2.0, 3.5, 5.0] {
            let mut mask = delta_mask(size);
            blur_alpha_mask(&mut mask, size, size, sigma);

            for (x, y) in [(center - 6, center), (center, center + 9)] {
                let mirrored = (2 * center - y) * size + (2 * center - x);
                let diff = (mask[y * size + x] as i32 - mask[mirrored] as i32).abs();
                assert!(diff <= 1, "sigma={sigma} at ({x},{y}) diff={diff}");
            }
            assert!(
                mask[center * size + center] < 60,
                "sigma={sigma} center={}",
                mask[center * size + center]
            );
            assert!(
                mask[(center + 4) * size + center] > 0,
                "sigma={sigma} did not spread"
            );
        }
    }

    #[test]
    fn block_mass_is_preserved() {
        let size = 96;
        let mask = block_mask(size, 32);
        let before = mass(&mask);
        for sigma in [2.0, 3.5, 5.0] {
            let mut blurred = mask.clone();
            blur_alpha_mask(&mut blurred, size, size, sigma);
            let after = mass(&blurred);
            let ratio = after as f64 / before as f64;
            assert!((ratio - 1.0).abs() < 0.005, "sigma={sigma} ratio={ratio}");
            assert_eq!(blurred[48 * size + 48], 255);
        }
    }
}
