use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow};
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, DisplayHandle, HasDisplayHandle, HasWindowHandle,
    RawDisplayHandle, RawWindowHandle, WindowHandle,
};

use crate::platform::blade::{BladeContext, BladeRenderer, BladeSurfaceConfig};
use crate::{
    AnyWindowHandle, Bounds, Capslock, Decorations, DevicePixels, DispatchEventResult, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformAtlas, PlatformInput,
    PlatformInputHandler, PlatformWindow, Point, PromptButton, PromptLevel, RequestFrameOptions,
    Size, WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowControlArea,
    WindowControls, WindowParams, point, px, size,
};

use super::bridge::{AndroidBridge, acquire_native_window, release_native_window};
use super::keyboard::{keystroke_for_keycode, modifiers_from_android};

pub(crate) struct AndroidWindowInner {
    gpu: Arc<BladeContext>,
    bridge: Arc<AndroidBridge>,
    state: RefCell<AndroidWindowState>,
    callbacks: RefCell<AndroidWindowCallbacks>,
}

struct AndroidWindowState {
    native_window: Option<NonNull<c_void>>,
    renderer: Option<BladeRenderer>,
    bounds: Bounds<crate::Pixels>,
    scale: f32,
    appearance: WindowAppearance,
    background_appearance: WindowBackgroundAppearance,
    input_handler: Option<PlatformInputHandler>,
    /// 软键盘是否处于我们请求显示的状态；用于“失去输入焦点后自动收起”。
    ime_visible: bool,
    mouse_position: Point<crate::Pixels>,
    modifiers: Modifiers,
    touch_down: bool,
    active: bool,
    maximized: bool,
    fullscreen: bool,
    title: String,
}

#[derive(Default)]
struct AndroidWindowCallbacks {
    request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    input: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    active_status_change: Option<Box<dyn FnMut(bool)>>,
    hover_status_change: Option<Box<dyn FnMut(bool)>>,
    resize: Option<Box<dyn FnMut(Size<crate::Pixels>, f32)>>,
    moved: Option<Box<dyn FnMut()>>,
    should_close: Option<Box<dyn FnMut() -> bool>>,
    close: Option<Box<dyn FnOnce()>>,
    appearance_changed: Option<Box<dyn FnMut()>>,
}

pub(crate) struct AndroidWindow(Rc<AndroidWindowInner>);

impl AndroidWindow {
    pub(crate) fn new(
        _handle: AnyWindowHandle,
        _options: WindowParams,
        gpu: Arc<BladeContext>,
        bridge: Arc<AndroidBridge>,
        appearance: WindowAppearance,
    ) -> Result<Self> {
        let native_window = bridge
            .wait_for_surface()
            .ok_or_else(|| anyhow!("Android Surface did not become ready"))?;
        unsafe { acquire_native_window(native_window) };
        let (width, height, scale) = bridge.surface_size();
        let bounds = Bounds::new(
            Point::default(),
            size(px(width / scale), px(height / scale)),
        );
        let renderer = BladeRenderer::new(
            gpu.as_ref(),
            &AndroidRawWindow {
                window: native_window,
                display: AndroidDisplayHandle::new(),
            },
            BladeSurfaceConfig {
                size: blade_graphics::Extent {
                    width: width as u32,
                    height: height as u32,
                    depth: 1,
                },
                transparent: false,
            },
        )
        .context("failed to create Android Blade renderer")?;

        Ok(Self(Rc::new(AndroidWindowInner {
            gpu,
            bridge,
            state: RefCell::new(AndroidWindowState {
                native_window: Some(native_window),
                renderer: Some(renderer),
                bounds,
                scale,
                appearance,
                background_appearance: WindowBackgroundAppearance::Opaque,
                input_handler: None,
                ime_visible: false,
                mouse_position: Point::default(),
                modifiers: Modifiers::default(),
                touch_down: false,
                active: true,
                maximized: false,
                fullscreen: false,
                title: String::new(),
            }),
            callbacks: RefCell::new(AndroidWindowCallbacks::default()),
        })))
    }

