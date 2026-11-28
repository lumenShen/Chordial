use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{
    Arc, Condvar, Mutex, OnceLock,
    mpsc::{Receiver, Sender},
};

use jni::objects::{Global, JObject};
use jni::{Env, JavaVM, sys};

use super::dispatcher::PlatformMessage;

static ANDROID_BRIDGE: OnceLock<Arc<AndroidBridge>> = OnceLock::new();

pub(crate) enum AndroidEvent {
    SurfaceCreated,
    SurfaceChanged {
        width: f32,
        height: f32,
        scale: f32,
    },
    SurfaceDestroyed,
    Frame,
    Touch {
        action: i32,
        pointer_id: i32,
        x: f32,
        y: f32,
        pressure: f32,
    },
    Key {
        key_code: i32,
        action: i32,
        meta_state: i32,
    },
    Back,
    Lifecycle {
        resumed: bool,
    },
    Ime {
        text: String,
        replacement_start: i32,
        replacement_end: i32,
    },
    Quit,
}

pub(crate) struct AndroidBridge {
    _java_vm: JavaVM,
    _activity: Global<JObject<'static>>,
    sender: Mutex<Option<Sender<PlatformMessage>>>,
    receiver: Mutex<Option<Receiver<PlatformMessage>>>,
    surface: Mutex<Option<usize>>,
    surface_ready: Condvar,
    surface_size: Mutex<(f32, f32, f32)>,
}

impl AndroidBridge {
    fn new(java_vm: JavaVM, activity: Global<JObject<'static>>) -> Self {
        Self {
            _java_vm: java_vm,
            _activity: activity,
            sender: Mutex::new(None),
            receiver: Mutex::new(None),
            surface: Mutex::new(None),
            surface_ready: Condvar::new(),
            surface_size: Mutex::new((1080.0, 2400.0, 3.0)),
        }
    }

    pub(crate) fn attach_dispatcher(
        &self,
        sender: Sender<PlatformMessage>,
        receiver: Receiver<PlatformMessage>,
    ) {
        *self.sender.lock().expect("Android sender lock") = Some(sender);
        *self.receiver.lock().expect("Android receiver lock") = Some(receiver);
    }

    pub(crate) fn take_receiver(&self) -> Option<Receiver<PlatformMessage>> {
        self.receiver.lock().expect("Android receiver lock").take()
    }

    pub(crate) fn send(&self, event: AndroidEvent) {
        if let Some(sender) = self.sender.lock().expect("Android sender lock").as_ref() {
            let _ = sender.send(PlatformMessage::Android(event));
        }
    }

    pub(crate) fn wait_for_surface(&self) -> Option<NonNull<c_void>> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut surface = self.surface.lock().expect("Android surface lock");
        while surface.is_none() {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (guard, timeout) = self
                .surface_ready
                .wait_timeout(surface, remaining)
                .expect("Android surface condvar");
            surface = guard;
            if timeout.timed_out() && surface.is_none() {
                return None;
            }
        }
        surface
            .map(|pointer| NonNull::new(pointer as *mut c_void).expect("non-null Android surface"))
    }

    pub(crate) fn surface(&self) -> Option<NonNull<c_void>> {
        self.surface
            .lock()
            .expect("Android surface lock")
            .map(|pointer| NonNull::new(pointer as *mut c_void).expect("non-null Android surface"))
    }

    pub(crate) fn take_surface(&self) -> Option<NonNull<c_void>> {
        self.surface
            .lock()
            .expect("Android surface lock")
            .take()
            .map(|pointer| NonNull::new(pointer as *mut c_void).expect("non-null Android surface"))
    }

    /// 请求 Android 壳显示 / 隐藏软键盘。
    ///
    /// gpui 只有 `update_ime_position` 这一条焦点信号，所以“显示”用它触发；
    /// “隐藏”由窗口在下一帧发现 input handler 为空时调用。
    pub(crate) fn set_ime_visible(&self, visible: bool) {
        let result =
            self._java_vm
                .attach_current_thread(|environment| -> Result<(), jni::errors::Error> {
                    environment.call_method(
                        self._activity.as_ref(),
                        jni::jni_str!("setImeVisible"),
                        jni::jni_sig!("(Z)V"),
                        &[jni::objects::JValue::Bool(visible)],
                    )?;
                    Ok(())
                });
        if let Err(error) = result {
            log::warn!("Android setImeVisible({visible}) failed: {error}");
        }
    }

    pub(crate) fn surface_size(&self) -> (f32, f32, f32) {
        *self.surface_size.lock().expect("Android surface size lock")
    }
}

