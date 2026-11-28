use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow};
use futures::channel::oneshot;

use crate::platform::blade::BladeContext;
use crate::{
    Action, AnyWindowHandle, BackgroundExecutor, ClipboardItem, CursorStyle, ForegroundExecutor,
    Keymap, Menu, MenuItem, PathPromptOptions, Platform, PlatformDisplay, PlatformKeyboardLayout,
    PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, Task, WindowAppearance,
    WindowParams,
};

use super::bridge::{AndroidBridge, AndroidEvent, current_bridge};
use super::dispatcher::{AndroidDispatcher, PlatformMessage};
use super::display::AndroidDisplay;
use super::keyboard::AndroidKeyboardLayout;
use super::text_system::CosmicTextSystem;
use super::window::{AndroidWindow, AndroidWindowInner};

pub(crate) struct AndroidPlatform {
    bridge: Arc<AndroidBridge>,
    dispatcher: Arc<AndroidDispatcher>,
    text_system: Arc<CosmicTextSystem>,
    gpu: Arc<BladeContext>,
    windows: Rc<RefCell<Vec<std::rc::Weak<AndroidWindowInner>>>>,
    on_quit: Rc<RefCell<Option<Box<dyn FnMut()>>>>,
    appearance: WindowAppearance,
}

impl AndroidPlatform {
    pub(crate) fn new() -> Self {
        let bridge = current_bridge().clone();
        let (dispatcher, _receiver) = AndroidDispatcher::new();
        bridge.attach_dispatcher(dispatcher.sender(), _receiver);
        let dispatcher = Arc::new(dispatcher);
        Self {
            bridge,
            dispatcher,
            text_system: Arc::new(CosmicTextSystem::new()),
            gpu: Arc::new(
                BladeContext::new().expect("failed to initialize Android Vulkan context"),
            ),
            windows: Rc::new(RefCell::new(Vec::new())),
            on_quit: Rc::new(RefCell::new(None)),
            appearance: WindowAppearance::Light,
        }
    }

    fn dispatch_android_event(&self, event: AndroidEvent) -> bool {
        match event {
            AndroidEvent::Quit => return false,
            AndroidEvent::Frame => self.for_each_window(|window| window.on_frame()),
            AndroidEvent::SurfaceCreated => {
                self.for_each_window(|window| window.on_surface_created())
            }
            AndroidEvent::SurfaceChanged {
                width,
                height,
                scale,
            } => {
                self.for_each_window(|window| window.on_surface_changed(width, height, scale));
            }
            AndroidEvent::SurfaceDestroyed => {
                self.for_each_window(|window| window.on_surface_destroyed())
            }
            AndroidEvent::Touch {
                action,
                pointer_id,
                x,
                y,
                pressure,
            } => {
                self.for_each_window(|window| window.on_touch(action, pointer_id, x, y, pressure));
            }
            AndroidEvent::Key {
                key_code,
                action,
                meta_state,
            } => {
                self.for_each_window(|window| window.on_key(key_code, action, meta_state));
            }
            AndroidEvent::Back => self.for_each_window(|window| window.on_back()),
            AndroidEvent::Lifecycle { resumed } => {
                self.for_each_window(|window| window.on_lifecycle(resumed));
            }
            AndroidEvent::Ime {
                text,
                replacement_start,
                replacement_end,
            } => {
                self.for_each_window(|window| {
                    window.on_ime(&text, replacement_start, replacement_end);
                });
            }
        }
        true
    }

    fn for_each_window(&self, mut callback: impl FnMut(&AndroidWindowInner)) {
        let windows = self.windows.borrow();
        for weak in windows.iter() {
            if let Some(window) = weak.upgrade() {
                callback(&window);
            }
        }
    }
}