    pub(crate) fn downgrade(&self) -> std::rc::Weak<AndroidWindowInner> {
        Rc::downgrade(&self.0)
    }
}

impl Drop for AndroidWindow {
    fn drop(&mut self) {
        self.0.on_surface_destroyed();
    }
}

impl AndroidWindowInner {
    fn dispatch_input(&self, input: PlatformInput) {
        if let Some(callback) = self.callbacks.borrow_mut().input.as_mut() {
            callback(input);
        }
    }

    fn set_active(&self, active: bool) {
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.active != active;
            state.active = active;
            changed
        };
        if changed {
            if let Some(callback) = self.callbacks.borrow_mut().active_status_change.as_mut() {
                callback(active);
            }
        }
    }

    pub(crate) fn on_frame(&self) {
        // on_frame 发生在两次 draw 之间：此时 input_handler 是否为空就是
        // 上一帧的焦点状态（draw 内部的 take/set 不会持续到这里）。
        let ime_action = {
            let mut state = self.state.borrow_mut();
            let focused = state.input_handler.is_some();
            if focused && !state.ime_visible {
                state.ime_visible = true;
                Some(true)
            } else if !focused && state.ime_visible {
                state.ime_visible = false;
                Some(false)
            } else {
                None
            }
        };
        if let Some(visible) = ime_action {
            self.bridge.set_ime_visible(visible);
        }
        if let Some(callback) = self.callbacks.borrow_mut().request_frame.as_mut() {
            callback(RequestFrameOptions::default());
        }
    }

    pub(crate) fn on_surface_created(&self) {
        let mut state = self.state.borrow_mut();
        if state.renderer.is_some() {
            return;
        }
        let Some(native_window) = self.bridge.surface() else {
            return;
        };
        unsafe { acquire_native_window(native_window) };
        let (width, height, scale) = self.bridge.surface_size();
        let renderer = match BladeRenderer::new(
            self.gpu.as_ref(),
            &AndroidRawWindow {
                window: native_window,
                display: AndroidDisplayHandle::new(),
            },
            BladeSurfaceConfig {
                size: blade_graphics::Extent {
                    width: width as u32,
                    height: height as u32,
                    depth: 1,
                },
                transparent: false,
            },
        ) {
            Ok(renderer) => renderer,
            Err(error) => {
                unsafe { release_native_window(native_window) };
                log::error!("failed to recreate Android renderer: {error:#}");
                return;
            }
        };
        state.native_window = Some(native_window);
        state.renderer = Some(renderer);
        state.scale = scale;
        state.bounds = Bounds::new(
            Point::default(),
            size(px(width / scale), px(height / scale)),
        );
        drop(state);
        self.on_surface_changed(width, height, scale);
        self.set_active(true);
    }

    pub(crate) fn on_surface_changed(&self, width: f32, height: f32, scale: f32) {
        let logical_size = size(px(width / scale), px(height / scale));
        {
            let mut state = self.state.borrow_mut();
            state.bounds = Bounds::new(Point::default(), logical_size);
            state.scale = scale;
            if let Some(renderer) = state.renderer.as_mut() {
                renderer.update_drawable_size(Size {
                    width: DevicePixels(width as i32),
                    height: DevicePixels(height as i32),
                });
            }
        }
        if let Some(callback) = self.callbacks.borrow_mut().resize.as_mut() {
            callback(logical_size, scale);
        }
        self.on_frame();
    }

    pub(crate) fn on_surface_destroyed(&self) {
        let native_window = {
            let mut state = self.state.borrow_mut();
            if let Some(mut renderer) = state.renderer.take() {
                renderer.destroy();
            }
            state.native_window.take()
        };
        if let Some(native_window) = native_window {
            unsafe { release_native_window(native_window) };
        }
        self.set_active(false);
    }

    pub(crate) fn on_touch(&self, action: i32, pointer_id: i32, x: f32, y: f32, pressure: f32) {
        if pointer_id != 0 {
            return;
        }
        // MotionEvent 给的是物理像素，gpui 的窗口坐标是逻辑像素。
        // 不除以 density 所有命中测试都会落到窗口外。
        let scale = {
            let state = self.state.borrow();
            if state.scale.is_finite() && state.scale > 0.0 {
                state.scale
            } else {
                1.0
            }
        };
        let position = point(px(x / scale), px(y / scale));
        let modifiers = self.state.borrow().modifiers;
        log::debug!(
            "android touch action={action} raw=({x}, {y}) scale={scale} logical=({}, {})",
            x / scale,
            y / scale
        );
        let _ = pressure;
        match action {
            0 | 5 => {
                self.state.borrow_mut().touch_down = true;
                self.state.borrow_mut().mouse_position = position;
                self.dispatch_input(PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers,
                    click_count: 1,
                    first_mouse: true,
                }));
            }
            2 => {
                let pressed_button = if self.state.borrow().touch_down {
                    Some(MouseButton::Left)
                } else {
                    None
                };
                self.state.borrow_mut().mouse_position = position;
                self.dispatch_input(PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    pressed_button,
                    modifiers,
                }));
            }
            1 | 6 => {
                self.state.borrow_mut().touch_down = false;
                self.state.borrow_mut().mouse_position = position;
                self.dispatch_input(PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers,
                    click_count: 1,
                }));
            }
            3 => {
                self.state.borrow_mut().touch_down = false;
                self.dispatch_input(PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers,
                    click_count: 1,
                }));
            }
            _ => {}
        }
    }

    pub(crate) fn on_key(&self, key_code: i32, action: i32, meta_state: i32) {
        let modifiers = modifiers_from_android(meta_state);
        self.state.borrow_mut().modifiers = modifiers;
        let keystroke = keystroke_for_keycode(key_code, modifiers);
        if keystroke.key.is_empty() {
            return;
        }
        match action {
            0 => self.dispatch_input(PlatformInput::KeyDown(crate::KeyDownEvent {
                keystroke,
                is_held: false,
            })),
            1 => self.dispatch_input(PlatformInput::KeyUp(crate::KeyUpEvent { keystroke })),
            _ => {}
        }
    }

    pub(crate) fn on_back(&self) {
        let modifiers = Modifiers::default();
        let keystroke = crate::Keystroke {
            modifiers,
            key: "escape".to_owned(),
            key_char: None,
        };
        self.dispatch_input(PlatformInput::KeyDown(crate::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
        }));
        self.dispatch_input(PlatformInput::KeyUp(crate::KeyUpEvent { keystroke }));
    }

    pub(crate) fn on_lifecycle(&self, resumed: bool) {
        self.set_active(resumed);
        if resumed {
            self.on_frame();
        }
    }

    pub(crate) fn on_ime(&self, text: &str, replacement_start: i32, replacement_end: i32) {
        let range = if replacement_start >= 0 && replacement_end >= replacement_start {
            Some(replacement_start as usize..replacement_end as usize)
        } else {
            None
        };
        if let Some(handler) = self.state.borrow_mut().input_handler.as_mut() {
            handler.replace_text_in_range(range, text);
        }
    }
}

