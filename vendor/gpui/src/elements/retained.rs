use std::ops::Range;

use crate::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, PaintIndex, Pixels, PrepaintStateIndex, Style, Window,
};

/// A subtree whose construction, layout and paint work can be reused across
/// frames while an explicit version stays unchanged.
///
/// Unlike [`crate::AnyView`] caching this does not require a separate view
/// entity, and it is not invalidated by descendant notifications (there are
/// none in the cached subtree). The caller must include every input that can
/// change the subtree's content, geometry, clip or hover state in `version`.
///
/// On a cache hit the element requests a leaf layout node with the supplied
/// `style` (which must match the built subtree's root size) and replays the
/// previous frame's prepaint/paint ranges. On a miss the subtree is rebuilt
/// and laid out at the current bounds.
pub struct Retained {
    id: ElementId,
    version: u64,
    style: Style,
    build: Option<Box<dyn FnOnce(&mut Window, &mut App) -> AnyElement + 'static>>,
}

impl Retained {
    /// Create a retained subtree. `style` must describe the same box the
    /// builder's root element produces.
    pub fn new(
        id: impl Into<ElementId>,
        version: u64,
        style: Style,
        build: impl FnOnce(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            version,
            style,
            build: Some(Box::new(build)),
        }
    }
}

impl IntoElement for Retained {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[derive(Default)]
struct RetainedCache {
    version: Option<u64>,
    /// Frames in which `prepaint_range` / `paint_range` were last recorded or
    /// re-based. They are indices into that frame's stores, so they can only
    /// be replayed in the immediately following frame.
    prepaint_frame: u64,
    paint_frame: u64,
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
}

/// Frame-local state for [`Retained`] (public because it is an associated
/// element type, but not part of the stable API).
pub struct RetainedFrameState {
    child: Option<AnyElement>,
}

impl Element for Retained {
    type RequestLayoutState = RetainedFrameState;
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let global_id = global_id.expect("Retained requires an element id");
        let style = self.style.clone();
        let version = self.version;

        let (layout_id, _hit, child) =
            window.with_element_state::<RetainedCache, _>(global_id, |cache, window| {
                let mut cache = cache.unwrap_or_default();
                // The cached ranges point into the previous frame's stores.
                // Only reuse them when the subtree was actually replayed in
                // the previous frame; otherwise they are stale and would
                // replay unrelated entries.
                let hit = cache.version == Some(version)
                    && cache.prepaint_frame + 1 == window.frame_sequence()
                    && cache.paint_frame + 1 == window.frame_sequence()
                    && !window.refreshing;
                cache.version = Some(version);
                let layout_id = window.request_layout(style, [], cx);
                let child = if hit {
                    None
                } else {
                    let build = self
                        .build
                        .take()
                        .expect("Retained builder consumed more than once");
                    Some(build(window, cx))
                };
                ((layout_id, hit, child), cache)
            });

        (layout_id, RetainedFrameState { child })
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        frame_state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let global_id = global_id.expect("Retained requires an element id");

        if let Some(mut child) = frame_state.child.take() {
            let start = window.prepaint_index();
            child.layout_as_root(bounds.size.into(), window, cx);
            child.prepaint_at(bounds.origin, window, cx);
            let end = window.prepaint_index();
            window.with_element_state::<RetainedCache, _>(global_id, |cache, window| {
                let mut cache = cache.unwrap_or_default();
                cache.prepaint_range = start..end;
                cache.prepaint_frame = window.frame_sequence();
                ((), cache)
            });
            Some(child)
        } else {
            window.with_element_state::<RetainedCache, _>(global_id, |cache, window| {
                let cache = cache.unwrap_or_default();
                // Re-base the replayed range. Cached indices point into the
                // frame in which the subtree was last built, while the entries
                // are appended at a new offset every frame. Keeping the old
                // indices would make them refer to unrelated entries (or to no
                // entries at all) as soon as the number of element states
                // recorded before this subtree changes.
                let start = window.prepaint_index();
                window.reuse_prepaint(cache.prepaint_range.clone());
                let end = window.prepaint_index();
                (
                    (),
                    RetainedCache {
                        prepaint_range: start..end,
                        prepaint_frame: window.frame_sequence(),
                        ..cache
                    },
                )
            });
            None
        }
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _frame_state: &mut Self::RequestLayoutState,
        child: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let global_id = global_id.expect("Retained requires an element id");

        if let Some(child) = child {
            let start = window.paint_index();
            child.paint(window, cx);
            let end = window.paint_index();
            window.with_element_state::<RetainedCache, _>(global_id, |cache, window| {
                let mut cache = cache.unwrap_or_default();
                cache.paint_range = start..end;
                cache.paint_frame = window.frame_sequence();
                ((), cache)
            });
        } else {
            window.with_element_state::<RetainedCache, _>(global_id, |cache, window| {
                let cache = cache.unwrap_or_default();
                // See `prepaint`: replaying a range never updates its indices,
                // so re-base them to where this frame appended the entries.
                let start = window.paint_index();
                window.reuse_paint(cache.paint_range.clone());
                let end = window.paint_index();
                (
                    (),
                    RetainedCache {
                        paint_range: start..end,
                        paint_frame: window.frame_sequence(),
                        ..cache
                    },
                )
            });
        }
    }
}
