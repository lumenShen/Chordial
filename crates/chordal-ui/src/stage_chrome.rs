//! 舞台次要控件的统一视觉滤镜。
//!
//! 顶部「更多」与底部可收起行（传输/音量/底栏）各自成为一个滤镜组，
//! 共用同一份由闲置弹簧采样得到的 [`StageChromeVisual`]。进度条不进入
//! 滤镜组，闲置时保持可见。完全隐藏时跳过子树的 prepaint / paint，
//! 因此隐藏控件不会留下命中框或事件监听。

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Point, Window, point, px,
};

/// 舞台控件的透明度、缩放与模糊曲线。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageChromeVisual {
    pub opacity: f32,
    pub scale: f32,
    pub blur_radius: Pixels,
}

impl StageChromeVisual {
    /// 从已有的显隐弹簧进度派生视觉参数；不创建新的动画状态。
    pub fn from_progress(progress: f32) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        let opacity = crate::motion::ramp(progress, 0.0, 1.0);
        Self {
            opacity,
            scale: 1.0 - 0.04 * (1.0 - progress).powf(1.25),
            blur_radius: px(2.0 * (1.0 - opacity)),
        }
    }

    fn paint_filter(self, bounds: Bounds<Pixels>, anchor: StageChromeAnchor) -> gpui::PaintFilter {
        gpui::PaintFilter {
            opacity: self.opacity,
            scale: self.scale,
            origin: anchor.origin(bounds),
            blur_radius: self.blur_radius,
        }
    }
}

/// 滤镜缩放与模糊的锚点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageChromeAnchor {
    TopRight,
    BottomCenter,
}

impl StageChromeAnchor {
    fn origin(self, bounds: Bounds<Pixels>) -> Point<Pixels> {
        match self {
            Self::TopRight => bounds.top_right(),
            Self::BottomCenter => point(bounds.center().x, bounds.bottom()),
        }
    }
}

/// 把一个子树包装为独立的舞台控件滤镜组。
pub struct StageChromeGroup {
    child: AnyElement,
    visual: StageChromeVisual,
    anchor: StageChromeAnchor,
}

impl StageChromeGroup {
    pub fn new(
        child: impl IntoElement,
        visual: StageChromeVisual,
        anchor: StageChromeAnchor,
    ) -> Self {
        Self {
            child: child.into_any_element(),
            visual,
            anchor,
        }
    }

    fn is_visible(&self) -> bool {
        self.visual.opacity > 0.001
    }
}

impl IntoElement for StageChromeGroup {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for StageChromeGroup {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        Element::request_layout(&mut self.child, id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if !self.is_visible() {
            return;
        }
        let filter = self.visual.paint_filter(bounds, self.anchor);
        window.prepaint_with_filter(filter, |window| {
            Element::prepaint(
                &mut self.child,
                id,
                inspector_id,
                bounds,
                request_layout,
                window,
                cx,
            );
        });
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !self.is_visible() {
            return;
        }
        let filter = self.visual.paint_filter(bounds, self.anchor);
        window.paint_with_filter(filter, |window| {
            Element::paint(
                &mut self.child,
                id,
                inspector_id,
                bounds,
                request_layout,
                prepaint,
                window,
                cx,
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(value: gpui::Pixels) -> f32 {
        value.into()
    }

    #[test]
    fn endpoints_are_fully_hidden_and_fully_visible() {
        let hidden = StageChromeVisual::from_progress(0.0);
        assert_eq!(hidden.opacity, 0.0);
        assert!((hidden.scale - 0.96).abs() < 1e-6);
        assert_eq!(hidden.blur_radius, px(2.0));

        let visible = StageChromeVisual::from_progress(1.0);
        assert_eq!(visible.opacity, 1.0);
        assert_eq!(visible.scale, 1.0);
        assert_eq!(visible.blur_radius, px(0.0));
    }

    #[test]
    fn scale_never_drops_below_minimum() {
        for step in 0..=100 {
            let visual = StageChromeVisual::from_progress(step as f32 / 100.0);
            assert!(visual.scale >= 0.96 - 1e-6);
            assert!(visual.scale <= 1.0 + 1e-6);
        }
    }

    #[test]
    fn blur_never_exceeds_two_pixels() {
        for step in 0..=100 {
            let visual = StageChromeVisual::from_progress(step as f32 / 100.0);
            assert!(pixels(visual.blur_radius) <= 2.0 + 1e-6);
        }
    }

    #[test]
    fn curves_are_identical_regardless_of_anchor() {
        let bounds = Bounds::from_corners(point(px(10.0), px(20.0)), point(px(110.0), px(80.0)));
        for progress in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let visual = StageChromeVisual::from_progress(progress);
            let top = visual.paint_filter(bounds, StageChromeAnchor::TopRight);
            let bottom = visual.paint_filter(bounds, StageChromeAnchor::BottomCenter);
            assert_eq!(top.opacity, bottom.opacity);
            assert_eq!(top.scale, bottom.scale);
            assert_eq!(top.blur_radius, bottom.blur_radius);
            assert_eq!(top.origin, bounds.top_right());
            assert_eq!(bottom.origin, point(bounds.center().x, bounds.bottom()));
        }
    }
}
