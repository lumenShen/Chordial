//! 单行文本输入框（IME / 中文输入支持）。
//!
//! 结构参照 gpui 0.2.2 的 IME 管线：
//! - `EntityInputHandler` 实现（UTF-16 偏移 ↔ 字节偏移，marked text = 拼音组合串）；
//! - 自定义 `Element`：request_measured_layout 定尺寸，paint 时
//!   `window.handle_input(ElementInputHandler)` 接入平台 IME；
//! - 动作集 `ChordInput` 上下文键位（backspace/left/right/enter/...）。
//!
//! 刻意保持单行最小实现：无纵向滚动、无换行（溢出裁剪），足以支撑搜索框。

use std::ops::Range;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId, Hsla, InspectorElementId,
    IntoElement, KeyBinding, MouseButton, MouseMoveEvent, PaintQuad, ParentElement, Pixels, Render,
    SharedString, Stateful, Style, TextAlign, TextRun, UTF16Selection, Window, actions, div, fill,
    point, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

const INPUT_FONT_SIZE: f32 = 13.0;
const INPUT_LINE_HEIGHT: f32 = 18.0;
const INPUT_PADDING_X: f32 = 10.0;
const INPUT_PADDING_Y: f32 = 8.0;

actions!(
    chord_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Submit
    ]
);

pub fn install_input_keybindings(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("ChordInput")),
        KeyBinding::new("delete", Delete, Some("ChordInput")),
        KeyBinding::new("left", Left, Some("ChordInput")),
        KeyBinding::new("right", Right, Some("ChordInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("ChordInput")),
        KeyBinding::new("shift-right", SelectRight, Some("ChordInput")),
        KeyBinding::new("ctrl-a", SelectAll, Some("ChordInput")),
        KeyBinding::new("home", Home, Some("ChordInput")),
        KeyBinding::new("end", End, Some("ChordInput")),
        KeyBinding::new("ctrl-c", Copy, Some("ChordInput")),
        KeyBinding::new("ctrl-x", Cut, Some("ChordInput")),
        KeyBinding::new("ctrl-v", Paste, Some("ChordInput")),
        KeyBinding::new("enter", Submit, Some("ChordInput")),
    ]);
}

/// 输入状态实体（与视图分离，便于外部读写）。
pub struct InputState {
    focus_handle: FocusHandle,
    content: String,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<WrappedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    submit_locked: bool,
}

#[allow(dead_code)]
impl InputState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: String::new(),
            placeholder: "".into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            submit_locked: false,
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    /// 外部设置输入内容（设置面板回填用）。
    pub fn set_content(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        if self.content == text {
            return;
        }
        self.content = text;
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    pub fn set_placeholder(&mut self, p: impl Into<SharedString>) {
        self.placeholder = p.into();
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.content.is_empty() {
            return;
        }
        self.content.clear();
        self.selected_range = 0..0;
        self.marked_range = None;
        cx.notify();
    }

    // ---- 内部游标工具 ----

    fn floor_boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.content.len());
        while offset > 0 && !self.content.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    fn clamp_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self.floor_boundary(range.start);
        let end = self.floor_boundary(range.end.max(start));
        start..end
    }

    fn replace_range(&mut self, range: Range<usize>, new_text: &str, cx: &mut Context<Self>) {
        let range = self.clamp_range(range);
        if range.start > range.end || range.end > self.content.len() {
            return;
        }
        // 单行：换行替换为空格。
        let new_text: String = new_text
            .chars()
            .map(|ch| if ch == '\r' || ch == '\n' { ' ' } else { ch })
            .collect();
        self.content.replace_range(range.clone(), &new_text);
        let caret = range.start + new_text.len();
        self.selected_range = caret..caret;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.floor_boundary(offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.floor_boundary(offset);
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    fn cursor_offset(&self) -> usize {
        self.selected_range.start
    }

    // ---- UTF-16 ↔ 字节 ----

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    // ---- 编辑动作 ----

    fn backspace(&mut self, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let start = self.previous_boundary(self.cursor_offset());
            self.select_to(start, cx);
        }
        self.replace_range(self.selected_range.clone(), "", cx);
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let end = self.next_boundary(self.cursor_offset());
            self.select_to(end, cx);
        }
        self.replace_range(self.selected_range.clone(), "", cx);
    }

    fn left(&mut self, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn home(&mut self, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard()
            && let Some(text) = item.text()
        {
            self.replace_range(self.selected_range.clone(), &text, cx);
        }
    }

    fn cut(&mut self, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_range(self.selected_range.clone(), "", cx);
        }
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn submit(&mut self, _cx: &mut Context<Self>) -> Option<SubmitRequest> {
        if self.submit_locked {
            return None;
        }
        Some(SubmitRequest {
            text: self.content.clone(),
        })
    }

    /// 占位防抖：宿主处理 submit 前若再次触发忽略。
    pub fn set_submit_locked(&mut self, locked: bool) {
        self.submit_locked = locked;
    }

    fn index_for_mouse_x(&self, x_in_field: Pixels) -> usize {
        let (Some(bounds), Some(line)) = (&self.last_bounds, &self.last_layout) else {
            return 0;
        };
        let x = (x_in_field - bounds.left() - px(INPUT_PADDING_X)).max(px(0.0));
        match line.index_for_position(point(x, px(0.0)), px(INPUT_LINE_HEIGHT)) {
            Ok(i) | Err(i) => i.min(self.content.len()),
        }
    }
}

