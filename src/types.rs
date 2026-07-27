/// Shared types and constants.
use wayland_backend::client::ObjectId;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_seat::WlSeat;

use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1;
use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1;
use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1;
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1;
use wayland_protocols::ext::workspace::v1::client::ext_workspace_handle_v1::ExtWorkspaceHandleV1;
use wayland_protocols::ext::workspace::v1::client::ext_workspace_manager_v1::ExtWorkspaceManagerV1;

pub const TARGET_APP_ID: &str = "util.cosmic-toggle-terminal.popup";
pub const TARGET_TITLE: Option<&str> = None;
pub const SPAWN_COMMAND: &[&str] = &["alacritty", "--class", TARGET_APP_ID];

/// Per-window state collected from both Wayland protocols.
#[derive(Default, Clone)]
pub struct WindowData {
    pub app_id: String,
    pub title: String,
    pub activated: bool,
    pub minimized: bool,
    pub cosmic_handle: Option<ZcosmicToplevelHandleV1>,
    /// The ext workspace handle the toplevel belongs to (for workspace move detection).
    pub workspace: Option<ExtWorkspaceHandleV1>,
}

/// Global application state shared across all dispatch implementations.
pub struct AppState {
    /// Keep the foreign toplevel list proxy alive so the compositor keeps
    /// sending toplevel events.
    pub _foreign_list: ExtForeignToplevelListV1,
    pub toplevel_info: ZcosmicToplevelInfoV1,
    pub toplevel_manager: ZcosmicToplevelManagerV1,
    pub _workspace_manager: ExtWorkspaceManagerV1,
    pub seat: WlSeat,
    /// Window state keyed by the foreign handle's ObjectId.
    pub windows: std::collections::HashMap<ObjectId, WindowData>,
    /// Maps each cosmic handle ObjectId back to the foreign handle ObjectId.
    pub cosmic_to_foreign: std::collections::HashMap<ObjectId, ObjectId>,
    /// Becomes true when zcosmic_toplevel_info_v1 signals that all current
    /// window state has been delivered.
    pub enumeration_done: bool,
    /// The currently active (current) ext workspace handle.
    pub current_workspace: Option<ExtWorkspaceHandleV1>,
    /// Name of the current workspace (for debugging).
    pub current_workspace_name: String,
    /// Available wl_output objects from the Wayland registry.
    pub outputs: Vec<WlOutput>,
}
