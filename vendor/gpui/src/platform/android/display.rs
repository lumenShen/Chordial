use crate::{Bounds, DisplayId, PlatformDisplay, Point, Result, px, size};
use std::sync::Arc;
use uuid::Uuid;

use super::bridge::AndroidBridge;

pub(crate) struct AndroidDisplay {
    bridge: Arc<AndroidBridge>,
}

impl AndroidDisplay {
    pub(crate) fn new(bridge: Arc<AndroidBridge>) -> Self {
        Self { bridge }
    }
}

impl std::fmt::Debug for AndroidDisplay {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AndroidDisplay")
            .finish_non_exhaustive()
    }
}

impl PlatformDisplay for AndroidDisplay {
    fn id(&self) -> DisplayId {
        DisplayId(0)
    }

    fn uuid(&self) -> Result<Uuid> {
        Ok(Uuid::nil())
    }

    fn bounds(&self) -> Bounds<crate::Pixels> {
        let (width, height, scale) = self.bridge.surface_size();
        Bounds::new(
            Point::default(),
            size(px(width / scale), px(height / scale)),
        )
    }
}