impl Focusable for InputState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for InputState {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.clamp_range(self.range_from_utf16(&range_utf16));
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        self.replace_range(range, new_text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let range = self.clamp_range(range);
        let new_text: String = new_text
            .chars()
            .map(|ch| if ch == '\r' || ch == '\n' { ' ' } else { ch })
            .collect();
        self.content.replace_range(range.clone(), &new_text);
        let insert_end = range.start + new_text.len();
        self.marked_range = if new_text.is_empty() {
            None
        } else {
            Some(range.start..insert_end)
        };
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .map(|r| range.start + r.start..range.start + r.end)
            .unwrap_or(insert_end..insert_end);
        self.selection_reversed = false;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = line.position_for_index(range.start, px(INPUT_LINE_HEIGHT))?;
        let end = line.position_for_index(range.end, px(INPUT_LINE_HEIGHT))?;
        Some(Bounds::from_corners(
            point(bounds.left() + start.x, bounds.top() + start.y),
            point(
                bounds.left() + end.x,
                bounds.top() + end.y + px(INPUT_LINE_HEIGHT),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let local = bounds.localize(&position)?;
        Some(self.offset_to_utf16(self.index_for_mouse_x(local.x)))
    }
}

struct SubmitRequest {
    text: String,
}

// ---------- Element ----------

struct InputElement {
    state: Entity<InputState>,
}

struct PrepaintState {
    line: WrappedLine,
    selection: Option<PaintQuad>,
    caret: Option<PaintQuad>,
}

impl IntoElement for InputElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for InputElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let state = self.state.clone();
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        let font_family = crate::theme::Theme::get(cx).font_family.clone();
        let text_color = crate::theme::Theme::get(cx).text;
        let faint_color = crate::theme::Theme::get(cx).text_faint;
        let layout_id =
            window.request_measured_layout(style, move |_known, available, window, cx| {
                let width = match available.width {
                    gpui::AvailableSpace::Definite(w) => w,
                    _ => px(480.0),
                };
                let state = state.read(cx);
                let is_placeholder = state.content.is_empty();
                let text = if is_placeholder {
                    state.placeholder.to_string()
                } else {
                    state.content.clone()
                };
                let color = if is_placeholder {
                    faint_color
                } else {
                    text_color
                };
                let _line = shape_line(&text, color, &font_family, width, window);
                size(width, px(INPUT_LINE_HEIGHT + INPUT_PADDING_Y * 2.0))
            });
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let state = self.state.clone();
        let (text, is_placeholder) = state.update(cx, |s, _| {
            let ph = s.content.is_empty();
            (
                if ph {
                    s.placeholder.to_string()
                } else {
                    s.content.clone()
                },
                ph,
            )
        });
        let width = bounds.size.width - px(INPUT_PADDING_X * 2.0);
        let theme = crate::theme::Theme::get(cx);
        let color = if is_placeholder {
            theme.text_faint
        } else {
            theme.text
        };
        let font_family = theme.font_family.clone();
        let line = shape_line(&text, color, &font_family, width, window).unwrap_or_default();
        let _ = is_placeholder;

        let focused = self.state.read(cx).focus_handle.is_focused(window);
        let (mut selection, mut caret) = (None, None);
        if focused {
            let state = self.state.read(cx);
            let sel = state.selected_range.clone();
            if !sel.is_empty() {
                let start_x = line
                    .position_for_index(sel.start, px(INPUT_LINE_HEIGHT))
                    .map(|p| p.x)
                    .unwrap_or(px(0.0));
                let end_x = line
                    .position_for_index(sel.end, px(INPUT_LINE_HEIGHT))
                    .map(|p| p.x)
                    .unwrap_or(px(0.0));
                let color = crate::theme::Theme::get(cx).selection;
                selection = Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + px(INPUT_PADDING_X) + start_x,
                            bounds.top() + px(INPUT_PADDING_Y),
                        ),
                        point(
                            bounds.left() + px(INPUT_PADDING_X) + end_x,
                            bounds.top() + px(INPUT_PADDING_Y) + px(INPUT_LINE_HEIGHT),
                        ),
                    ),
                    color,
                ));
            }
            // 组合串期间不画光标（IME 自己画下划线区域由 marked text 处理）。
            if state.marked_range.is_none() {
                let caret_x = line
                    .position_for_index(state.cursor_offset(), px(INPUT_LINE_HEIGHT))
                    .map(|p| p.x)
                    .unwrap_or(px(0.0));
                caret = Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + px(INPUT_PADDING_X) + caret_x,
                            bounds.top() + px(INPUT_PADDING_Y),
                        ),
                        point(
                            bounds.left() + px(INPUT_PADDING_X) + caret_x + px(2.0),
                            bounds.top() + px(INPUT_PADDING_Y) + px(INPUT_LINE_HEIGHT),
                        ),
                    ),
                    crate::theme::Theme::get(cx).text,
                ));
            }
        }
        let _ = is_placeholder;
        PrepaintState {
            line,
            selection,
            caret,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // 接入平台 IME / 键盘输入。
        let focus_handle = self.state.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.state.clone()),
            cx,
        );

        // 颜色在 shape_line 阶段已烘焙进 TextRun（占位符灰、正文亮）。
        let line = prepaint.line.clone();
        let selection = prepaint.selection.take();
        let caret = prepaint.caret.take();

        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if let Some(q) = selection {
                window.paint_quad(q);
            }
            let origin = point(
                bounds.left() + px(INPUT_PADDING_X),
                bounds.top() + px(INPUT_PADDING_Y),
            );
            line.paint(
                origin,
                px(INPUT_LINE_HEIGHT),
                TextAlign::Left,
                Some(bounds),
                window,
                cx,
            )
            .ok();
            if let Some(q) = caret {
                window.paint_quad(q);
            }
        });

        self.state.update(cx, |s, _| {
            s.last_bounds = Some(bounds);
            s.last_layout = Some(line);
        });
    }
}

