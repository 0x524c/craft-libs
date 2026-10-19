//! Shared panel docking: transactional layout changes and egui drag/drop presentation.
//!
//! Panel IDs identify application-owned contents, never document state or native windows. A
//! layout can be adapted to an existing workspace schema or serialized with the `serde` feature.
//! Validate deserialized layouts before rendering them. Every transaction checks its complete
//! input and commits only a valid result; a rejected move cannot lose the source panel.

pub mod chrome;
mod dock_model;
mod hits;
mod model;
mod render;
mod resize;
mod surface;
mod targets;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod workspace;

pub use hits::{Divider, Hit, ResizeEdge};
pub use render::{close_flyout, flyout, open_flyout};
pub use dock_model::{Column, ColumnLocation, Group, GroupLocation, PanelState, Side, Target};
pub use model::{Action, DockError, Floating, Layout, Location, MAX_DEPTH, MAX_PANELS, Node, Permissions, Placement, StackEntry, Zone};
pub use surface::{
    DockArea, DockContent, DockEvent, DockOutput, DockStyle, DropFeedback, DropRequest, HidePanels, PanelLimits, Profile, begin_drag, cancel_drag,
    is_dragging,
};
pub use targets::{DropProposal, DropTarget, drop_zone, resolve_drop, tab_insertion_marker, zone_rect};
pub use workspace::Workspace;

use surface::{Drag, GroupDrag, GroupGesture};

#[cfg(test)]
mod dock_tests;
#[cfg(test)]
mod floating_tests;
#[cfg(test)]
mod group_tests;
#[cfg(test)]
mod iconic_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod tests;
