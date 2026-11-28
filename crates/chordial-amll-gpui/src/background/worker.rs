//! AMLL 软件光栅专用 worker。
//!
//! UI 只发布最新快照并轮询结果；worker 独占 renderer 和所有像素缓冲。旧输入直接
//! 覆盖，不建立任务队列，避免 300Hz prepaint 把光栅任务越积越多。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use chordal_core::audio::{AudioBands, AudioControlMapping};
use chordal_core::runtime::lower_current_thread_priority;
use gpui::{Hsla, RenderImage};

use super::{BackgroundRendererKind, LyricBackgroundRenderer};

#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Debug)]
pub(super) struct BackgroundConfig {
    pub kind: BackgroundRendererKind,
    pub solid_color: Option<Hsla>,
    pub fps: u32,
    pub scale: f32,
    pub flow_speed: f32,
    pub static_mode: bool,
    pub beat_strength: f32,
    pub low_freq_volume: f32,
    pub control_mapping: AudioControlMapping,
}

impl Default for BackgroundConfig {
    fn default() -> Self {
        Self {
            kind: BackgroundRendererKind::MeshGradient,
            solid_color: None,
            fps: 30,
            scale: 0.5,
            flow_speed: 4.0,
            static_mode: false,
            beat_strength: 1.0,
            low_freq_volume: 0.0,
            control_mapping: AudioControlMapping::default(),
        }
    }
}

#[derive(Clone)]
pub(super) struct FrameInput {
    state_revision: u64,
    config: BackgroundConfig,
    config_revision: u64,
    palette: Option<Arc<[u8]>>,
    palette_revision: u64,
    viewport: (u32, u32),
    audio: AudioBands,
    playing: bool,
    visible: bool,
}

#[derive(Clone)]
pub(crate) enum FrameOutput {
    Unchanged {
        state_revision: u64,
    },
    Frame {
        state_revision: u64,
        image: Arc<RenderImage>,
        animating: bool,
    },
    Clear {
        state_revision: u64,
        animating: bool,
    },
    Failed {
        state_revision: u64,
    },
}

struct SharedState {
    input: Option<FrameInput>,
    output: Option<FrameOutput>,
    recycled: Vec<Vec<u8>>,
    busy: bool,
    stopped: bool,
}

struct Shared {
    state: Mutex<SharedState>,
    wake: Condvar,
}

#[derive(Clone)]
pub(super) struct BackgroundWorkerHandle {
    shared: Arc<Shared>,
}

impl Drop for BackgroundWorkerHandle {
    fn drop(&mut self) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.stopped = true;
        self.shared.wake.notify_one();
    }
}

impl BackgroundWorkerHandle {
    fn spawn(config: BackgroundConfig) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(SharedState {
                input: None,
                output: None,
                recycled: Vec::new(),
                busy: false,
                stopped: false,
            }),
            wake: Condvar::new(),
        });
        let worker_shared = shared.clone();
        std::thread::Builder::new()
            .name("chordial-amll-raster".into())
            .spawn(move || worker_loop(worker_shared, config))
            .expect("spawn amll raster worker");
        Self { shared }
    }

    fn publish(&self, input: FrameInput) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped {
            state.output = Some(FrameOutput::Failed {
                state_revision: input.state_revision,
            });
            return;
        }
        state.input = Some(input);
        state.busy = true;
        self.shared.wake.notify_one();
    }

    fn poll(&self) -> Option<FrameOutput> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .output
            .take()
    }

    fn discard_output(&self) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .output = None;
    }

    fn is_busy(&self) -> bool {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .busy
    }

    fn recycle(&self, buffer: Vec<u8>) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.stopped && state.recycled.len() < 2 {
            state.recycled.push(buffer);
        }
    }
}

/// UI 侧配置镜像 + worker 句柄，保持原有 setter/query API。
pub(crate) struct BackgroundController {
    handle: BackgroundWorkerHandle,
    config: BackgroundConfig,
    config_revision: u64,
    palette: Option<Arc<[u8]>>,
    palette_revision: u64,
    playing: bool,
    state_revision: u64,
    viewport: (u32, u32),
    visible: bool,
    animating: bool,
}

impl BackgroundController {
    pub fn new(kind: BackgroundRendererKind) -> Self {
        let config = BackgroundConfig {
            kind,
            ..Default::default()
        };
        Self {
            handle: BackgroundWorkerHandle::spawn(config.clone()),
            config,
            config_revision: 0,
            palette: None,
            palette_revision: 0,
            playing: true,
            state_revision: 0,
            viewport: (0, 0),
            visible: true,
            animating: false,
        }
    }

