use std::cell::RefCell;
use std::rc::Rc;

use gpui::{Bounds, Pixels};

use crate::layout_morph::{
    DOCK_ARTIST_SIZE, DOCK_COVER_RADIUS, DOCK_LINE_GAP, DOCK_TITLE_SIZE, QUEUE_COVER_RADIUS,
    StageGeometry, StageMode, StageRect,
};

#[derive(Clone, Copy, Debug, PartialEq)]
struct MeasureKey {
    mode: StageMode,
    width: f32,
    height: f32,
    cover_scale: f32,
    idle_shift: f32,
}

#[derive(Clone, Copy, Debug)]
struct MeasureStyle {
    radius: f32,
    title_size: f32,
    artist_size: f32,
    line_gap: f32,
    title_weight: f32,
    artist_weight: f32,
}

impl MeasureStyle {
    fn for_mode(mode: StageMode, height: f32) -> Self {
        match mode {
            StageMode::Dock => Self {
                radius: DOCK_COVER_RADIUS,
                title_size: DOCK_TITLE_SIZE,
                artist_size: DOCK_ARTIST_SIZE,
                line_gap: DOCK_LINE_GAP,
                title_weight: 600.0,
                artist_weight: 400.0,
            },
            StageMode::MiniPlayer => Self {
                radius: 1.0,
                title_size: 12.5,
                artist_size: 10.0,
                line_gap: 2.0,
                title_weight: 500.0,
                artist_weight: 400.0,
            },
            StageMode::PhonePlayer | StageMode::CompactArt | StageMode::WideSplit => {
                let size = (height * 0.02).max(16.0);
                Self {
                    radius: 1.0,
                    title_size: size,
                    artist_size: size,
                    line_gap: 0.0,
                    title_weight: 500.0,
                    artist_weight: 400.0,
                }
            }
            StageMode::CompactQueue => Self {
                radius: QUEUE_COVER_RADIUS,
                title_size: 16.0,
                artist_size: 11.0,
                line_gap: 2.0,
                title_weight: 500.0,
                artist_weight: 400.0,
            },
            StageMode::CompactLyrics => Self {
                radius: 1.0,
                title_size: (height * 0.02).clamp(14.0, 20.0),
                artist_size: (height * 0.02).clamp(14.0, 20.0),
                line_gap: 0.0,
                title_weight: 500.0,
                artist_weight: 400.0,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum MeasurePart {
    Cover,
    Info,
    Card,
}

#[derive(Default)]
struct MeasureFrame {
    key: Option<MeasureKey>,
    mode: Option<StageMode>,
    style: Option<MeasureStyle>,
    offset: (f32, f32),
    cover: Option<StageRect>,
    info: Option<StageRect>,
    card: Option<StageRect>,
}

impl MeasureFrame {
    fn record(&mut self, part: MeasurePart, bounds: Bounds<Pixels>) {
        let rect = StageRect {
            x: f32::from(bounds.origin.x) + self.offset.0,
            y: f32::from(bounds.origin.y) + self.offset.1,
            w: f32::from(bounds.size.width),
            h: f32::from(bounds.size.height),
        };
        if !rect.is_zero() && rect.x.is_finite() && rect.y.is_finite() {
            match part {
                MeasurePart::Cover => self.cover = Some(rect),
                MeasurePart::Info if rect.w > 0.5 => self.info = Some(rect),
                MeasurePart::Info => {}
                MeasurePart::Card => self.card = Some(rect),
            }
        }
    }

    fn finish(self) -> Option<StageGeometry> {
        let style = self.style?;
        Some(StageGeometry {
            cover: self.cover?,
            cover_radius: style.radius,
            info: self.info?,
            title_size: style.title_size,
            artist_size: style.artist_size,
            line_gap: style.line_gap,
            title_weight: style.title_weight,
            artist_weight: style.artist_weight,
        })
    }
}

impl MeasureKey {
    fn for_state(
        mode: StageMode,
        width: f32,
        height: f32,
        cover_scale: f32,
        idle_shift: f32,
    ) -> Self {
        Self {
            mode,
            width,
            height,
            cover_scale: if mode.is_source() {
                1.0
            } else {
                (cover_scale * 1000.0).round() / 1000.0
            },
            idle_shift: if mode.is_source() {
                0.0
            } else {
                (idle_shift * 1000.0).round() / 1000.0
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MeasuredStage {
    key: MeasureKey,
    geometry: StageGeometry,
    card: Option<StageRect>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct StageRequest {
    pub from_mode: StageMode,
    pub from: StageGeometry,
    pub velocity: StageGeometry,
    pub to_mode: StageMode,
    pub cover_scale: f32,
}

#[derive(Default)]
pub(super) struct StageMeasurements {
    entries: Vec<MeasuredStage>,
    output: Option<Rc<RefCell<MeasureFrame>>>,
}

impl StageMeasurements {
    pub fn cached(&self, mode: StageMode) -> Option<StageGeometry> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.key.mode == mode)
            .map(|entry| entry.geometry)
    }

    pub fn cached_current(
        &self,
        mode: StageMode,
        width: f32,
        height: f32,
        cover_scale: f32,
        idle_shift: f32,
    ) -> Option<StageGeometry> {
        let key = MeasureKey::for_state(mode, width, height, cover_scale, idle_shift);
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.key == key)
            .map(|entry| entry.geometry)
    }

    pub fn cached_card_current(
        &self,
        mode: StageMode,
        width: f32,
        height: f32,
    ) -> Option<StageRect> {
        let key = MeasureKey::for_state(mode, width, height, 1.0, 0.0);
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.key == key)
            .and_then(|entry| entry.card)
    }

    pub fn begin(
        &mut self,
        mode: StageMode,
        width: f32,
        height: f32,
        cover_scale: f32,
        idle_shift: f32,
        origin: (f32, f32),
        force: bool,
    ) -> bool {
        let key = MeasureKey::for_state(mode, width, height, cover_scale, idle_shift);
        if !force && self.entries.iter().any(|entry| entry.key == key) {
            return false;
        }
        let output = Rc::new(RefCell::new(MeasureFrame {
            key: Some(key),
            mode: Some(mode),
            style: Some(MeasureStyle::for_mode(mode, height)),
            offset: (-origin.0, -origin.1),
            ..Default::default()
        }));
        self.output = Some(output);
        true
    }

    pub fn observe(
        &self,
        mode: StageMode,
        element: gpui::Div,
        children: &[(usize, MeasurePart)],
    ) -> gpui::Div {
        let Some(output) = self
            .output
            .as_ref()
            .filter(|frame| frame.borrow().mode == Some(mode))
            .cloned()
        else {
            return element;
        };
        let parts = children.to_vec();
        element.on_children_prepainted(move |bounds, _, _| {
            let mut frame = output.borrow_mut();
            for (index, part) in &parts {
                if let Some(bounds) = bounds.get(*index) {
                    frame.record(*part, *bounds);
                }
            }
        })
    }

    pub fn finish_frame(&mut self) -> bool {
        let Some(output) = self.output.take() else {
            return false;
        };
        let Ok(frame) = Rc::try_unwrap(output) else {
            return false;
        };
        let frame = frame.into_inner();
        let Some(key) = frame.key else {
            return false;
        };
        let card = frame.card;
        let Some(geometry) = frame.finish() else {
            return false;
        };
        self.entries.retain(|entry| entry.key.mode != key.mode);
        self.entries.push(MeasuredStage {
            key,
            geometry,
            card,
        });
        self.entries.truncate(8);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_uses_real_rects_and_invalidates_on_layout_state() {
        let mut measurements = StageMeasurements::default();
        assert!(measurements.begin(
            StageMode::CompactArt,
            800.0,
            1000.0,
            0.75,
            0.0,
            (0.0, 0.0),
            false
        ));
        {
            let output = measurements.output.as_ref().unwrap();
            let mut frame = output.borrow_mut();
            frame.record(
                MeasurePart::Cover,
                gpui::Bounds::new(
                    gpui::point(gpui::px(100.0), gpui::px(56.0)),
                    gpui::size(gpui::px(300.0), gpui::px(300.0)),
                ),
            );
            frame.record(
                MeasurePart::Info,
                gpui::Bounds::new(
                    gpui::point(gpui::px(100.0), gpui::px(384.0)),
                    gpui::size(gpui::px(248.0), gpui::px(40.0)),
                ),
            );
        }
        assert!(measurements.finish_frame());
        let geometry = measurements
            .cached_current(StageMode::CompactArt, 800.0, 1000.0, 0.75, 0.0)
            .unwrap();
        assert_eq!(geometry.cover.w, 300.0);
        assert_eq!(geometry.info.x, 100.0);
        assert!(
            measurements
                .cached_current(StageMode::CompactArt, 801.0, 1000.0, 0.75, 0.0)
                .is_none()
        );
        assert!(!measurements.begin(
            StageMode::CompactArt,
            800.0,
            1000.0,
            0.75,
            0.0,
            (0.0, 0.0),
            false
        ));
        assert!(measurements.begin(
            StageMode::CompactArt,
            800.0,
            1000.0,
            1.0,
            0.0,
            (0.0, 0.0),
            false
        ));
    }

    #[test]
    fn measured_frame_uses_layout_rects_and_style_channels() {
        let mut frame = MeasureFrame {
            style: Some(MeasureStyle::for_mode(StageMode::CompactQueue, 800.0)),
            offset: (-20.0, -10.0),
            ..Default::default()
        };
        frame.record(
            MeasurePart::Cover,
            Bounds::new(
                gpui::point(gpui::px(74.0), gpui::px(72.0)),
                gpui::size(gpui::px(54.0), gpui::px(54.0)),
            ),
        );
        frame.record(
            MeasurePart::Info,
            Bounds::new(
                gpui::point(gpui::px(140.0), gpui::px(80.0)),
                gpui::size(gpui::px(200.0), gpui::px(35.75)),
            ),
        );
        let geometry = frame.finish().unwrap();
        assert_eq!(geometry.cover.x, 54.0);
        assert_eq!(geometry.cover.y, 62.0);
        assert_eq!(geometry.info.x, 120.0);
        assert_eq!(geometry.title_size, 16.0);
        assert_eq!(geometry.artist_size, 11.0);
    }
}
