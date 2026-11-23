//! `ChordialApp` 的 navigation 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    pub(super) fn enter_immersive(&mut self, cx: &mut Context<Self>) {
        self.immersive = true;
        self.sheet_drag = None;
        // Dock→全屏：第一帧就是 Dock 的条状卡片，随后与共享封面/曲名
        // 一起放大到满窗。
        self.immersive_expand = Some(SpringSettle::new(
            1.0,
            0.0,
            0.0,
            Instant::now(),
            Self::EXPAND_DAMPING,
            Self::EXPAND_STIFFNESS,
        ));
        self.portrait_page = PortraitPage::Art;
        self.portrait_transition = None;
        self.portrait_enter = Some(Instant::now());
        self.portrait_last_input = Instant::now();
        self.portrait_chrome_hidden = false;
        self.portrait_chrome_at = None;
        self.portrait_toast = None;
        self.hide_drawer_now();
        self.wide_lyrics_hidden = false;
        self.stage_motion = None;
        self.ensure_portrait_idle_ticker(cx);
        // AMLL：页面打开时所有 MediaButton 的 SVG 内容重新挂载 → pressed 回弹；
        // 上一首/下一首的双箭头同时播放一次合并入场动画。
        self.ui_anim_enter = Some(Instant::now());
        self.ui_anim_play = None;
        self.ui_anim_shuffle = None;
        self.ui_anim_repeat = None;
        self.ui_anim_volume = None;
        self.last_volume_input = self.player_status.volume;
        self.quality_dialog_open = false;
        self.cover_scale_anim = None;
        self.ensure_ui_anim_ticker(cx);
        // 沉浸层会接管 `lyric_view` 的样式与背景；先撤销手机壳的 Mesh
        // 背景标记，退出沉浸后下一帧才能重新套用手机播放页样式。
        self.phone_backdrop = PhoneBackdrop::Off;
        let wide = !self.force_portrait
            && self.viewport_width >= self.viewport_height
            && self.viewport_width >= MIN_WIDE_SPLIT_W;
        let white = gpui::Hsla {
            h: 0.0,
            s: 0.0,
            l: 1.0,
            a: 1.0,
        };
        self.lyric_view.update(cx, |view, _| {
            // 沉浸层背景 = AMLL 的 Mesh Gradient（动画），铺满整个窗口：
            // 光栅按低分辨率计算，由 GPU 双线性放大到屏幕。
            view.set_text_visible(wide);
            view.set_click_seek_enabled(wide);
            // 浅色主题在主面板用纯色，但沉浸层必须用封面驱动的 Mesh，
            // 否则会得到一块纯黑/纯纸色背景。
            view.set_background_kind(BackgroundRendererKind::MeshGradient);
            view.set_background_visible(!self.no_bg);
            view.set_background_color(gpui::rgb(0x111111).into());
            view.set_background_fps(30);
            view.set_background_flow_speed(2.0);
            view.set_colors(LyricRenderColors {
                text: white,
                // AMLL lyricSubLine 的 0.3 透明度由渲染层统一乘，这里给纯白。
                translation: white,
                overlay: gpui::Hsla {
                    h: 0.0,
                    s: 0.0,
                    l: 0.0,
                    a: 0.0,
                },
                interlude_dot: white,
                fallback_background: gpui::Hsla {
                    h: 0.0,
                    s: 0.0,
                    l: 0.0,
                    a: 1.0,
                },
                glow: white,
            });
        });
    }

    // ---------- 播放页（AMLL 沉浸层 + 竖屏子页面） ----------

    /// 兼容旧入口：统一进入 AMLL 沉浸层；竖向窗口使用竖屏布局。
    pub(super) fn open_portrait(&mut self, cx: &mut Context<Self>) {
        self.settings_open = false;
        self.login_open = false;
        self.eq_open = false;
        self.force_portrait = true;
        self.enter_immersive(cx);
        cx.notify();
    }

    /// 主面板的歌词入口：宽屏沉浸层直接展示右侧歌词（关闭状态会被重置）；
    /// 竖屏窗口不承载歌词，只打开同一套播放页。
    pub(super) fn open_lyrics_immersive(&mut self, cx: &mut Context<Self>) {
        self.settings_open = false;
        self.login_open = false;
        self.eq_open = false;
        self.force_portrait = false;
        self.enter_immersive(cx);
        cx.notify();
    }

    pub(super) fn close_portrait(&mut self, cx: &mut Context<Self>) {
        self.exit_immersive(cx);
        cx.notify();
    }

    /// 任意输入都会重置自动隐藏计时；隐藏状态下则立刻淡入。
    pub(super) fn bump_portrait(&mut self, cx: &mut Context<Self>) {
        self.portrait_last_input = Instant::now();
        if self.portrait_chrome_hidden {
            self.portrait_chrome_hidden = false;
            self.portrait_chrome_at = Some(Instant::now());
            self.ensure_ui_anim_ticker(cx);
        }
        self.ensure_portrait_idle_ticker(cx);
    }

    pub(super) fn set_portrait_page(&mut self, page: PortraitPage, cx: &mut Context<Self>) {
        let from = self
            .portrait_transition
            .map(|t| t.to)
            .unwrap_or(self.portrait_page);
        if from == page {
            return;
        }
        let now = Instant::now();
        // FLIP：先记录当前屏幕上的共享元素几何（可能正处于上一段补间中），
        // 再切换页面，最后补间到新页面的页头/大封面几何。播放页在宽屏
        // 歌词可见时是 WideSplit，因此不能只看 `portrait_page` 的默认模态。
        let art_mode = self.immersive_stage_mode(cx, self.viewport_width, self.viewport_height);
        let current_mode = if self.portrait_page == PortraitPage::Art {
            art_mode
        } else {
            Self::stage_mode_for_page(self.portrait_page)
        };
        let target_mode = if page == PortraitPage::Art {
            art_mode
        } else {
            Self::stage_mode_for_page(page)
        };
        // 队列是覆盖在播放页上的子页面，整页纵向位移。
        let moving_page = from != page;
        if moving_page {
            let transition = Transition::new(from, page);
            self.stage_motion =
                Some(self.layout_motion_to(target_mode, current_mode, now, transition.duration_ms));
            self.portrait_transition = Some(transition);
        } else {
            self.portrait_transition = None;
        }
        self.portrait_page = page;
        if page == PortraitPage::Queue
            && let Some(position) = self.queue_pos
        {
            // 虚拟列表只渲染可视行；进入队列时把当前曲目滚到视口中心。
            self.queue_scroll
                .scroll_to_item(position, gpui::ScrollStrategy::Center);
        }
        self.portrait_last_input = Instant::now();
        self.ensure_ui_anim_ticker(cx);
        self.ensure_portrait_idle_ticker(cx);
        cx.notify();
    }

    pub(super) fn stage_mode_for_page(page: PortraitPage) -> StageMode {
        match page {
            PortraitPage::Art => StageMode::CompactArt,
            PortraitPage::Queue => StageMode::CompactQueue,
        }
    }

    /// 构造一段 First→Last 补间。`fallback` 是当前静止模态；若上一段补间
    /// 尚未结束，则从它的当前插值几何继续，避免连续点击时画面跳变。
    pub(super) fn layout_motion_to(
        &self,
        to_mode: StageMode,
        fallback: StageMode,
        now: Instant,
        duration_ms: f32,
    ) -> LayoutMotion {
        let from = self.current_stage_geometry(fallback, now);
        let to = immersive_geometry(to_mode, self.viewport_width, self.viewport_height);
        LayoutMotion::starting_at(fallback, from, to_mode, to, now, duration_ms)
    }

    /// 当前屏幕上的共享元素几何：补间进行中就用插值结果。
    pub(super) fn current_stage_geometry(&self, fallback: StageMode, now: Instant) -> StageGeometry {
        if let Some(motion) = self.stage_motion.filter(|motion| motion.active(now)) {
            return motion.geometry(now);
        }
        immersive_geometry(fallback, self.viewport_width, self.viewport_height)
    }

    /// 当前是否使用布局 1（宽屏/类平板 + 歌词可见）。
    ///
    /// 布局 1 的唯一判据是「横向窗口且歌词可见」；竖屏、无歌词、用户收起歌词
    /// 都回落到布局 2。这样布局不再由两套页面分别决定，避免再次漂移。
    pub(super) fn layout_one_active(&self, cx: &App, width: f32, height: f32) -> bool {
        !self.force_portrait
            && width >= height
            && width >= MIN_WIDE_SPLIT_W
            && self.lyric_view.read(cx).has_lyrics()
            && !self.wide_lyrics_hidden
    }

    /// 宽屏沉浸层：歌词开/关在「布局 1」与「布局 2」之间切换，
    /// 播放器列与歌词列走同一套共享元素 FLIP。
    pub(super) fn toggle_wide_lyrics(&mut self, cx: &mut Context<Self>) {
        if !self.layout_can_toggle_lyrics(cx) {
            return;
        }
        let now = Instant::now();
        let current_mode = if self.wide_lyrics_hidden {
            StageMode::CompactArt
        } else {
            StageMode::WideSplit
        };
        self.wide_lyrics_hidden = !self.wide_lyrics_hidden;
        let target_mode = if self.wide_lyrics_hidden {
            StageMode::CompactArt
        } else {
            StageMode::WideSplit
        };
        self.stage_motion = Some(self.layout_motion_to(target_mode, current_mode, now, 320.0));
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    /// 歌词开关只对「横向窗口 + 当前有歌词」有意义。
    pub(super) fn layout_can_toggle_lyrics(&self, cx: &App) -> bool {
        !self.force_portrait
            && self.viewport_width >= self.viewport_height
            && self.viewport_width >= MIN_WIDE_SPLIT_W
            && self.lyric_view.read(cx).has_lyrics()
    }

    // ---------- 断点与手机壳状态 ----------

    pub(super) fn is_phone(&self) -> bool {
        self.viewport_class.is_phone()
    }

    /// 手机壳（非沉浸层）才有「播放页 / 曲库页」的切换。
    pub(super) fn phone_shell_active(&self) -> bool {
        self.is_phone() && !self.immersive
    }

    pub(super) fn show_phone_browse(&mut self) {
        if self.is_phone() {
            self.phone_page = PhonePage::Browse;
            self.phone_sheet = None;
            self.phone_sheet_spring = None;
            self.phone_sheet_anchor = None;
            self.phone_edge_swipe = None;
            self.selection = None;
            self.track_menu = None;
        }
    }

    /// 从手机曲库选中曲目后回到播放页；播放解析是异步的，不能等
    /// `current_track` 落定才切页，否则首次播放会停在曲库页。
    pub(super) fn show_phone_player(&mut self, cx: &mut Context<Self>) {
        if self.is_phone() {
            self.open_phone_sheet(cx);
        }
    }

    // ---- 手机壳 mini player ⇄ 播放页 ----

    /// 播放页 sheet 当前展开比例：0 = mini player，1 = 全屏播放页。
    pub(super) fn phone_sheet_progress(&self, now: Instant) -> f32 {
        if let Some(drag) = self.phone_sheet {
            return drag.progress(now);
        }
        if let Some(spring) = self.phone_sheet_spring {
            return spring.position(now).clamp(0.0, 1.0);
        }
        if self.phone_page == PhonePage::Player {
            1.0
        } else {
            0.0
        }
    }

    pub(super) fn phone_sheet_animating(&self, now: Instant) -> bool {
        self.phone_sheet.is_some_and(|drag| drag.animating(now))
            || self
                .phone_sheet_spring
                .is_some_and(|spring| !spring.settled(now))
    }

    /// 点击 mini player / 从曲库选歌：从当前位置弹性展开播放页。
    pub(super) fn open_phone_sheet(&mut self, cx: &mut Context<Self>) {
        if !self.is_phone() {
            return;
        }
        let now = Instant::now();
        let from = self.phone_sheet_progress(now);
        self.phone_page = PhonePage::Player;
        self.phone_sheet = None;
        self.phone_sheet_anchor = None;
        self.track_menu = None;
        self.selection = None;
        if from < 0.999 {
            self.phone_sheet_spring = Some(SpringSettle::new(from, 1.0, 0.0, now, 25.0, 190.0));
            self.ensure_ui_anim_ticker(cx);
        }
        cx.notify();
    }

    /// 播放页下拉 / 返回：收起成 mini player。
    pub(super) fn close_phone_sheet(&mut self, cx: &mut Context<Self>) {
        if !self.is_phone() || self.phone_page != PhonePage::Player {
            return;
        }
        let now = Instant::now();
        let from = self.phone_sheet_progress(now);
        self.phone_sheet = None;
        self.phone_sheet_spring = Some(SpringSettle::new(from, 0.0, 0.0, now, 26.0, 190.0));
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    pub(super) fn begin_phone_sheet_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        if !self.is_phone() || matches!(self.phone_page, PhonePage::Lyrics) {
            return;
        }
        let now = Instant::now();
        let start = self.phone_sheet_progress(now);
        self.phone_sheet_spring = None;
        self.phone_sheet = Some(SheetSwipe::begin(start, y, now));
        // 从 mini player 起手先把播放页挂出来，让第一帧就跟手；收起时
        // 页面会停在 Player，等 spring/手势落点决定是否回 Browse。
        if start < 0.5 {
            self.phone_page = PhonePage::Player;
        }
        cx.notify();
    }

    pub(super) fn update_phone_sheet_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        let height = self.viewport_height.max(1.0);
        let now = Instant::now();
        if let Some(drag) = &mut self.phone_sheet
            && drag.update(y, height, now)
        {
            self.ensure_ui_anim_ticker(cx);
            cx.notify();
        }
    }

    pub(super) fn release_phone_sheet_drag(&mut self, cx: &mut Context<Self>) {
        let height = self.viewport_height.max(1.0);
        let now = Instant::now();
        if let Some(drag) = &mut self.phone_sheet {
            if drag.release(height, now) {
                self.ensure_ui_anim_ticker(cx);
            } else if !drag.engaged() {
                // 只是按住/轻点：按当前进度落在最近的一端，不留下一个
                // 永远停在 Tracking 的手势。
                let progress = drag.progress(now);
                self.phone_sheet = None;
                self.phone_page = if progress >= 0.5 {
                    PhonePage::Player
                } else {
                    PhonePage::Browse
                };
            }
            cx.notify();
        }
    }

    /// mini player 上按住：先记起点，等指针真正上移再接管，避免点击
    /// 展开与拖拽展开互相打架。
    pub(super) fn press_phone_mini_player(&mut self, y: f32, cx: &mut Context<Self>) {
        if self.phone_page == PhonePage::Browse {
            self.phone_sheet_anchor = Some(y);
            cx.notify();
        }
    }

    pub(super) fn drag_phone_mini_player(&mut self, y: f32, cx: &mut Context<Self>) {
        let Some(start) = self.phone_sheet_anchor else {
            return;
        };
        if start - y <= 4.0 {
            return;
        }
        self.phone_sheet_anchor = None;
        let now = Instant::now();
        self.phone_sheet_spring = None;
        self.phone_page = PhonePage::Player;
        self.phone_sheet = Some(SheetSwipe::begin(0.0, start, now));
        self.update_phone_sheet_drag(y, cx);
    }

    pub(super) fn release_phone_mini_player(&mut self, cx: &mut Context<Self>) {
        let had_anchor = self.phone_sheet_anchor.take().is_some();
        if self.phone_sheet.is_some() {
            self.release_phone_sheet_drag(cx);
        } else if had_anchor {
            // 没越过死区：等价于点击 mini player。
            self.open_phone_sheet(cx);
        }
        cx.notify();
    }

    // ---- 长按多选 ----

    pub(super) fn start_long_press(
        &mut self,
        list: QueueList,
        index: usize,
        x: f32,
        y: f32,
        cx: &mut Context<Self>,
    ) {
        if !self.is_phone() || self.selection.is_some() {
            return;
        }
        let now = Instant::now();
        self.long_press = Some((list, index, now));
        self.long_press_pos = Some((x, y));
        self.long_press_gen = self.long_press_gen.wrapping_add(1);
        let generation = self.long_press_gen;
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(480))
                .await;
            let _ = weak.update(cx, |app, cx| {
                if app.long_press_gen != generation {
                    return;
                }
                if let Some((list, index, _)) = app.long_press.take() {
                    app.long_press_pos = None;
                    app.enter_selection(list, index, cx);
                    app.row_click_suppress_until =
                        Some(Instant::now() + Duration::from_millis(450));
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// 按住后移动超过 6px 视为滚动，不再触发长按多选。
    pub(super) fn cancel_long_press_on_move(&mut self, x: f32, y: f32) {
        if let Some((sx, sy)) = self.long_press_pos
            && ((x - sx).abs() > 6.0 || (y - sy).abs() > 6.0)
        {
            self.cancel_long_press();
        }
    }

    pub(super) fn cancel_long_press(&mut self) {
        self.long_press = None;
        self.long_press_pos = None;
        self.long_press_gen = self.long_press_gen.wrapping_add(1);
    }

    pub(super) fn row_click_suppressed(&self) -> bool {
        self.row_click_suppress_until
            .is_some_and(|until| Instant::now() < until)
    }

    // ---- 队列拖动排序 ----

    pub(super) fn begin_queue_drag(&mut self, from: usize, y: f32, cx: &mut Context<Self>) {
        if from >= self.queue.len() {
            return;
        }
        self.queue_drag = Some(QueueDragState {
            from,
            to: from,
            start_y: y,
            engaged: false,
        });
        self.cancel_long_press();
        cx.notify();
    }

    pub(super) fn update_queue_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        let Some(drag) = &mut self.queue_drag else {
            return;
        };
        if (y - drag.start_y).abs() > 4.0 {
            drag.engaged = true;
        }
        if drag.engaged {
            let delta = ((y - drag.start_y) / crate::theme::size::ROW).round() as i32;
            let to = (drag.from as i32 + delta).clamp(0, self.queue.len().saturating_sub(1) as i32)
                as usize;
            if to != drag.to {
                drag.to = to;
                cx.notify();
            }
        }
    }

    pub(super) fn release_queue_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.queue_drag.take() else {
            return;
        };
        if !drag.engaged || drag.from == drag.to {
            cx.notify();
            return;
        }
        self.row_click_suppress_until = Some(Instant::now() + Duration::from_millis(300));
        let mut queue = self.queue.as_ref().clone();
        if drag.from < queue.len() {
            let track = queue.remove(drag.from);
            let to = drag.to.min(queue.len());
            queue.insert(to, track);
            self.queue = Arc::new(queue);
            self.queue_pos = self.queue_pos.map(|pos| {
                if pos == drag.from {
                    to
                } else if drag.from < pos && to >= pos {
                    pos - 1
                } else if drag.from > pos && to <= pos {
                    pos + 1
                } else {
                    pos
                }
            });
            self.show_portrait_toast("已调整播放顺序", cx);
        }
        cx.notify();
    }

    // ---- 封面 ⇄ 歌词 ----

    pub(super) fn begin_phone_edge_swipe(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let (forward, back) = match self.phone_page {
            PhonePage::Player => (true, false),
            PhonePage::Lyrics => (false, true),
            PhonePage::Browse => return,
        };
        self.phone_edge_swipe = Some(EdgeSwipe::begin(x, y, Instant::now(), forward, back));
        cx.notify();
    }

    pub(super) fn update_phone_edge_swipe(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let now = Instant::now();
        if let Some(swipe) = &mut self.phone_edge_swipe
            && swipe.update(x, y, now)
        {
            self.ensure_ui_anim_ticker(cx);
            cx.notify();
        }
    }

    pub(super) fn release_phone_edge_swipe(&mut self, cx: &mut Context<Self>) {
        let width = self.viewport_width.max(1.0);
        let now = Instant::now();
        if let Some(swipe) = &mut self.phone_edge_swipe {
            if swipe.engaged() {
                self.phone_swipe_suppress_click = Some(now);
            }
            if swipe.release(width, now) {
                self.ensure_ui_anim_ticker(cx);
                cx.notify();
            } else if !swipe.engaged() {
                self.phone_edge_swipe = None;
            }
        }
    }

    pub(super) fn phone_swipe_click_suppressed(&self) -> bool {
        self.phone_swipe_suppress_click
            .is_some_and(|at| at.elapsed() < Duration::from_millis(260))
    }

    // ---- 曲目菜单 / 多选 ----

    /// 当前可见列表的快照（与 `play_from_list` 同源）。
    pub(super) fn snapshot_list(&self, list: QueueList) -> Arc<Vec<Track>> {
        match list {
            QueueList::Search => match &self.source_route {
                Some(route) => Arc::new(
                    self.search_tracks
                        .iter()
                        .filter(|t| &t.id.source == route)
                        .cloned()
                        .collect(),
                ),
                None => self.search_tracks.clone(),
            },
            QueueList::Library => self.library.clone(),
            QueueList::Queue => self.queue.clone(),
            QueueList::Collection => self
                .collection
                .as_ref()
                .map(|c| c.tracks.clone())
                .unwrap_or_else(|| Arc::new(Vec::new())),
        }
    }

    pub(super) fn open_track_menu(
        &mut self,
        list: QueueList,
        index: usize,
        x: f32,
        y: f32,
        cx: &mut Context<Self>,
    ) {
        let tracks = self.snapshot_list(list);
        let Some(track) = tracks.get(index).cloned() else {
            return;
        };
        let is_current = self
            .queue_pos
            .and_then(|pos| self.queue.get(pos))
            .is_some_and(|current| current.id == track.id);
        let in_queue = self.queue.iter().any(|item| item.id == track.id);
        // 「⋮」是在 mouse-down 上打开的，随后的 mouse-up 可能被行当成
        // click；短暂吞掉，避免打开菜单的同时把歌切了。
        self.row_click_suppress_until = Some(Instant::now() + Duration::from_millis(260));
        self.track_menu = Some(TrackMenuState {
            list,
            index,
            has_album: track.album.as_ref().and_then(|a| a.id.as_ref()).is_some(),
            has_artist: track.artists.iter().any(|a| a.id.is_some()),
            is_current,
            in_queue,
            track,
            x,
            y,
        });
        cx.notify();
    }

    pub(super) fn close_track_menu(&mut self, cx: &mut Context<Self>) {
        if self.track_menu.take().is_some() {
            cx.notify();
        }
    }

    /// 下一首播放：插在当前曲目之后（没有当前曲目则排到队首）。
    pub(super) fn queue_play_next(&mut self, track: Track, cx: &mut Context<Self>) {
        let mut queue = self.queue.as_ref().clone();
        let insert_at = self
            .queue_pos
            .map(|pos| (pos + 1).min(queue.len()))
            .unwrap_or(0);
        queue.insert(insert_at, track);
        if self.queue_pos.is_none() {
            // 未在播放时仅入队，不抢占播放。
            self.queue_pos = None;
        }
        self.queue = Arc::new(queue);
        self.show_portrait_toast("已设为下一首播放", cx);
        cx.notify();
    }

    /// 追加到播放队列末尾。
    pub(super) fn queue_append(&mut self, track: Track, cx: &mut Context<Self>) {
        let mut queue = self.queue.as_ref().clone();
        queue.push(track);
        self.queue = Arc::new(queue);
        self.show_portrait_toast("已添加到播放队列", cx);
        cx.notify();
    }

    /// 按曲目 id 从队列移除（同曲重复只移除一项）。
    pub(super) fn remove_track_from_queue(
        &mut self,
        id: &chordal_core::model::TrackId,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.queue.iter().position(|t| &t.id == id) else {
            return;
        };
        self.remove_queue_at(index, cx);
    }

    pub(super) fn enter_selection(&mut self, list: QueueList, index: usize, cx: &mut Context<Self>) {
        let tracks = self.snapshot_list(list);
        if index >= tracks.len() {
            return;
        }
        let mut selected = vec![false; tracks.len()];
        selected[index] = true;
        self.selection = Some(SelectionState {
            list,
            tracks,
            selected,
        });
        self.track_menu = None;
        cx.notify();
    }

    pub(super) fn exit_selection(&mut self, cx: &mut Context<Self>) {
        if self.selection.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn toggle_selection(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(selection) = &mut self.selection {
            selection.toggle(index);
            cx.notify();
        }
    }

    pub(super) fn select_all_visible(&mut self, cx: &mut Context<Self>) {
        if let Some(selection) = &mut self.selection {
            selection.select_all();
            cx.notify();
        }
    }

    /// 多选批量动作。`action` 决定把选中项播放 / 下一首 / 入队 / 移除。
    pub(super) fn apply_selection(&mut self, action: SelectionAction, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.take() else {
            return;
        };
        let tracks = selection.selected_tracks();
        if tracks.is_empty() {
            cx.notify();
            return;
        }
        match action {
            SelectionAction::Play => {
                self.queue = Arc::new(tracks);
                self.queue_pos = Some(0);
                self.status_message = None;
                self.resolve_and_play(cx);
                self.open_phone_sheet(cx);
            }
            SelectionAction::PlayNext => {
                let mut queue = self.queue.as_ref().clone();
                let at = self
                    .queue_pos
                    .map(|pos| (pos + 1).min(queue.len()))
                    .unwrap_or(0);
                for (offset, track) in tracks.into_iter().enumerate() {
                    queue.insert(at + offset, track);
                }
                self.queue = Arc::new(queue);
                self.show_portrait_toast("已设为下一首播放", cx);
            }
            SelectionAction::Append => {
                let mut queue = self.queue.as_ref().clone();
                queue.extend(tracks);
                self.queue = Arc::new(queue);
                self.show_portrait_toast("已添加到播放队列", cx);
            }
            SelectionAction::Remove => {
                let ids = tracks.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
                let mut queue = self.queue.as_ref().clone();
                let current_id = self
                    .queue_pos
                    .and_then(|pos| queue.get(pos))
                    .map(|t| t.id.clone());
                queue.retain(|t| !ids.contains(&t.id));
                self.queue = Arc::new(queue);
                self.queue_pos =
                    current_id.and_then(|id| self.queue.iter().position(|t| t.id == id));
                self.show_portrait_toast("已从队列移除", cx);
            }
        }
        cx.notify();
    }

    pub(super) fn open_album_for_track(&mut self, track: &Track, cx: &mut Context<Self>) {
        let Some(album) = track.album.clone() else {
            return;
        };
        let Some(id) = album.id else {
            return;
        };
        self.open_collection(
            CollectionId::new(track.id.source.clone(), CollectionKind::Album, id),
            album.name,
            cx,
        );
    }

    pub(super) fn open_artist_for_track(&mut self, track: &Track, cx: &mut Context<Self>) {
        let Some(artist) = track.artists.iter().find(|a| a.id.is_some()).cloned() else {
            return;
        };
        let Some(id) = artist.id else {
            return;
        };
        self.open_collection(
            CollectionId::new(track.id.source.clone(), CollectionKind::Artist, id),
            artist.name,
            cx,
        );
    }

    /// 跨断点时收敛一次跨模块状态。
    ///
    /// 需要处理的只有「哪些视图属于当前布局」：手机壳没有 Dock/侧栏，
    /// 离开手机壳时抽屉与手机背景必须交还给桌面/沉浸层，否则会留下
    /// 一个无入口可关的浮层或一块黑色 Mesh 背景。
    pub(super) fn handle_viewport_class_change(&mut self, next: ViewportClass, cx: &mut Context<Self>) {
        let previous = self.viewport_class;
        if previous == next {
            return;
        }
        self.viewport_class = next;
        match (previous, next) {
            (_, ViewportClass::Phone) => {
                self.hide_drawer_now();
                // 首次收窄：保留搜索/集合/队列的浏览上下文；普通曲库
                // 且有曲目时直接落在播放页（参考手机播放器）。
                self.phone_page = phone_default_page(
                    self.view_tab,
                    self.library_filter,
                    self.current_track.is_some(),
                );
                // 不重置 `force_portrait`：它是用户主动进入竖屏播放页的
                // 选择，跨断点后仍应保留，交给沉浸层自己判断布局。
            }
            (ViewportClass::Phone, _) => {
                self.hide_drawer_now();
                self.phone_page = PhonePage::Player;
            }
            _ => {}
        }
        if !self.immersive {
            self.sync_phone_backdrop(cx);
        }
        cx.notify();
    }

    /// 手机壳与沉浸层共用 `lyric_view`。这里把「播放页背景 / 歌词页 /
    /// 不占用」收敛成一次显式迁移，避免每帧重置或两个页面同时渲染。
    pub(super) fn sync_phone_backdrop(&mut self, cx: &mut Context<Self>) {
        let mode = if self.phone_shell_active() && self.current_track.is_some() {
            match self.phone_page {
                PhonePage::Player => PhoneBackdrop::Player,
                PhonePage::Lyrics => PhoneBackdrop::Lyrics,
                PhonePage::Browse => PhoneBackdrop::Off,
            }
        } else {
            PhoneBackdrop::Off
        };
        if mode == self.phone_backdrop {
            return;
        }
        self.phone_backdrop = mode;
        if mode != PhoneBackdrop::Off {
            let (width, height) = (self.viewport_width, self.viewport_height);
            let scale = self.background_render_scale();
            let no_bg = self.no_bg;
            let no_blur = self.no_blur;
            let sweep = self.lyric_sweep_fade_em;
            let theme = Theme::get(cx).clone();
            let show_text = mode == PhoneBackdrop::Lyrics;
            let style = if show_text {
                phone_lyric_style(&theme, width, height, sweep)
            } else {
                immersive_lyric_style(width, height, sweep)
            };
            let white = gpui::white();
            self.lyric_view.update(cx, |view, _| {
                view.set_text_visible(show_text);
                view.set_click_seek_enabled(show_text);
                view.set_player_config(LyricPlayerConfig {
                    align_anchor: if show_text {
                        AlignAnchor::Center
                    } else {
                        AlignAnchor::Top
                    },
                    align_position: if show_text { 0.5 } else { 0.1 },
                    enable_blur: !no_blur,
                    ..LyricPlayerConfig::default()
                });
                view.set_background_kind(BackgroundRendererKind::MeshGradient);
                view.set_background_visible(!no_bg);
                view.set_background_bounds(None);
                view.set_background_render_scale(scale);
                view.set_background_color(gpui::rgb(0x111111).into());
                view.set_background_fps(30);
                view.set_background_flow_speed(2.0);
                view.set_colors(LyricRenderColors {
                    text: white,
                    translation: white,
                    overlay: gpui::Hsla {
                        h: 0.0,
                        s: 0.0,
                        l: 0.0,
                        a: 0.0,
                    },
                    interlude_dot: white,
                    // 没有封面时 Mesh 会退化成纯黑；给手机壳一个
                    // 偏蓝的近黑底，至少和参考图的“暗色玻璃”一致。
                    fallback_background: gpui::rgb(0x171820).into(),
                    glow: white,
                });
                view.set_text_style(style);
            });
        } else if !self.immersive {
            let theme = Theme::get(cx).clone();
            self.lyric_view.update(cx, |view, _| {
                view.set_text_visible(true);
                view.set_click_seek_enabled(true);
                view.set_background_visible(true);
                view.set_background_bounds(None);
                view.set_background_kind(if theme.is_dark() {
                    BackgroundRendererKind::MeshGradient
                } else {
                    BackgroundRendererKind::Css
                });
                view.set_colors(LyricRenderColors {
                    text: theme.text,
                    translation: theme.text_dim,
                    overlay: theme.overlay,
                    interlude_dot: theme.text,
                    fallback_background: theme.bg,
                    glow: theme.playing,
                });
                view.set_text_style(compact_lyric_style(&theme, self.lyric_sweep_fade_em));
            });
        }
    }

}