    pub fn kind(&self) -> BackgroundRendererKind {
        self.config.kind
    }

    pub fn solid_color(&self) -> Option<Hsla> {
        self.config.solid_color
    }

    pub fn is_animating(&self) -> bool {
        self.animating || self.handle.is_busy()
    }

    fn invalidate(&mut self) {
        self.state_revision = self.state_revision.wrapping_add(1);
        self.handle.discard_output();
    }

    pub fn set_kind(&mut self, kind: BackgroundRendererKind) {
        if self.config.kind == kind {
            return;
        }
        self.config.kind = kind;
        self.config_revision = self.config_revision.wrapping_add(1);
        self.invalidate();
    }

    pub fn set_solid_color(&mut self, color: Hsla) {
        if self.config.solid_color == Some(color) {
            return;
        }
        self.config.solid_color = Some(color);
        self.config_revision = self.config_revision.wrapping_add(1);
        self.invalidate();
    }

    pub fn set_fps(&mut self, fps: u32) {
        if self.config.fps == fps {
            return;
        }
        self.config.fps = fps;
        self.config_revision = self.config_revision.wrapping_add(1);
        self.invalidate();
    }

    pub fn set_render_scale(&mut self, scale: f32) {
        if self.config.scale.to_bits() == scale.to_bits() {
            return;
        }
        self.config.scale = scale;
        self.config_revision = self.config_revision.wrapping_add(1);
        self.invalidate();
    }

    pub fn set_flow_speed(&mut self, speed: f32) {
        if self.config.flow_speed.to_bits() == speed.to_bits() {
            return;
        }
        self.config.flow_speed = speed;
        self.config_revision = self.config_revision.wrapping_add(1);
        self.invalidate();
    }

    pub fn set_static_mode(&mut self, enable: bool) {
        if self.config.static_mode == enable {
            return;
        }
        self.config.static_mode = enable;
        self.config_revision = self.config_revision.wrapping_add(1);
        self.invalidate();
    }

    pub fn set_beat_strength(&mut self, strength: f32) {
        if self.config.beat_strength.to_bits() == strength.to_bits() {
            return;
        }
        self.config.beat_strength = strength;
        self.config_revision = self.config_revision.wrapping_add(1);
        self.invalidate();
    }

    pub fn set_low_freq_volume(&mut self, volume: f32) {
        if self.config.low_freq_volume.to_bits() == volume.to_bits() {
            return;
        }
        self.config.low_freq_volume = volume;
        self.config_revision = self.config_revision.wrapping_add(1);
        self.invalidate();
    }