type WrappedLine = gpui::WrappedLine;

fn shape_line(
    text: &str,
    color: Hsla,
    font_family: &str,
    _width: Pixels,
    window: &mut Window,
) -> Option<gpui::WrappedLine> {
    if text.is_empty() {
        return Some(gpui::WrappedLine::default());
    }
    let run = TextRun {
        len: text.len(),
        font: gpui::font(font_family.to_string()),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_text(
            text.to_string().into(),
            px(INPUT_FONT_SIZE),
            &[run],
            None,
            None,
        )
        .ok()
        .and_then(|lines| lines.into_iter().next())
}

// ---------- 视图包装 ----------

pub struct InputView {
    state: Entity<InputState>,
    id: SharedString,
    on_submit: Option<Rc<dyn Fn(String, &mut Window, &mut App)>>,
    on_blur: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

#[allow(dead_code)]
impl InputView {
    pub fn new(state: Entity<InputState>, cx: &mut Context<Self>) -> Self {
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        Self {
            state,
            id: "search-input".into(),
            on_submit: None,
            on_blur: None,
        }
    }

    /// 多个输入框同时存在时用不同 id，避免 gpui 元素 id 冲突。
    pub fn with_id(mut self, id: impl Into<SharedString>) -> Self {
        self.id = id.into();
        self
    }

    pub fn on_submit(mut self, f: impl Fn(String, &mut Window, &mut App) + 'static) -> Self {
        self.on_submit = Some(Rc::new(f));
        self
    }

    pub fn on_blur(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_blur = Some(Rc::new(f));
        self
    }
}

impl Render for InputView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        let on_submit = self.on_submit.clone();
        let on_blur = self.on_blur.clone();

        let focus_handle = state.read(cx).focus_handle.clone();
        let focused = focus_handle.is_focused(window);

        let mut el: Stateful<gpui::Div> = div()
            .id(self.id.clone())
            .flex()
            .items_center()
            .w_full()
            .min_h(px(INPUT_LINE_HEIGHT + INPUT_PADDING_Y * 2.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(if focused {
                crate::theme::Theme::get(cx).focus_ring
            } else {
                crate::theme::Theme::get(cx).border
            })
            .bg(crate::theme::Theme::get(cx).surface_alt)
            .overflow_hidden()
            .cursor(CursorStyle::IBeam)
            .key_context("ChordInput")
            .track_focus(&focus_handle)
            .child(InputElement {
                state: state.clone(),
            })
            // 动作绑定
            .on_action(cx.listener(|this: &mut Self, _: &Backspace, _, cx| {
                this.state.update(cx, |s, cx| s.backspace(cx));
            }))
            .on_action(cx.listener(|this: &mut Self, _: &Delete, _, cx| {
                this.state.update(cx, |s, cx| s.delete(cx));
            }))
            .on_action(cx.listener(|this: &mut Self, _: &Left, _, cx| {
                this.state.update(cx, |s, cx| s.left(cx));
            }))
            .on_action(cx.listener(|this: &mut Self, _: &Right, _, cx| {
                this.state.update(cx, |s, cx| s.right(cx));
            }))
            .on_action(cx.listener(|this: &mut Self, _: &SelectLeft, _, cx| {
                this.state.update(cx, |s, cx| s.select_left(cx));
            }))
            .on_action(cx.listener(|this: &mut Self, _: &SelectRight, _, cx| {
                this.state.update(cx, |s, cx| s.select_right(cx));
            }))
            .on_action(cx.listener(|this: &mut Self, _: &SelectAll, _, cx| {
                this.state.update(cx, |s, cx| s.select_all(cx));
            }))
            .on_action(cx.listener(|this: &mut Self, _: &Home, _, cx| {
                this.state.update(cx, |s, cx| s.home(cx));
            }))
            .on_action(cx.listener(|this: &mut Self, _: &End, _, cx| {
                this.state.update(cx, |s, cx| s.end(cx));
            }))
            .on_action(cx.listener(|this: &mut Self, _: &Paste, _, cx| {
                this.state.update(cx, |s, cx| s.paste(cx));
            }))
            .on_action(cx.listener(|this: &mut Self, _: &Cut, _, cx| {
                this.state.update(cx, |s, cx| s.cut(cx));
            }))
            .on_action(cx.listener(|this: &mut Self, _: &Copy, _, cx| {
                this.state.update(cx, |s, cx| s.copy(cx));
            }));

        // Submit 与 Blur 需要在状态释放后回调宿主。
        let submit_state = state.clone();
        el = el.on_action(move |_: &Submit, window: &mut Window, cx: &mut App| {
            let request = submit_state.update(cx, |s, cx| s.submit(cx));
            if let Some(req) = request
                && let Some(f) = on_submit.clone()
            {
                f(req.text, window, cx);
            }
        });
        let blur_state = state.clone();
        el = el.on_action(
            move |_: &crate::actions::Blur, window: &mut Window, _cx: &mut App| {
                window.blur();
                let _ = blur_state;
                if let Some(f) = on_blur.clone() {
                    f(window, _cx);
                }
            },
        );

        // 点击聚焦 + 定位光标。
        let down_state = state.clone();
        el = el.on_mouse_down(
            MouseButton::Left,
            move |event: &gpui::MouseDownEvent, window, cx| {
                let x = event.position.x;
                window.focus(&down_state.read(cx).focus_handle.clone());
                down_state.update(cx, |s, cx| {
                    let offset = s.index_for_mouse_x(x);
                    s.move_to(offset, cx);
                });
            },
        );
        let move_state = state.clone();
        el = el.on_mouse_move(move |event: &MouseMoveEvent, _window, cx| {
            // 按住拖动扩展选区（简化：仅左键按下时）。
            if event.pressed_button == Some(MouseButton::Left) {
                move_state.update(cx, |s, cx| {
                    let offset = s.index_for_mouse_x(event.position.x);
                    s.select_to(offset, cx);
                });
            }
        });

        el
    }
}

/// 供宿主在窗口创建时聚焦输入框。
pub fn focus_input(state: &Entity<InputState>, window: &mut Window, cx: &mut App) {
    let handle = state.read(cx).focus_handle.clone();
    window.focus(&handle);
}
