//! 全局动作与键位。

use gpui::{App, KeyBinding, actions};

actions!(
    chordial,
    [
        PlayPause,
        NextTrack,
        PrevTrack,
        FocusSearch,
        RescanLibrary,
        ToggleSidebar,
        TogglePortrait,
        ToggleEqualizer,
        PlayFirst,
        ExitImmersive,
        OpenSettings,
        CloseSettings,
        Quit
    ]
);

pub fn install_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-space", PlayPause, Some("ChordialMain")),
        KeyBinding::new("ctrl-right", NextTrack, Some("ChordialMain")),
        KeyBinding::new("ctrl-left", PrevTrack, Some("ChordialMain")),
        KeyBinding::new("ctrl-k", FocusSearch, Some("ChordialMain")),
        KeyBinding::new("ctrl-r", RescanLibrary, Some("ChordialMain")),
        KeyBinding::new("ctrl-b", ToggleSidebar, Some("ChordialMain")),
        KeyBinding::new("f2", TogglePortrait, Some("ChordialMain")),
        KeyBinding::new("f5", PlayFirst, Some("ChordialMain")),
        KeyBinding::new("f5", PlayFirst, Some("ChordialImmersive")),
        KeyBinding::new("f2", TogglePortrait, Some("ChordialImmersive")),
        KeyBinding::new("f3", ToggleEqualizer, Some("ChordialMain")),
        KeyBinding::new("f9", OpenSettings, None),
        KeyBinding::new("f3", ToggleEqualizer, Some("ChordialImmersive")),
        KeyBinding::new("escape", ExitImmersive, Some("ChordialImmersive")),
        KeyBinding::new("escape", CloseSettings, Some("ChordialMain")),
        KeyBinding::new("space", PlayPause, Some("ChordialImmersive")),
        KeyBinding::new("ctrl-space", PlayPause, Some("ChordialImmersive")),
        KeyBinding::new("ctrl-right", NextTrack, Some("ChordialImmersive")),
        KeyBinding::new("ctrl-left", PrevTrack, Some("ChordialImmersive")),
        KeyBinding::new("ctrl-q", Quit, Some("ChordialMain")),
        // 搜索框内的独立键位（见 input.rs 的动作集）。
        KeyBinding::new("escape", Blur, Some("ChordInput")),
    ]);
}

actions!(chord_input, [Blur]);