    pub fn set_audio_control_mapping(&mut self, mapping: AudioControlMapping) {
        if self.config.control_mapping == mapping {
            return;
        }
        self.config.control_mapping = mapping;
        self.config_revision = self.config_revision.wrapping_add(1);
        self.invalidate();
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    pub fn set_album_palette(&mut self, palette: Vec<u8>) {
        if self.palette.as_deref() == Some(palette.as_slice()) {
            return;
        }
        self.palette = Some(Arc::from(palette));
        self.palette_revision = self.palette_revision.wrapping_add(1);
        self.invalidate();
    }

    pub fn clear_album(&mut self) {
        if self.palette.is_none() {
            return;
        }
        self.palette = None;
        self.palette_revision = self.palette_revision.wrapping_add(1);
        self.invalidate();
    }

    pub fn publish(&mut self, viewport: (u32, u32), audio: AudioBands, visible: bool) {
        if self.viewport != viewport || self.visible != visible {
            self.state_revision = self.state_revision.wrapping_add(1);
            self.handle.discard_output();
        }
        self.viewport = viewport;
        self.visible = visible;
        self.handle.publish(FrameInput {
            state_revision: self.state_revision,
            config: self.config.clone(),
            config_revision: self.config_revision,
            palette: self.palette.clone(),
            palette_revision: self.palette_revision,
            viewport,
            audio,
            playing: self.playing,
            visible,
        });
    }

    /// 接受配置/画布状态一致的最新完成帧。动画 revision 可以落后于正在处理的
    /// 新输入；否则 300Hz UI 会持续丢弃 worker 的上一帧，导致背景永远不显示。
    pub fn poll(&mut self) -> Option<FrameOutput> {
        let output = self.handle.poll()?;
        if output.state_revision() != self.state_revision {
            return None;
        }
        match &output {
            FrameOutput::Frame { animating, .. } | FrameOutput::Clear { animating, .. } => {
                self.animating = *animating
            }
            FrameOutput::Unchanged { .. } => {}
            FrameOutput::Failed { .. } => self.animating = false,
        }
        Some(output)
    }

    pub fn recycle_output(&self, buffer: Vec<u8>) {
        self.handle.recycle(buffer);
    }
}

impl FrameOutput {
    fn state_revision(&self) -> u64 {
        match self {
            Self::Unchanged { state_revision, .. }
            | Self::Frame { state_revision, .. }
            | Self::Clear { state_revision, .. }
            | Self::Failed { state_revision, .. } => *state_revision,
        }
    }
}

fn apply_config(renderer: &mut LyricBackgroundRenderer, config: &BackgroundConfig) {
    renderer.set_fps(config.fps);
    renderer.set_render_scale(config.scale);
    renderer.set_flow_speed(config.flow_speed);
    renderer.set_static_mode(config.static_mode);
    renderer.set_beat_strength(config.beat_strength);
    renderer.set_low_freq_volume(config.low_freq_volume);
    renderer.set_audio_control_mapping(config.control_mapping);
    if let Some(color) = config.solid_color {
        renderer.set_solid_color(color);
    }
}

fn worker_loop(shared: Arc<Shared>, config: BackgroundConfig) {
    lower_current_thread_priority();
    let mut renderer = LyricBackgroundRenderer::new(config.kind);
    apply_config(&mut renderer, &config);
    let mut config_revision = 0u64;
    let mut palette_revision = 0u64;
    let mut viewport = (0u32, 0u32);
    let mut playing = None;
    let mut last_tick: Option<Instant> = None;

    loop {
        let input = {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            while state.input.is_none() && !state.stopped {
                state = shared.wake.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            if state.stopped {
                break;
            }
            let input = state.input.take().expect("checked");
            for buffer in state.recycled.drain(..) {
                renderer.recycle_output(buffer);
            }
            input
        };

        if input.config.kind != renderer.kind() {
            renderer = LyricBackgroundRenderer::new(input.config.kind);
            config_revision = 0;
            palette_revision = 0;
            viewport = (0, 0);
            playing = None;
            last_tick = None;
        }
        if input.config_revision != config_revision {
            apply_config(&mut renderer, &input.config);
            config_revision = input.config_revision;
        }
        if input.palette_revision != palette_revision {
            match &input.palette {
                Some(palette) => renderer.set_album_palette(palette.to_vec()),
                None => renderer.clear_album(),
            }
            palette_revision = input.palette_revision;
        }
        if viewport != input.viewport {
            renderer.set_viewport(input.viewport.0, input.viewport.1);
            viewport = input.viewport;
        }
        renderer.set_audio_bands(input.audio);
        if playing != Some(input.playing) {
            if input.playing {
                renderer.resume();
            } else {
                renderer.pause();
            }
            playing = Some(input.playing);
        }

        let now = Instant::now();
        let (output, next_tick) = if !input.visible
            || matches!(
                input.config.kind,
                BackgroundRendererKind::Css | BackgroundRendererKind::None
            ) {
            (
                FrameOutput::Clear {
                    state_revision: input.state_revision,
                    animating: false,
                },
                None,
            )
        } else {
            let dt = last_tick
                .map(|previous| now.saturating_duration_since(previous).as_secs_f32() * 1000.0)
                .unwrap_or(16.0)
                .clamp(0.0, 100.0);
            let result = catch_unwind(AssertUnwindSafe(|| renderer.render(dt)));
            match result {
                Ok(Some(image)) => (
                    FrameOutput::Frame {
                        state_revision: input.state_revision,
                        image,
                        animating: renderer.is_animating(),
                    },
                    Some(now),
                ),
                Ok(None) => (
                    FrameOutput::Unchanged {
                        state_revision: input.state_revision,
                    },
                    Some(now),
                ),
                Err(_) => {
                    tracing::error!("amll raster worker panicked; background disabled");
                    (
                        FrameOutput::Failed {
                            state_revision: input.state_revision,
                        },
                        None,
                    )
                }
            }
        };
        last_tick = next_tick;

        let failed = matches!(output, FrameOutput::Failed { .. });
        {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            state.output = Some(output);
            state.busy = state.input.is_some();
            if failed {
                state.stopped = true;
            }
        }
        if failed {
            break;
        }
    }
    renderer.dispose();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for_current(controller: &mut BackgroundController) -> FrameOutput {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(output) = controller.poll() {
                return output;
            }
            assert!(
                Instant::now() < deadline,
                "background worker did not publish"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn renderer_and_images_are_send() {
        let _guard = TEST_LOCK.lock().unwrap();
        fn assert_send<T: Send>() {}
        assert_send::<LyricBackgroundRenderer>();
        assert_send::<Arc<RenderImage>>();
    }

    #[test]
    fn mesh_worker_emits_frame_and_css_clears() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut controller = BackgroundController::new(BackgroundRendererKind::MeshGradient);
        controller.set_fps(0);
        controller.set_album_palette(vec![255; 32 * 32 * 4]);
        controller.publish((16, 16), AudioBands::default(), true);
        assert!(matches!(
            wait_for_current(&mut controller),
            FrameOutput::Frame { .. }
        ));

        controller.set_kind(BackgroundRendererKind::Css);
        controller.publish((16, 16), AudioBands::default(), true);
        assert!(matches!(
            wait_for_current(&mut controller),
            FrameOutput::Clear { .. }
        ));
    }

    #[test]
    fn zero_viewport_is_unchanged_not_clear() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut controller = BackgroundController::new(BackgroundRendererKind::MeshGradient);
        controller.publish((0, 0), AudioBands::default(), true);
        assert!(matches!(
            wait_for_current(&mut controller),
            FrameOutput::Unchanged { .. }
        ));
    }

    #[test]
    fn recycled_buffer_can_cross_threads_and_hidden_state_recovers() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut controller = BackgroundController::new(BackgroundRendererKind::MeshGradient);
        controller.set_fps(0);
        controller.set_album_palette(vec![255; 32 * 32 * 4]);
        controller.publish((16, 16), AudioBands::default(), true);
        assert!(matches!(
            wait_for_current(&mut controller),
            FrameOutput::Frame { .. }
        ));

        controller.recycle_output(vec![0; 16 * 16 * 4]);
        controller.publish((16, 16), AudioBands::default(), false);
        assert!(matches!(
            wait_for_current(&mut controller),
            FrameOutput::Clear { .. }
        ));
        controller.publish((16, 16), AudioBands::default(), true);
        assert!(matches!(
            wait_for_current(&mut controller),
            FrameOutput::Frame { .. }
        ));
    }

    #[test]
    fn identical_config_updates_do_not_invalidate_frames() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut controller = BackgroundController::new(BackgroundRendererKind::MeshGradient);
        let initial = controller.state_revision;
        controller.set_kind(BackgroundRendererKind::MeshGradient);
        controller.set_fps(30);
        controller.set_render_scale(0.5);
        controller.set_flow_speed(4.0);
        controller.set_static_mode(false);
        controller.set_beat_strength(1.0);
        controller.set_low_freq_volume(0.0);
        controller.set_audio_control_mapping(AudioControlMapping::default());
        controller.clear_album();
        assert_eq!(controller.state_revision, initial);

        controller.set_render_scale(0.25);
        let changed = controller.state_revision;
        assert_ne!(changed, initial);
        controller.set_render_scale(0.25);
        assert_eq!(controller.state_revision, changed);

        controller.set_album_palette(vec![0; 32 * 32 * 4]);
        let palette_changed = controller.state_revision;
        controller.set_album_palette(vec![0; 32 * 32 * 4]);
        assert_eq!(controller.state_revision, palette_changed);
    }

    #[test]
    fn poll_accepts_lagging_frame_only_for_current_state() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut controller = BackgroundController::new(BackgroundRendererKind::MeshGradient);
        controller.state_revision = 3;
        controller.handle.shared.state.lock().unwrap().output =
            Some(FrameOutput::Unchanged { state_revision: 3 });
        assert!(matches!(
            controller.poll(),
            Some(FrameOutput::Unchanged { .. })
        ));

        controller.state_revision = 4;
        controller.handle.shared.state.lock().unwrap().output =
            Some(FrameOutput::Unchanged { state_revision: 3 });
        assert!(controller.poll().is_none());
    }

    #[test]
    fn stale_state_frame_is_discarded() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut controller = BackgroundController::new(BackgroundRendererKind::MeshGradient);
        controller.publish((8, 8), AudioBands::default(), true);
        controller.set_kind(BackgroundRendererKind::Css);
        controller.publish((8, 8), AudioBands::default(), true);
        let output = wait_for_current(&mut controller);
        assert_eq!(output.state_revision(), controller.state_revision);
        assert!(matches!(output, FrameOutput::Clear { .. }));
    }
}
