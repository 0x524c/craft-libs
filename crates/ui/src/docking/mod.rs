//! Shared panel docking: transactional layout changes and egui drag/drop presentation.
//!
//! Panel IDs identify application-owned contents, never document state or native windows. A
//! layout can be adapted to an existing workspace schema or serialized with the `serde` feature.
//! Validate deserialized layouts before rendering them. Every transaction checks its complete
//! input and commits only a valid result; a rejected move cannot lose the source panel.

mod dock_model;
mod model;
mod render;
mod resize;
mod surface;
mod targets;
mod workspace;

pub use dock_model::{Column, ColumnLocation, Group, GroupLocation, PanelState, Side, Target};
pub use model::{Action, DockError, Floating, Layout, Location, MAX_DEPTH, MAX_PANELS, Node, Permissions, Placement, StackEntry, Zone};
pub use surface::{DockArea, DockContent, DockOutput, DockStyle, DropFeedback, DropRequest, PanelLimits, begin_drag, cancel_drag, is_dragging};
pub use targets::{DropProposal, DropTarget, drop_zone, resolve_drop, tab_insertion_marker, zone_rect};
pub use workspace::Workspace;

use surface::{Drag, GroupDrag};

#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod tests;
