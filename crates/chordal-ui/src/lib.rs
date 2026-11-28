#![allow(clippy::type_complexity)]

//! Chordial 2.0 shared application library.

mod actions;
mod app;
mod autoeq;
mod bubble;
mod config;
mod crash_log;
mod downloads;
mod eq;
mod eq_page;
mod frame_trace;
mod icons;
mod input;
mod layout_morph;
mod lyrics;
mod main_state;
mod motion;
mod perf_overlay;
mod phone_gesture;
mod player_page;
mod playlists;
mod responsive;
mod settings;
mod sheet_drag;
mod sources;
mod stage_chrome;
mod storage;
mod theme;
mod widgets;

pub mod desktop;

#[cfg(target_os = "android")]
mod android;

#[cfg(target_os = "android")]
pub use android::run as run_android;

/// Tracks live and peak heap usage for the in-app memory diagnostics.
#[global_allocator]
static GLOBAL_ALLOCATOR: chordal_core::mem::TrackingAllocator =
    chordal_core::mem::TrackingAllocator;
