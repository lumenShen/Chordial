//! `SourceCleanup` trait 实现 — 注销来源时联动清理库中孤儿数据。

use super::MusicLibrary;
use crate::module::music_source::registrar::SourceCleanup;

impl SourceCleanup for MusicLibrary {
    fn remove_source_from_all_entities(&self, source_name: &str) -> Result<(), String> {
        MusicLibrary::remove_source_from_all_entities(self, source_name)
    }
}