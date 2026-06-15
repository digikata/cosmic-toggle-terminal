# COSMIC Terminal Toggle (CTT) — Conceptual Spec

A Rust CLI utility that toggles a terminal window show/hide on the COSMIC
desktop (Wayland-only), bound to a hotkey such as **F12**. This is the
"Path A" approach: it drives the compositor's existing window-management
protocol rather than drawing its own surface, so it works with an arbitrary
terminal (defaulting to COSMIC Terminal) but behaves as *raise-and-focus /
minimize* rather than a slide-in overlay.

This document is conceptual. It names the crates, protocols, and messages
involved and gives high-level pseudocode. Lower-level implementation details
(types, lifetimes, dispatch wiring, versions) are deliberately out of scope.

---

## 1. Use case

1. User configures a COSMIC custom keyboard shortcut (e.g. F12) to run this
   CLI. The compositor invokes the CLI on each keypress — the CLI itself does
   **not** grab the key.
2. On each invocation the CLI looks at the current set of open windows and
   decides, for a target terminal:
   - **Not running** → launch it (a "run" / spawn).
   - **Running and currently focused/visible** → hide it (minimize) — *popdown*.
   - **Running but not focused** → raise and focus it — *popup*.
3. The CLI does its work and exits. It is a short-lived, fire-and-forget
   process, not a daemon.

The target terminal is identified by its `app_id` (default
`com.system76.CosmicTerm`), with the spawn command configurable
(default launches COSMIC Terminal).

### Out of scope / not possible on this path
- Setting an arbitrary position or size for the window.
- Pinning the window permanently above everything (always-on-top / overlay
  layer). `activate` raises to the top of normal windows *at the moment it is
  called*; focusing another window drops it back. A true Yakuake-style
  slide-over would require the terminal itself to be a layer-shell client,
  which is a different ("Path B") design.

---

## 2. Architecture

```
[COSMIC custom shortcut: F12]
        │  spawns
        ▼
[this CLI]  ──Wayland──▶  [cosmic-comp compositor]
        │  1. enumerate open windows + their state
        │  2. decide: spawn | minimize | raise
        │  3. send the chosen request(s)
        ▼
     exit
```

The CLI is a Wayland **client**. It connects to the running compositor over
the user's Wayland socket, reads window state, sends one or more management
requests, flushes them to the compositor, and exits.

---

## 3. Crates

- **`wayland-client`** — core Wayland client connection, registry, event
  dispatch.
- **`smithay-client-toolkit`** (commonly aliased `sctk`) — convenience helpers
  for the registry, seats, and outputs. The **seat** is needed because the
  activate request must be attributed to a seat.
- **`wayland-protocols`** — provides the standard `ext-foreign-toplevel-list`
  protocol bindings used to enumerate windows.
- **`cosmic-protocols`** — Rust bindings for COSMIC's own Wayland protocol
  extensions (the toplevel-info and toplevel-management protocols below).
- **`cosmic-client-toolkit`** — client-side helpers built on top of
  `cosmic-protocols` that track toplevel info and expose the management
  manager. Using this removes most of the manual event-tracking boilerplate;
  it already correlates each window with its COSMIC handle.

A small argument parser (e.g. `clap`) is optional for CLI flags.

---

## 4. Wayland protocols involved

| Protocol | Role |
|---|---|
| `wl_registry` (core) | Discover and bind the globals below. |
| `wl_seat` (core) | Required argument to the `activate` request. |
| `ext-foreign-toplevel-list-v1` (standard) | Enumerate all toplevel windows; receive their `app_id`, `title`, and lifecycle (created/closed). |
| `cosmic-toplevel-info-unstable-v1` | Per-window **state** (the set: maximized / minimized / **activated** / fullscreen / sticky) and the mapping from each foreign-toplevel handle to a COSMIC toplevel handle. |
| `cosmic-toplevel-management-unstable-v1` | The **action** requests: `activate`, `set_minimized`, `unset_minimized` (also maximize/fullscreen/sticky/move/close, unused here). |

Why two COSMIC protocols: the *info* protocol tells you what windows exist and
their state and gives you a handle; the *management* protocol is what you send
actions to. The handle from info is the object you pass to management requests.