struct AndroidRawWindow {
    window: NonNull<c_void>,
    display: AndroidDisplayHandle,
}

impl HasWindowHandle for AndroidRawWindow {
    fn window_handle(
        &self,
    ) -> std::result::Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(
                self.window.cast(),
            )))
        })
    }
}

impl HasDisplayHandle for AndroidRawWindow {
    fn display_handle(
        &self,
    ) -> std::result::Result<DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(unsafe { DisplayHandle::borrow_raw(RawDisplayHandle::Android(self.display)) })
    }
}

impl HasWindowHandle for AndroidWindow {
    fn window_handle(
        &self,
    ) -> std::result::Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        let window = self
            .0
            .state
            .borrow()
            .native_window
            .ok_or(raw_window_handle::HandleError::NotSupported)?;
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(
                window.cast(),
            )))
        })
    }
}

impl HasDisplayHandle for AndroidWindow {
    fn display_handle(
        &self,
    ) -> std::result::Result<DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Android(AndroidDisplayHandle::new()))
        })
    }
}

impl PlatformWindow for AndroidWindow {
    fn bounds(&self) -> Bounds<crate::Pixels> {
        self.0.state.borrow().bounds
    }

    fn is_maximized(&self) -> bool {
        self.0.state.borrow().maximized
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Windowed(self.bounds())
    }

