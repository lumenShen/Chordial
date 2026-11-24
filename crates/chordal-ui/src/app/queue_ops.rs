//! 播放队列修改的统一入口。
//!
//! 所有来源（本地库 / 搜索 / 集合 / 歌单 / 推荐流 / 手机预览）都通过
//! [`ChordialApp::snapshot_list`] 解析出曲目，再调用这里的方法改队列。
//! 所有写入都经过 [`ChordialApp::set_queue`]，保证 `queue_gen` 一致；
//! `queue_pos` 的修正也只在这里做一次。

use super::*;

/// 队列手动排序键。随机播放只改变取曲顺序，不改动队列本身。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QueueSortKey {
    Title,
    Artist,
    Duration,
}

impl QueueSortKey {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Title => "按标题排序",
            Self::Artist => "按艺人排序",
            Self::Duration => "按时长排序",
        }
    }
}

/// 队列移动后当前位置的新下标；`to` 是移除后插入的位置。
pub(super) fn remap_pos_after_move(
    pos: Option<usize>,
    from: usize,
    to: usize,
) -> Option<usize> {
    pos.map(|pos| {
        if pos == from {
            to
        } else if from < pos && to >= pos {
            pos - 1
        } else if from > pos && to <= pos {
            pos + 1
        } else {
            pos
        }
    })
}

/// 队列删除一项后当前位置的新下标；`len_after` 是删除后的长度。
pub(super) fn remap_pos_after_remove(
    pos: Option<usize>,
    index: usize,
    len_after: usize,
) -> Option<usize> {
    match pos {
        Some(pos) if pos > index => Some(pos - 1),
        Some(pos) if pos == index => {
            if len_after == 0 {
                None
            } else {
                Some(pos.min(len_after - 1))
            }
        }
        other => other,
    }
}

impl ChordialApp {
    /// 用列表整体替换队列并从 `index` 开始播放。
    pub(super) fn queue_replace(
        &mut self,
        list: &QueueList,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        self.play_from_list(list, index, cx);
    }

    /// 播放整张列表（从第一首开始）。
    pub(super) fn queue_play_all(&mut self, list: &QueueList, cx: &mut Context<Self>) {
        self.queue_replace(list, 0, cx);
    }

    /// 把整张列表去重追加到队尾；返回实际追加数量。
    pub(super) fn queue_append_list(&mut self, list: &QueueList, cx: &mut Context<Self>) -> usize {
        let incoming = self.snapshot_list(list).as_ref().clone();
        let mut queue = self.queue.as_ref().clone();
        let added = Self::append_unique_tracks(&mut queue, incoming);
        if added == 0 {
            self.info_message = Some("这些曲目都已在播放队列里".into());
            cx.notify();
            return 0;
        }
        self.set_queue(Arc::new(queue), self.queue_refill.clone());
        self.show_portrait_toast(format!("已添加 {added} 首到播放队列"), cx);
        cx.notify();
        added
    }

    /// 把整张列表插到当前曲目之后；返回实际插入数量。
    pub(super) fn queue_play_next_list(
        &mut self,
        list: &QueueList,
        cx: &mut Context<Self>,
    ) -> usize {
        let incoming = self.snapshot_list(list).as_ref().clone();
        if incoming.is_empty() {
            return 0;
        }
        let mut queue = self.queue.as_ref().clone();
        let mut base: HashSet<String> = queue.iter().map(|t| t.identity_key()).collect();
        let fresh: Vec<Track> = incoming
            .into_iter()
            .filter(|track| base.insert(track.identity_key()))
            .collect();
        let added = fresh.len();
        if added == 0 {
            return 0;
        }
        let at = self.queue_pos.map(|pos| pos + 1).unwrap_or(0);
        queue.splice(at..at, fresh);
        self.set_queue(Arc::new(queue), self.queue_refill.clone());
        self.show_portrait_toast(format!("已将 {added} 首设为下一首播放"), cx);
        cx.notify();
        added
    }

