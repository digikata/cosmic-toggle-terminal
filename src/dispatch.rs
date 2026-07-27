// Dispatch implementations grouped by protocol.

use std::sync::Arc;
use wayland_backend::client::{ObjectData, ObjectId};
use wayland_client::{
    globals::GlobalListContents,
    protocol::{
        wl_output::{self, WlOutput},
        wl_registry::{self, WlRegistry},
        wl_seat::{self, WlSeat},
    },
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};
use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::{
    self, ZcosmicToplevelHandleV1,
};
use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_info_v1::{
    self, ZcosmicToplevelInfoV1,
};
use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::{
    self, ZcosmicToplevelManagerV1,
};
use wayland_client::WEnum;

pub use crate::types::AppState;
use crate::types::WindowData;

// ── Wayland core protocols ───────────────────────────────────────────────────

impl Dispatch<WlRegistry, GlobalListContents> for AppState {
    fn event(
        _state: &mut Self,
        _registry: &WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &WlSeat,
        _event: wl_seat::Event,
        _udata: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

// ext_foreign_toplevel_handle_v1 delivers WlOutput objects in output_enter /
// output_leave events; we just ignore those events.
impl Dispatch<WlOutput, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &WlOutput,
        _event: wl_output::Event,
        _udata: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

// ── Foreign toplevel list ────────────────────────────────────────────────────

impl Dispatch<ExtForeignToplevelListV1, ()> for AppState {
    fn event_created_child(opcode: u16, qh: &QueueHandle<Self>) -> Arc<dyn ObjectData> {
        match opcode {
            0 => qh.make_data::<ExtForeignToplevelHandleV1, ()>(()),
            _ => panic!("unknown opcode {opcode} for ext_foreign_toplevel_list_v1"),
        }
    }

    fn event(
        state: &mut Self,
        _proxy: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _udata: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            let foreign_id = toplevel.id();
            let info = state.toplevel_info.clone();
            let cosmic = info.get_cosmic_toplevel(&toplevel, qh, foreign_id.clone());
            state.cosmic_to_foreign.insert(cosmic.id(), foreign_id.clone());
            state.windows.insert(
                foreign_id,
                WindowData {
                    cosmic_handle: Some(cosmic),
                    ..Default::default()
                },
            );
        }
    }
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _udata: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let id = proxy.id();
        match event {
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(w) = state.windows.get_mut(&id) {
                    w.app_id = app_id;
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Title { title } => {
                if let Some(w) = state.windows.get_mut(&id) {
                    w.title = title;
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Closed => {
                state.windows.remove(&id);
                state.cosmic_to_foreign.retain(|_, v| *v != id);
            }
            _ => {}
        }
    }
}

// ── Ext workspace protocols ──────────────────────────────────────────────────

impl Dispatch<ExtWorkspaceManagerV1, ()> for AppState {
    fn event_created_child(opcode: u16, qh: &QueueHandle<Self>) -> Arc<dyn ObjectData> {
        match opcode {
            0 => qh.make_data::<ExtWorkspaceGroupHandleV1, ()>(()),
            1 => qh.make_data::<ExtWorkspaceHandleV1, ()>(()),
            _ => panic!("unknown opcode {opcode} for ext_workspace_manager_v1"),
        }
    }

    fn event(
        _state: &mut Self,
        _proxy: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _udata: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let ext_workspace_manager_v1::Event::Done = event {
            // Workspace enumeration complete; the Done from toplevel_info
            // will arrive shortly after and set enumeration_done = true.
        }
        // WorkspaceGroup, Workspace, Finished — handled by child dispatchers.
    }
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &ExtWorkspaceGroupHandleV1,
        _event: ext_workspace_group_handle_v1::Event,
        _udata: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _udata: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_handle_v1::Event::State { state: raw_state } => {
                let is_current = match raw_state {
                    WEnum::Value(flags) => flags.contains(ext_workspace_handle_v1::State::Active),
                    WEnum::Unknown(bits) => bits & 1 != 0, // Active = bit 0
                };
                let matches_current = state
                    .current_workspace
                    .as_ref()
                    .map(|h| h.id() == proxy.id())
                    .unwrap_or(false);
                if is_current {
                    state.current_workspace = Some(proxy.clone());
                } else if matches_current {
                    state.current_workspace = None;
                }
            }
            ext_workspace_handle_v1::Event::Name { name } => {
                if state
                    .current_workspace
                    .as_ref()
                    .map(|h| h.id() == proxy.id())
                    .unwrap_or(false)
                {
                    state.current_workspace_name = name;
                }
            }
            _ => {}
        }
    }
}

// ── Cosmic toplevel protocols ────────────────────────────────────────────────

impl Dispatch<ZcosmicToplevelInfoV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _proxy: &ZcosmicToplevelInfoV1,
        event: zcosmic_toplevel_info_v1::Event,
        _udata: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let zcosmic_toplevel_info_v1::Event::Done = event {
            state.enumeration_done = true;
        }
    }
}

impl Dispatch<ZcosmicToplevelHandleV1, ObjectId> for AppState {
    fn event(
        state: &mut Self,
        _proxy: &ZcosmicToplevelHandleV1,
        event: zcosmic_toplevel_handle_v1::Event,
        foreign_id: &ObjectId,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zcosmic_toplevel_handle_v1::Event::State { state: raw } => {
                let states: Vec<u32> = raw
                    .chunks_exact(4)
                    .map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                if let Some(w) = state.windows.get_mut(foreign_id) {
                    w.minimized = states.contains(&1);
                    w.activated = states.contains(&2);
                }
            }
            zcosmic_toplevel_handle_v1::Event::Closed => {
                state.windows.remove(foreign_id);
                state.cosmic_to_foreign.retain(|_, v| v != foreign_id);
            }
            zcosmic_toplevel_handle_v1::Event::ExtWorkspaceEnter { workspace } => {
                if let Some(w) = state.windows.get_mut(foreign_id) {
                    w.workspace = Some(workspace);
                }
            }
            zcosmic_toplevel_handle_v1::Event::ExtWorkspaceLeave { .. } => {
                if let Some(w) = state.windows.get_mut(foreign_id) {
                    w.workspace = None;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZcosmicToplevelManagerV1, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &ZcosmicToplevelManagerV1,
        _event: zcosmic_toplevel_manager_v1::Event,
        _udata: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}
