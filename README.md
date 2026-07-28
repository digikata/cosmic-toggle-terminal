# COSMIC Toggle Terminal (CTT)

A CLI utility that toggles a terminal window show/hide on the COSMIC
desktop (Wayland-only), bound to a hotkey such as **F12**. The utility drives the
compositor's existing window-management protocol, an it works arbitrary terminal
as long as the terminal an set an appid/class. This is a utility control
to *raise-and-focus /minimize* a wayland terminal app rather than a slide-in overlay.

The default terminal that this works with is **Alacritty**. Ironically, 'alacritty'
is used as the hardcoded terminal for this as 'cosmic-terminal' doesn't have the
options to do this (as of v1.4.0) at the moment.  Other terminals could
be supported.

## 1. Triggering it with a hotkey

Bind the hotkey in **COSMIC Settings → Keyboard → Custom shortcuts**, with the
command set to invoke this CLI (e.g. `cosmic-toggle-term`). COSMIC custom
shortcuts can spawn arbitrary commands, so no key-grabbing logic lives in the
binary. F12 is a reasonable default but any free binding works.

---

## 2. Use case

1. User configures a COSMIC custom keyboard shortcut (e.g. F12) to run this
   CLI. The compositor invokes the CLI on each keypress — the CLI itself does
   **not** grab the key.
2. On each invocation the CLI looks at the current set of open windows and
    decides, for a target terminal:
    - **Not running** → launch it (a "run" / spawn).
    - **Running but not focused** → raise and focus it — *popup*.
    - **Running and currently focused/visible** → hide it (minimize) — *popdown*.

   The utility will also bring the terminal into the current workspace if
   its active or minimiezed in a different cosmic workspace.
  
3. The CLI does its work and exits. It is a short-lived process, not a daemon.

The target terminal is identified by its `app_id` (default
`util.termtoggle.popup`), which is set at launch via the terminal's `--class`
flag and never changes at runtime.

### Out of scope
- Setting an arbitrary position or size for the window.
- Pinning the window permanently above everything (always-on-top / overlay
  layer). `activate` raises to the top of normal windows *at the moment it is
  called*; focusing another window drops it back. A true quake-style
  slide-over would require a different design path.

---

## 3. References
- COSMIC protocol definitions and Rust bindings: `pop-os/cosmic-protocols`
  (the `cosmic-toplevel-info` and `cosmic-toplevel-management` protocols).
- Window behavior (run-or-raise, minimize toggle) implemented in Python over
  the same protocols: `lapause/cosmic-ext-window-helper`.
  https://github.com/lapause/cosmic-ext-window-helper
- Reddit discussion https://www.reddit.com/r/COSMICDE/comments/1rpvn69/question_regarding_compositor_scriptability