    /// 从队列移除指定下标；不打断当前播放，只修正 `queue_pos`。
    pub(super) fn queue_remove_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.queue.len() {
            return;
        }
        let mut queue = self.queue.as_ref().clone();
        queue.remove(index);
        let pos = remap_pos_after_remove(self.queue_pos, index, queue.len());
        self.set_queue(Arc::new(queue), self.queue_refill.clone());
        self.queue_pos = pos;
        cx.notify();
    }

    /// 队列内移动一项并保持当前播放曲目的位置跟随。
    pub(super) fn queue_move(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to || from >= self.queue.len() {
            return;
        }
        let mut queue = self.queue.as_ref().clone();
        let track = queue.remove(from);
        let to = to.min(queue.len());
        queue.insert(to, track);
        let pos = remap_pos_after_move(self.queue_pos, from, to);
        self.set_queue(Arc::new(queue), self.queue_refill.clone());
        self.queue_pos = pos;
        self.show_portrait_toast("已调整播放顺序", cx);
        cx.notify();
    }

    /// 按标题 / 艺人 / 时长排序队列；当前曲目位置随排序结果更新。
    pub(super) fn queue_sort(&mut self, key: QueueSortKey, cx: &mut Context<Self>) {
        if self.queue.len() < 2 {
            return;
        }
        let current_id = self
            .queue_pos
            .and_then(|pos| self.queue.get(pos))
            .map(|track| track.id.clone());
        let mut queue = self.queue.as_ref().clone();
        queue.sort_by(|a, b| match key {
            QueueSortKey::Title => a.title.cmp(&b.title),
            QueueSortKey::Artist => a.primary_artist().cmp(b.primary_artist()),
            QueueSortKey::Duration => a
                .duration_ms
                .unwrap_or(u64::MAX)
                .cmp(&b.duration_ms.unwrap_or(u64::MAX)),
        });
        let pos = current_id.and_then(|id| queue.iter().position(|track| track.id == id));
        self.set_queue(Arc::new(queue), self.queue_refill.clone());
        self.queue_pos = pos;
        self.show_portrait_toast(key.label(), cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_current_track_follows_destination() {
        assert_eq!(remap_pos_after_move(Some(0), 0, 3), Some(3));
        assert_eq!(remap_pos_after_move(Some(2), 0, 3), Some(1));
        assert_eq!(remap_pos_after_move(Some(0), 2, 0), Some(1));
        assert_eq!(remap_pos_after_move(None, 0, 3), None);
    }

    #[test]
    fn removing_current_track_keeps_playback_continuous() {
        assert_eq!(remap_pos_after_remove(Some(3), 1, 4), Some(2));
        assert_eq!(remap_pos_after_remove(Some(1), 1, 3), Some(1));
        assert_eq!(remap_pos_after_remove(Some(2), 2, 0), None);
        assert_eq!(remap_pos_after_remove(None, 0, 2), None);
    }

    #[test]
    fn append_unique_dedupes_by_identity() {
        let mut base: Vec<chordal_core::model::Track> = vec!["A".to_string(), "B".to_string()]
            .into_iter()
            .map(|s| chordal_core::model::Track::new(
                chordal_core::model::TrackId::new(chordal_core::model::SourceId::new("local"), s.clone()),
                s.clone(),
                Vec::new(),
                None,
                None,
                chordal_core::model::TrackOrigin::Local { path: std::path::PathBuf::from(s) },
            ))
            .collect();
        let added = ChordialApp::append_unique_tracks(
            &mut base,
            vec!["B".to_string(), "C".to_string()]
                .into_iter()
                .map(|s| chordal_core::model::Track::new(
                    chordal_core::model::TrackId::new(chordal_core::model::SourceId::new("netease"), s.clone()),
                    s,
                    Vec::new(),
                    None,
                    None,
                    chordal_core::model::TrackOrigin::Remote { source: chordal_core::model::SourceId::new("netease") },
                ))
                .collect(),
        );
        assert_eq!(added, 1);
        assert_eq!(base.len(), 3);
    }
}
