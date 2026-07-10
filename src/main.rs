use std::collections::HashMap;
use std::process::Command;

use std::sync::Arc;
use wayland_backend::client::{ObjectData, ObjectId};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
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

// ZcosmicToplevelHandleV1 is in the toplevel_info module together with
// ZcosmicToplevelInfoV1 (same XML file in cosmic-protocols).
use cosmic_protocols::{
    toplevel_info::v1::client::zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1},
    toplevel_info::v1::client::zcosmic_toplevel_info_v1::{self, ZcosmicToplevelInfoV1},
    toplevel_management::v1::client::zcosmic_toplevel_manager_v1::{self, ZcosmicToplevelManagerV1},
};

const TARGET_APP_ID: &str = "util.cosmic-toggle-terminal.popup";
/// Restrict toggling to a window whose title exactly matches this string.
/// `None` matches any window with the right app_id.
/// `Some("…")` adds a title filter as a secondary guard (useful if --app-id
/// is not available and you rely on title matching instead).
const TARGET_TITLE: Option<&str> = None;
const SPAWN_COMMAND: &[&str] = &["alacritty", "--class", TARGET_APP_ID];

/// Per-window state collected from both Wayland protocols.
#[derive(Default, Clone)]
struct WindowData {
    app_id: String,
    title: String,
    activated: bool,
    minimized: bool,
    /// The COSMIC handle used to send management requests.
    cosmic_handle: Option<ZcosmicToplevelHandleV1>,
}

struct AppState {
    /// Keep the list proxy alive so the compositor keeps sending toplevel events.
    _foreign_list: ExtForeignToplevelListV1,
    toplevel_info: ZcosmicToplevelInfoV1,
    toplevel_manager: ZcosmicToplevelManagerV1,
    seat: WlSeat,
    /// Window state keyed by the foreign handle's ObjectId.
    windows: HashMap<ObjectId, WindowData>,
    /// Maps each cosmic handle ObjectId back to the foreign handle ObjectId so
    /// state events from the cosmic handle can update the right WindowData.
    cosmic_to_foreign: HashMap<ObjectId, ObjectId>,
    /// Becomes true when zcosmic_toplevel_info_v1 sends its Done event,
    /// signalling that all current window state has been delivered.
    enumeration_done: bool,
}

// ── Dispatch implementations ─────────────────────────────────────────────────
//
// wayland-client 0.31's Dispatch trait takes `state: &mut State` as an
// explicit first argument rather than a `&mut self` receiver.

// Required by registry_queue_init; we only need it to satisfy the bound since
// the GlobalList collects globals during its own internal roundtrip.
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

// Needed because ext_foreign_toplevel_handle_v1 delivers WlOutput objects in
// output_enter / output_leave events; we just ignore those events.
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

impl Dispatch<ExtForeignToplevelListV1, ()> for AppState {
    // The `toplevel` event (opcode 0) creates a new ExtForeignToplevelHandleV1.
    // wayland-client 0.31 requires event_created_child to supply the ObjectData
    // for any event that carries a new_id argument.
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
            // Clone the info proxy so we can release the shared borrow before
            // mutably accessing state.windows and state.cosmic_to_foreign.
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
            // title, output_enter, output_leave, done — not needed
            _ => {}
        }
    }
}

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

// UserData = ObjectId of the corresponding foreign handle, threaded through
// when we call get_cosmic_toplevel() so state events land in the right entry.
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
                // raw is a byte array; each u32 (native byte order) encodes one
                // State enum value: 0=Maximized, 1=Minimized, 2=Activated,
                // 3=Fullscreen, 4=Sticky
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

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let conn = Connection::connect_to_env()
        .expect("could not connect to Wayland — is WAYLAND_DISPLAY set?");

    let (globals, mut event_queue) =
        registry_queue_init::<AppState>(&conn).expect("failed to initialise Wayland registry");

    let qh = event_queue.handle();

    // Bind the required protocol globals.  These calls buffer wl_registry.bind
    // requests; they are flushed on the first roundtrip below.
    let foreign_list: ExtForeignToplevelListV1 = globals
        .bind(&qh, 1..=1, ())
        .expect("ext-foreign-toplevel-list-v1 not advertised by compositor");

    // Version 2 adds get_cosmic_toplevel() and the Done event on the info global.
    let toplevel_info: ZcosmicToplevelInfoV1 = globals
        .bind(&qh, 2..=3, ())
        .expect("zcosmic_toplevel_info_v1 v2 not available — run on COSMIC desktop");

    let toplevel_manager: ZcosmicToplevelManagerV1 = globals
        .bind(&qh, 1..=1, ())
        .expect("zcosmic_toplevel_manager_v1 not available");

    let seat: WlSeat = globals
        .bind(&qh, 1..=8, ())
        .expect("no wl_seat");

    let mut state = AppState {
        _foreign_list: foreign_list,
        toplevel_info,
        toplevel_manager,
        seat,
        windows: HashMap::new(),
        cosmic_to_foreign: HashMap::new(),
        enumeration_done: false,
    };

    // Dispatch until zcosmic_toplevel_info_v1 signals that the initial window
    // snapshot is complete.  Typically takes 2 roundtrips: the first delivers
    // ext_foreign_toplevel_handle events (which trigger get_cosmic_toplevel
    // calls), and the second delivers the resulting state + Done.
    while !state.enumeration_done {
        event_queue
            .roundtrip(&mut state)
            .expect("Wayland dispatch error");
    }

    // Collect windows matching the target app_id (and title, if set).
    let matching: Vec<WindowData> = state
        .windows
        .values()
        .filter(|w| {
            w.app_id == TARGET_APP_ID
                && TARGET_TITLE.map_or(true, |t| w.title == t)
        })
        .cloned()
        .collect();

    if matching.is_empty() {
        // Terminal not running — spawn it.
        Command::new(SPAWN_COMMAND[0])
            .args(&SPAWN_COMMAND[1..])
            .spawn()
            .expect("failed to spawn terminal");
        return;
    }

    // Prefer the currently-activated window; fall back to the last in the list.
    let target = matching
        .iter()
        .find(|w| w.activated)
        .unwrap_or_else(|| matching.last().unwrap())
        .clone();

    let cosmic_handle = target
        .cosmic_handle
        .as_ref()
        .expect("window has no COSMIC handle — is zcosmic_toplevel_info_v1 v2 supported?");

    if target.activated && !target.minimized {
        // Focused and visible → hide (popdown).
        state.toplevel_manager.set_minimized(cosmic_handle);
    } else {
        // Minimized or in background → raise and focus (popup).
        if target.minimized {
            state.toplevel_manager.unset_minimized(cosmic_handle);
        }
        state.toplevel_manager.activate(cosmic_handle, &state.seat);
    }

    // Flush requests so the compositor acts on them before this process exits.
    event_queue
        .roundtrip(&mut state)
        .expect("failed to flush requests to compositor");
}