---

## 5. Messages

### Received (events) — used to build the world state
- **registry globals** advertised → bind the toplevel-info, toplevel-management,
  and seat globals.
- **toplevel created** + per-window **`app_id`**, **`title`**, and a **done**
  marker (from foreign-toplevel-list).
- **state** array for each window (from toplevel-info) — read whether the set
  contains `activated` and/or `minimized`.
- **toplevel closed** — window gone.
- **manager capabilities** (from toplevel-management) — optional: advertises
  which actions the compositor supports; can be checked before acting.

> Note: window info arrives asynchronously across several events. The client
> must dispatch/round-trip until the initial enumeration is "done" before its
> view of the windows is complete. This round-trip is the source of the small
> per-invocation latency.

### Sent (requests) — the actions
- **`activate(window_handle, seat)`** — raise + focus the target (popup). This
  is the only stacking primitive; there is no separate "raise" request.
- **`set_minimized(window_handle)`** — hide the target (popdown).
- **`unset_minimized(window_handle)`** — restore a minimized target before
  activating it, if it was minimized.

After sending, **flush / round-trip** so the compositor processes the requests
before the process exits.

---

## 6. High-level pseudocode

```
CONFIG:
    target_app_id  = "com.system76.CosmicTerm"   # overridable by flag
    spawn_command  = ["cosmic-term"]              # overridable by flag

main():
    conn   = connect_to_wayland_compositor()
    bind globals: foreign-toplevel-list, cosmic-toplevel-info,
                  cosmic-toplevel-management, seat

    # Gather a complete, current snapshot of all windows.
    round_trip until initial toplevel enumeration is "done"

    matches = [ w for w in all_windows if w.app_id == target_app_id ]

    if matches is empty:
        spawn(spawn_command)        # run
        flush; exit

    # Pick a target window. Simplest: prefer the currently-activated match,
    # otherwise the most-recently-seen match.
    target = activated_match(matches) or last(matches)

    if target.state contains ACTIVATED and not MINIMIZED:
        # It's up and focused → hide it.
        send set_minimized(target.handle)            # popdown
    else:
        # It's hidden or in the background → bring it forward.
        if target.state contains MINIMIZED:
            send unset_minimized(target.handle)
        send activate(target.handle, seat)           # popup

    flush / round_trip      # ensure requests reach the compositor
    exit
```

### Decision summary
| Target state | Action |
|---|---|
| No matching window | spawn terminal |
| Focused & visible (`activated`, not `minimized`) | `set_minimized` (popdown) |
| Minimized | `unset_minimized` then `activate` (popup) |
| Background (running, not focused, not minimized) | `activate` (popup) |

Optional refinement: combine with **`set_sticky`** so the terminal follows the
user across workspaces instead of switching workspace when activated from
elsewhere.

---

## 7. Triggering it (outside the binary)

Bind the hotkey in **COSMIC Settings → Keyboard → Custom shortcuts**, with the
command set to invoke this CLI (e.g. `cosmic-toggle-term`). COSMIC custom
shortcuts can spawn arbitrary commands, so no key-grabbing logic lives in the
binary. F12 is a reasonable default but any free binding works.

---

## 8. Open items for the implementation phase
- **Latency:** each press pays the enumeration round-trip. If it feels slow, a
  later iteration could keep a long-lived helper holding live state and accept
  toggle commands over a socket (the binary stays the same conceptually, the
  state-gathering moves into a resident service).
- **Target selection** when multiple windows share the `app_id` (which one is
  "the" terminal): last-focused vs first-created vs a saved id.
- **Capability check:** optionally verify the manager advertises `activate` /
  `minimize` before sending, and fail gracefully otherwise.
- **Multi-seat / multi-output** handling if relevant to the target setup.

---

## 9. References
- COSMIC protocol definitions and Rust bindings: `pop-os/cosmic-protocols`
  (the `cosmic-toplevel-info` and `cosmic-toplevel-management` protocols, plus
  the `cosmic-client-toolkit` helper crate and its toplevel examples).
- Prior-art behavior (run-or-raise, minimize toggle) implemented in Python over
  the same protocols: `lapause/cosmic-ext-window-helper`.