mod dispatcher;
mod display;
mod keyboard;
mod platform;
mod text_system;
mod window;

/// JNI-facing event bridge used by the Android application shell.
pub mod bridge;

pub(crate) use platform::AndroidPlatform;
pub(crate) type PlatformScreenCaptureFrame = ();
