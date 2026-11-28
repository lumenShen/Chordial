#![cfg(target_os = "android")]

//! JNI exports called by `com.chordial.app.NativeBridge`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::android::bridge;
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jfloat, jint};
use jni::{EnvUnowned, Outcome};

static STARTED: AtomicBool = AtomicBool::new(false);

/// 把 Android 应用私有目录注入 Rust 侧。
///
/// Android 没有 XDG 目录，`dirs` 全返回 `None`，默认路径会落到工作目录 `/`。
/// 在 `start` 之前调用：创建所需子目录，并写入 `CHORDIAL_CONFIG_DIR` 等
/// 环境变量，供 config / cache / crash_log / 本地库解析使用。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_setStorageDirs<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    files_dir: JString<'local>,
    cache_dir: JString<'local>,
) {
    let outcome = environment.with_env(|_environment| -> Result<(), jni::errors::Error> {
        let files = PathBuf::from(files_dir.to_string());
        let cache = PathBuf::from(cache_dir.to_string());
        let music = files.join("music");
        let downloads = files.join("downloads");
        let logs = files.join("logs");
        let objects = cache.join("chordial").join("objects");
        for dir in [&music, &downloads, &logs, &objects] {
            let _ = std::fs::create_dir_all(dir);
        }
        // SAFETY: 由 Java 主线程在应用线程启动前调用，此时没有并发读取环境变量。
        unsafe {
            std::env::set_var("CHORDIAL_CONFIG_DIR", &files);
            std::env::set_var("CHORDIAL_LOG_DIR", &logs);
            std::env::set_var("CHORDIAL_CACHE_DIR", &objects);
            std::env::set_var("CHORDIAL_MUSIC_DIR", &music);
            std::env::set_var("CHORDIAL_DOWNLOAD_DIR", &downloads);
        }
        Ok(())
    });
    if let Outcome::Err(error) = outcome.into_outcome() {
        log::error!("failed to set Android storage dirs: {error}");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_start<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    activity: JObject<'local>,
) {
    let initialized = environment.with_env(|environment| bridge::init(environment, activity));
    if let Outcome::Err(error) = initialized.into_outcome() {
        log::error!("failed to initialize Android JNI bridge: {error}");
        return;
    }
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("chordial-android".to_owned())
        .spawn(chordal_ui::run_android)
        .expect("failed to start Chordial Android thread");
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_surfaceCreated<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    surface: JObject<'local>,
) {
    let outcome = environment.with_env(|environment| -> Result<(), jni::errors::Error> {
        if let Err(error) = bridge::surface_created(environment, &surface) {
            log::error!("failed to attach Android surface: {error}");
        }
        Ok(())
    });
    if let Outcome::Err(error) = outcome.into_outcome() {
        log::error!("failed to deliver Android surface event: {error}");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_surfaceChanged<'local>(
    _environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    _surface: JObject<'local>,
    width: jint,
    height: jint,
    scale: jfloat,
) {
    bridge::surface_changed(width, height, scale);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_surfaceDestroyed<'local>(
    _environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    _surface: JObject<'local>,
) {
    bridge::surface_destroyed();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_frame<'local>(
    _environment: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    bridge::frame();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_touch<'local>(
    _environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    action: jint,
    pointer_id: jint,
    x: jfloat,
    y: jfloat,
    pressure: jfloat,
) {
    bridge::touch(action, pointer_id, x, y, pressure);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_key<'local>(
    _environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    key_code: jint,
    action: jint,
    meta_state: jint,
) {
    bridge::key(key_code, action, meta_state);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_back<'local>(
    _environment: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    bridge::back();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_resume<'local>(
    _environment: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    bridge::lifecycle(true);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_pause<'local>(
    _environment: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    bridge::lifecycle(false);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_imeInput<'local>(
    mut environment: EnvUnowned<'local>,
    _class: JClass<'local>,
    text: JString<'local>,
    replacement_start: jint,
    replacement_end: jint,
) {
    let outcome = environment.with_env(|_environment| -> Result<(), jni::errors::Error> {
        let text = text.to_string();
        bridge::ime(text, replacement_start, replacement_end);
        Ok(())
    });
    if let Outcome::Err(error) = outcome.into_outcome() {
        log::error!("failed to deliver Android IME input: {error}");
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_chordial_app_NativeBridge_requestPermissions<'local>(
    _environment: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
}