impl Platform for AndroidPlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        BackgroundExecutor::new(self.dispatcher.clone())
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        ForegroundExecutor::new(self.dispatcher.clone())
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.text_system.clone()
    }

    fn run(&self, on_finish_launching: Box<dyn FnOnce()>) {
        on_finish_launching();
        let receiver = self
            .bridge
            .take_receiver()
            .expect("Android dispatcher receiver missing");
        while let Ok(message) = receiver.recv() {
            match message {
                PlatformMessage::Runnable(runnable) => {
                    let _ = runnable.run();
                }
                PlatformMessage::Android(event) => {
                    if !self.dispatch_android_event(event) {
                        break;
                    }
                }
            }
        }
        if let Some(mut callback) = self.on_quit.borrow_mut().take() {
            callback();
        }
    }

    fn quit(&self) {
        self.bridge.send(AndroidEvent::Quit);
    }

    fn restart(&self, _binary_path: Option<PathBuf>) {
        self.quit();
    }

    fn activate(&self, _ignoring_other_apps: bool) {}
    fn hide(&self) {}
    fn hide_other_apps(&self) {}
    fn unhide_other_apps(&self) {}

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        vec![Rc::new(AndroidDisplay::new(self.bridge.clone()))]
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(AndroidDisplay::new(self.bridge.clone())))
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        None
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        if self
            .windows
            .borrow()
            .iter()
            .any(|window| window.strong_count() > 0)
        {
            return Err(anyhow!(
                "the Android GPUI backend currently supports one window"
            ));
        }
        let window = AndroidWindow::new(
            handle,
            options,
            self.gpu.clone(),
            self.bridge.clone(),
            self.appearance,
        )?;
        let inner = window.downgrade();
        self.windows.borrow_mut().push(inner);
        Ok(Box::new(window))
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.appearance
    }

    fn open_url(&self, _url: &str) {}
    fn on_open_urls(&self, _callback: Box<dyn FnMut(Vec<String>)>) {}
    fn register_url_scheme(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    fn prompt_for_paths(
        &self,
        _options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(Ok(None));
        receiver
    }

    fn prompt_for_new_path(
        &self,
        _directory: &std::path::Path,
        _suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(Ok(None));
        receiver
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        false
    }

    fn reveal_path(&self, _path: &std::path::Path) {}
    fn open_with_system(&self, _path: &std::path::Path) {}

    fn on_quit(&self, callback: Box<dyn FnMut()>) {
        *self.on_quit.borrow_mut() = Some(callback);
    }

    fn on_reopen(&self, _callback: Box<dyn FnMut()>) {}
    fn set_menus(&self, _menus: Vec<Menu>, _keymap: &Keymap) {}
    fn set_dock_menu(&self, _menu: Vec<MenuItem>, _keymap: &Keymap) {}
    fn on_app_menu_action(&self, _callback: Box<dyn FnMut(&dyn Action)>) {}
    fn on_will_open_app_menu(&self, _callback: Box<dyn FnMut()>) {}
    fn on_validate_app_menu_command(&self, _callback: Box<dyn FnMut(&dyn Action) -> bool>) {}

    fn compositor_name(&self) -> &'static str {
        "Android"
    }

    fn app_path(&self) -> Result<PathBuf> {
        std::env::current_exe().context("failed to resolve Android executable path")
    }

    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        Err(anyhow!(
            "auxiliary executable {name} is not available on Android"
        ))
    }

    fn set_cursor_style(&self, _style: CursorStyle) {}
    fn should_auto_hide_scrollbars(&self) -> bool {
        false
    }

    fn write_to_clipboard(&self, _item: ClipboardItem) {}
    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        None
    }

    fn write_credentials(&self, _url: &str, _username: &str, _password: &[u8]) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    fn read_credentials(&self, _url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Ok(None))
    }

    fn delete_credentials(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(AndroidKeyboardLayout)
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(crate::DummyKeyboardMapper)
    }

    fn on_keyboard_layout_change(&self, _callback: Box<dyn FnMut()>) {}
}
