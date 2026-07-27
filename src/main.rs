mod dispatch;
mod types;

use std::collections::HashMap;
use std::process::Command;

use wayland_client::globals::registry_queue_init;
use wayland_client::{Connection, Proxy};

use dispatch::AppState;
use types::{SPAWN_COMMAND, TARGET_APP_ID, TARGET_TITLE};

fn main() {
    let debug_mode = std::env::args().any(|a| a == "-d");

    let conn = Connection::connect_to_env()
        .expect("could not connect to Wayland — is WAYLAND_DISPLAY set?");

    let (globals, mut event_queue) =
        registry_queue_init::<AppState>(&conn).expect("failed to initialise Wayland registry");

    let qh = event_queue.handle();

    use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1;
    use wayland_protocols::ext::workspace::v1::client::ext_workspace_manager_v1::ExtWorkspaceManagerV1;
    use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1;
    use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1;
    use wayland_client::protocol::wl_output::WlOutput;
    use wayland_client::protocol::wl_seat::WlSeat;

    let foreign_list: ExtForeignToplevelListV1 = globals
        .bind(&qh, 1..=1, ())
        .expect("ext-foreign-toplevel-list-v1 not advertised by compositor");

    let toplevel_info: ZcosmicToplevelInfoV1 = globals
        .bind(&qh, 2..=3, ())
        .expect("zcosmic_toplevel_info_v1 v2 not available — run on COSMIC desktop");

    let toplevel_manager: ZcosmicToplevelManagerV1 = globals
        .bind(&qh, 1..=4, ())
        .expect("zcosmic_toplevel_manager_v1 not available");

    let seat: WlSeat = globals
        .bind(&qh, 1..=8, ())
        .expect("no wl_seat");

    let workspace_manager: ExtWorkspaceManagerV1 = globals
        .bind(&qh, 1..=1, ())
        .expect("ext-workspace-manager-v1 not available");

    // Bind to wl_output (name 0 is the primary display output).
    let output: WlOutput = globals
        .bind(&qh, 4..=4, ())
        .expect("no wl_output");

    let mut state = AppState {
        _foreign_list: foreign_list,
        toplevel_info,
        toplevel_manager,
        _workspace_manager: workspace_manager,
        seat,
        windows: HashMap::new(),
        cosmic_to_foreign: HashMap::new(),
        enumeration_done: false,
        current_workspace: None,
        current_workspace_name: String::new(),
        outputs: vec![output],
    };

    // Drain events until the compositor finishes sending initial state.
    while !state.enumeration_done {
        event_queue
            .roundtrip(&mut state)
            .expect("Wayland dispatch error");
    }

    // Print state mode — exit early after printing.
    if debug_mode {
        print_debug(&state);
        return;
    }

    // Find windows matching the target app_id (and title, if set).
    let matching: Vec<types::WindowData> = state
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

    let current_output = state.outputs.first().cloned();

    // move to current workspace
    if let (Some(workspace), Some(output)) =
        (state.current_workspace.as_ref(), current_output)
    {
        state.toplevel_manager.move_to_ext_workspace(cosmic_handle, workspace, &output);
    }

    match (target.minimized, target.activated) {
        (true, _) => {
            state.toplevel_manager.unset_minimized(cosmic_handle);
            state.toplevel_manager.activate(cosmic_handle, &state.seat);
        }
        (false, true) => {
            state.toplevel_manager.set_minimized(cosmic_handle);
        }
        (false, false) => {
            state.toplevel_manager.activate(cosmic_handle, &state.seat);
        }
    }

    // Flush requests so the compositor acts on them before this process exits.
    event_queue
        .roundtrip(&mut state)
        .expect("failed to flush requests to compositor");
}

fn print_debug(state: &AppState) {
    println!("=== Current Workspace ===");
    match &state.current_workspace {
        Some(ws) => {
            println!("  id: {:?}", ws.id());
            println!("  name: {:?}", state.current_workspace_name);
        }
        None => println!("  (none)"),
    }
    println!("  outputs: {}", state.outputs.len());

    let matching: Vec<&types::WindowData> = state
        .windows
        .values()
        .filter(|w| {
            w.app_id == TARGET_APP_ID
                && TARGET_TITLE.map_or(true, |t| w.title == t)
        })
        .collect();

    println!();
    println!("=== Target Terminal (app_id='{}') ===", TARGET_APP_ID);
    if matching.is_empty() {
        println!("  (not found)");
    } else {
        for w in matching {
            println!("  app_id: {}", w.app_id);
            println!("  title: {}", w.title);
            println!("  activated: {}", w.activated);
            println!("  minimized: {}", w.minimized);
            println!("  workspace: {:?}", w.workspace.as_ref().map(|h| h.id()));
            println!("  cosmic_handle: {:?}", w.cosmic_handle.as_ref().map(|h| h.id()));
        }
    }
}
