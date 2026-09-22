//! Compositor-independent domain logic for niri-zones.

mod backend;
mod engine;
mod geometry;
mod layout;
mod selection;
mod state;

pub use backend::{BackendError, BackendOp, MockBackend, MockFailure, WindowBackend, WindowId, WindowSnapshot};
pub use engine::{EngineError, SnapEngine, SnapOutcome};
pub use geometry::{GeometryError, NormalizedRect, Point, Rect, Size};
pub use layout::{builtin_layout, builtin_layouts, Axis, LayoutDefinition, LayoutError, LayoutKind, ResolvedZone, SplitNode, ZoneId, ZoneSpec};
pub use selection::{select_directional, Direction};
pub use state::{SnapRecord, SnapStateStore};