    fn content_size(&self) -> Size<crate::Pixels> {
        self.0.state.borrow().bounds.size
    }

    fn resize(&mut self, _size: Size<crate::Pixels>) {}

    fn scale_factor(&self) -> f32 {
        self.0.state.borrow().scale
    }

    fn appearance(&self) -> WindowAppearance {
        self.0.state.borrow().appearance
    }

    fn display(&self) -> Option<Rc<dyn crate::PlatformDisplay>> {
        None
    }

    fn mouse_position(&self) -> Point<crate::Pixels> {
        self.0.state.borrow().mouse_position
    }

    fn modifiers(&self) -> Modifiers {
        self.0.state.borrow().modifiers
    }

    fn capslock(&self) -> Capslock {
        Capslock::default()
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.state.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.state.borrow_mut().input_handler.take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        None
    }

    fn activate(&self) {}
    fn is_active(&self) -> bool {
        self.0.state.borrow().active
    }
    fn is_hovered(&self) -> bool {
        true
    }

    fn set_title(&mut self, title: &str) {
        self.0.state.borrow_mut().title = title.to_owned();
    }

    fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
        self.0.state.borrow_mut().background_appearance = background_appearance;
    }

    fn minimize(&self) {}
    fn zoom(&self) {}
    fn toggle_fullscreen(&self) {}
    fn is_fullscreen(&self) -> bool {
        self.0.state.borrow().fullscreen
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.callbacks.borrow_mut().request_frame = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.0.callbacks.borrow_mut().input = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.callbacks.borrow_mut().active_status_change = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.callbacks.borrow_mut().hover_status_change = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<crate::Pixels>, f32)>) {
        self.0.callbacks.borrow_mut().resize = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.0.callbacks.borrow_mut().moved = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.callbacks.borrow_mut().should_close = Some(callback);
    }

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.callbacks.borrow_mut().close = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.callbacks.borrow_mut().appearance_changed = Some(callback);
    }

    fn draw(&self, scene: &crate::Scene) {
        let mut state = self.0.state.borrow_mut();
        if let Some(renderer) = state.renderer.as_mut() {
            renderer.draw(scene);
        }
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0
            .state
            .borrow()
            .renderer
            .as_ref()
            .expect("Android renderer must exist before the first frame")
            .sprite_atlas()
            .clone()
    }

    fn window_decorations(&self) -> Decorations {
        Decorations::Server
    }

    fn window_controls(&self) -> WindowControls {
        WindowControls {
            fullscreen: true,
            maximize: false,
            minimize: false,
            window_menu: false,
        }
    }

    fn gpu_specs(&self) -> Option<crate::GpuSpecs> {
        self.0
            .state
            .borrow()
            .renderer
            .as_ref()
            .map(BladeRenderer::gpu_specs)
    }

    fn update_ime_position(&self, _bounds: Bounds<crate::Pixels>) {
        // 文本输入获得焦点 / 光标移动时 gpui 会调用这里，借机唤起软键盘。
        let should_show = {
            let mut state = self.0.state.borrow_mut();
            if state.ime_visible {
                false
            } else {
                state.ime_visible = true;
                true
            }
        };
        if should_show {
            self.0.bridge.set_ime_visible(true);
        }
    }
}
