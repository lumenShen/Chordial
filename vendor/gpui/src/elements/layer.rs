use crate::{
    AnyElement, App, Bounds, Element, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Window,
};

/// Groups a subtree into one painting layer.
///
/// Inside a layer all primitives share one draw order. gpui can therefore batch
/// all paths (and all quads/text runs) of the subtree into a single draw call,
/// instead of starting a full-viewport path pass for every interleaved icon.
///
/// Layer members are painted by kind order (quads, paths, glyphs, images), so
/// this is intended for sections where that ordering matches the desired
/// stacking (backgrounds below icons, icons below text). Do not wrap a
/// background image and the text drawn on top of it in the same layer.
pub struct BatchLayer {
    child: Option<AnyElement>,
    disabled: bool,
}

/// Wrap `child` in a single painting layer. See [`BatchLayer`].
pub fn batch_layer(child: impl IntoElement) -> BatchLayer {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    let disabled = *DISABLED.get_or_init(|| {
        matches!(
            std::env::var("CHORDIAL_NO_BATCH").as_deref(),
            Ok("1") | Ok("true")
        )
    });
    BatchLayer {
        child: Some(child.into_any_element()),
        disabled,
    }
}

impl IntoElement for BatchLayer {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for BatchLayer {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<crate::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut child = self
            .child
            .take()
            .expect("BatchLayer must only be laid out once per frame");
        let layout_id = child.request_layout(window, cx);
        (layout_id, child)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.disabled {
            child.paint(window, cx);
        } else {
            window.paint_layer(bounds, |window| child.paint(window, cx));
        }
    }
}