pub(crate) fn current_bridge() -> &'static Arc<AndroidBridge> {
    ANDROID_BRIDGE
        .get()
        .expect("Android bridge must be initialized before creating the GPUI application")
}

/// Stores the Java VM and Activity before the Rust application thread starts.
pub fn init(env: &mut Env<'_>, activity: JObject<'_>) -> Result<(), jni::errors::Error> {
    let java_vm = env.get_java_vm()?;
    let activity = env.new_global_ref(activity)?;
    let bridge = Arc::new(AndroidBridge::new(java_vm, activity));
    let _ = ANDROID_BRIDGE.set(bridge);
    Ok(())
}

/// Converts a Java Surface to an acquired ANativeWindow and notifies GPUI.
pub fn surface_created(env: &mut Env<'_>, surface: &JObject<'_>) -> Result<(), String> {
    let bridge = current_bridge();
    let window = unsafe { ANativeWindow_fromSurface(env.get_raw(), surface.as_raw()) };
    let window =
        NonNull::new(window).ok_or_else(|| "ANativeWindow_fromSurface returned null".to_owned())?;
    unsafe { ANativeWindow_acquire(window.as_ptr()) };
    *bridge.surface.lock().expect("Android surface lock") = Some(window.as_ptr() as usize);
    bridge.surface_ready.notify_all();
    bridge.send(AndroidEvent::SurfaceCreated);
    Ok(())
}

/// Updates the physical surface size and display scale factor.
pub fn surface_changed(width: i32, height: i32, scale: f32) {
    let bridge = current_bridge();
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    *bridge
        .surface_size
        .lock()
        .expect("Android surface size lock") = (width.max(1) as f32, height.max(1) as f32, scale);
    bridge.send(AndroidEvent::SurfaceChanged {
        width: width.max(1) as f32,
        height: height.max(1) as f32,
        scale,
    });
}

/// Notifies GPUI that the Surface and its renderer must be released.
pub fn surface_destroyed() {
    let bridge = current_bridge();
    bridge.send(AndroidEvent::SurfaceDestroyed);
    let _ = bridge.take_surface();
}

/// Delivers a Choreographer frame callback to the Rust event loop.
pub fn frame() {
    current_bridge().send(AndroidEvent::Frame);
}

/// Delivers a primary-pointer touch event to GPUI.
pub fn touch(action: i32, pointer_id: i32, x: f32, y: f32, pressure: f32) {
    current_bridge().send(AndroidEvent::Touch {
        action,
        pointer_id,
        x,
        y,
        pressure,
    });
}

/// Delivers a hardware key event to GPUI.
pub fn key(key_code: i32, action: i32, meta_state: i32) {
    current_bridge().send(AndroidEvent::Key {
        key_code,
        action,
        meta_state,
    });
}

/// Maps Android Back to GPUI's Escape dismissal behavior.
pub fn back() {
    current_bridge().send(AndroidEvent::Back);
}

/// Updates window activation when the Activity resumes or pauses.
pub fn lifecycle(resumed: bool) {
    current_bridge().send(AndroidEvent::Lifecycle { resumed });
}

/// Replaces text in the focused GPUI input handler.
pub fn ime(text: String, replacement_start: i32, replacement_end: i32) {
    current_bridge().send(AndroidEvent::Ime {
        text,
        replacement_start,
        replacement_end,
    });
}

/// Stops the GPUI platform event loop.
pub fn quit() {
    current_bridge().send(AndroidEvent::Quit);
}

pub(crate) unsafe fn acquire_native_window(window: NonNull<c_void>) {
    unsafe { ANativeWindow_acquire(window.as_ptr()) };
}

pub(crate) unsafe fn release_native_window(window: NonNull<c_void>) {
    unsafe { ANativeWindow_release(window.as_ptr()) };
}

unsafe extern "C" {
    fn ANativeWindow_fromSurface(env: *mut sys::JNIEnv, surface: sys::jobject) -> *mut c_void;
    fn ANativeWindow_acquire(window: *mut c_void);
    fn ANativeWindow_release(window: *mut c_void);
}
