//! Panel thread: polls `hyprctl cursorpos` + `monitors -j` every 200ms
//! for edge detection. Communicates with the Slint PanelWindow via
//! `invoke_from_event_loop`. Focus loss is handled via a separate
//! `activewindow` listener thread.

use crate::config::PanelEdge;
use slint::ComponentHandle;
use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// ── Constants ────────────────────────────────────────────────────────

/// How close the cursor must be to the configured edge to trigger the panel.
const TRIGGER_THRESHOLD: i32 = 10;

/// How far the cursor must be from the edge to start the collapse timer.
const COLLAPSE_THRESHOLD: i32 = 100;

/// How long to wait after cursor leaves the zone before collapsing.
const COLLAPSE_DELAY: Duration = Duration::from_millis(500);

/// Poll interval for the edge detection loop.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

// ── Types ────────────────────────────────────────────────────────────

/// Handle to a running panel thread. Drop signals shutdown and joins.
#[allow(dead_code)]
pub struct PanelHandle {
    pub shutdown: Arc<AtomicBool>,
    pub visible: Arc<AtomicBool>,
    pub expanded: Arc<AtomicBool>,
    edge_thread: Option<JoinHandle<()>>,
    focus_thread: Option<JoinHandle<()>>,
}

impl Drop for PanelHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(t) = self.edge_thread.take() {
            let _ = t.join();
        }
        if let Some(t) = self.focus_thread.take() {
            let _ = t.join();
        }
    }
}

// ── Public API ───────────────────────────────────────────────────────

/// Spawn the panel threads and return a handle.
///
/// `panel_weak` is used to show/hide the PanelWindow via invoke_from_event_loop.
/// `main_weak` is used to dispatch quick-control callbacks (toggle-system, etc.).
pub fn run(
    edge: PanelEdge,
    panel_weak: slint::Weak<crate::PanelWindow>,
    _main_weak: slint::Weak<crate::MainWindow>,
    expanded_width: i32,
) -> PanelHandle {
    let shutdown = Arc::new(AtomicBool::new(false));
    let visible = Arc::new(AtomicBool::new(false));
    let expanded = Arc::new(AtomicBool::new(false));

    // ── Edge detection thread ──
    let edge_shutdown = shutdown.clone();
    let edge_visible = visible.clone();
    let edge_expanded = expanded.clone();
    let edge_panel = panel_weak.clone();

    let edge_thread = thread::Builder::new()
        .name("hve-panel-edge".into())
        .spawn(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                edge_loop(
                    edge,
                    edge_shutdown,
                    edge_visible,
                    edge_expanded,
                    edge_panel,
                    expanded_width,
                );
            }));
            tracing::info!("[panel] Edge detection thread exited");
        });

    if let Err(e) = &edge_thread {
        tracing::error!("[panel] Failed to spawn edge thread: {}", e);
    }

    // ── Focus loss thread ──
    let focus_shutdown = shutdown.clone();
    let focus_visible = visible.clone();
    let focus_panel = panel_weak.clone();

    let focus_thread = thread::Builder::new()
        .name("hve-panel-focus".into())
        .spawn(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                focus_loop(focus_shutdown, focus_visible, focus_panel);
            }));
            tracing::info!("[panel] Focus listener thread exited");
        });

    if let Err(e) = &focus_thread {
        tracing::error!("[panel] Failed to spawn focus thread: {}", e);
    }

    PanelHandle {
        shutdown,
        visible,
        expanded,
        edge_thread: edge_thread.ok(),
        focus_thread: focus_thread.ok(),
    }
}

// ── Edge Detection Loop ──────────────────────────────────────────────

fn edge_loop(
    edge: PanelEdge,
    shutdown: Arc<AtomicBool>,
    visible: Arc<AtomicBool>,
    expanded: Arc<AtomicBool>,
    panel_weak: slint::Weak<crate::PanelWindow>,
    expanded_width: i32,
) {
    let mut last_trigger: Option<Instant> = None;
    let mut was_visible = false;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let cursor = get_cursor_pos();
        let monitors = get_monitors();

        match (cursor, monitors) {
            (Some(cursor), Some(monitors)) => {
                let in_zone = is_in_trigger_zone(cursor, &monitors, &edge);
                let far_from_edge = !is_near_edge(cursor, &monitors, &edge, COLLAPSE_THRESHOLD);

                if in_zone {
                    // Cursor is in the trigger zone → show + expand
                    last_trigger = Some(Instant::now());
                    visible.store(true, Ordering::Relaxed);
                    expanded.store(true, Ordering::Relaxed);

                    if !was_visible {
                        was_visible = true;
                        show_panel_and_expand(
                            &panel_weak,
                            cursor,
                            &monitors,
                            &edge,
                            expanded_width,
                        );
                    }
                } else if was_visible && far_from_edge {
                    // Cursor is far from edge → check collapse timer
                    // If last_trigger was reset in twilight zone, collapse immediately.
                    let should_collapse = last_trigger
                        .map_or(true, |t| t.elapsed() >= COLLAPSE_DELAY);
                    if should_collapse {
                        was_visible = false;
                        visible.store(false, Ordering::Relaxed);
                        expanded.store(false, Ordering::Relaxed);
                        hide_panel(&panel_weak);
                        last_trigger = None;
                    }
                } else if was_visible {
                    // Cursor in twilight zone (between trigger and collapse threshold):
                    // start the collapse timer if not already running.
                    // DO NOT reset it — that would break the timer when cursor
                    // later moves into far_from_edge territory.
                    last_trigger.get_or_insert_with(Instant::now);
                }
            }
            (_, _) => {
                // hyprctl failed — skip cycle
            }
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}

// ── Focus Loss Loop ──────────────────────────────────────────────────

fn focus_loop(
    shutdown: Arc<AtomicBool>,
    visible: Arc<AtomicBool>,
    panel_weak: slint::Weak<crate::PanelWindow>,
) {
    let instance = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
    let runtime =
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());

    let socket_path = match instance {
        Some(inst) => {
            std::path::PathBuf::from(&runtime)
                .join("hypr")
                .join(&inst)
                .join(".socket2.sock")
        }
        None => {
            tracing::warn!("[panel] No HYPRLAND_INSTANCE_SIGNATURE, focus loss disabled");
            return;
        }
    };

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        match connect_and_listen_focus(&socket_path, &shutdown, &visible, &panel_weak) {
            Ok(_) => break,
            Err(e) => {
                tracing::warn!("[panel] Focus listener error: {}, reconnecting in 2s", e);
                for _ in 0..20 {
                    if shutdown.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

fn connect_and_listen_focus(
    socket_path: &std::path::Path,
    shutdown: &AtomicBool,
    visible: &AtomicBool,
    panel_weak: &slint::Weak<crate::PanelWindow>,
) -> Result<(), String> {
    use std::io::BufReader;
    use std::os::unix::net::UnixStream;

    let stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("Cannot connect to focus socket: {}", e))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| format!("Cannot set timeout: {}", e))?;

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        if shutdown.load(Ordering::Relaxed) {
            return Ok(());
        }

        match line {
            Ok(line) => {
                if line.starts_with("activewindow>>") {
                    let title = line.trim_start_matches("activewindow>>");
                    let is_hve =
                        title.contains("Hyprland Visual Editor") || title.contains("HVE Panel");

                    if !is_hve && !title.is_empty() {
                        // Non-HVE window focused → hide panel immediately
                        visible.store(false, Ordering::Relaxed);

                        let w = panel_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(panel) = w.upgrade() {
                                let _ = panel.window().hide();
                            }
                        });
                    }
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => {
                return Err(format!("Read error: {}", e));
            }
        }
    }

    Ok(())
}

// ── hyprctl Helpers ──────────────────────────────────────────────────

/// Parse `hyprctl cursorpos` output → (x, y). Returns None on failure.
fn get_cursor_pos() -> Option<(i32, i32)> {
    let output = std::process::Command::new("hyprctl")
        .arg("cursorpos")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let mut parts = s.splitn(2, ',');
    let x = parts.next()?.trim().parse::<i32>().ok()?;
    let y = parts.next()?.trim().parse::<i32>().ok()?;
    Some((x, y))
}

/// Parsed monitor geometry from `hyprctl monitors -j`.
#[derive(Debug, Clone)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

fn get_monitors() -> Option<Vec<Monitor>> {
    let output = std::process::Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8_lossy(&output.stdout);
    let json: Vec<serde_json::Value> = serde_json::from_str(&s).ok()?;

    let monitors: Vec<Monitor> = json
        .iter()
        .filter_map(|m| {
            Some(Monitor {
                x: m["x"].as_i64()? as i32,
                y: m["y"].as_i64()? as i32,
                width: m["width"].as_i64()? as i32,
                height: m["height"].as_i64()? as i32,
            })
        })
        .collect();

    if monitors.is_empty() {
        return None;
    }
    Some(monitors)
}

// ── Edge Detection Logic ─────────────────────────────────────────────

/// Pure function: returns true if the cursor is within THRESHOLD pixels of
/// the configured edge on ANY connected monitor.
pub fn is_in_trigger_zone(cursor: (i32, i32), monitors: &[Monitor], edge: &PanelEdge) -> bool {
    is_near_edge(cursor, monitors, edge, TRIGGER_THRESHOLD)
}

/// Pure function: returns true if the cursor is within `threshold` pixels of
/// the configured edge on ANY connected monitor.
pub fn is_near_edge(
    cursor: (i32, i32),
    monitors: &[Monitor],
    edge: &PanelEdge,
    threshold: i32,
) -> bool {
    let (cx, cy) = cursor;

    monitors.iter().any(|m| match edge {
        PanelEdge::Left => {
            cx >= m.x && cx <= m.x + threshold && cy >= m.y && cy <= m.y + m.height
        }
        PanelEdge::Right => {
            let right_edge = m.x + m.width;
            cx <= right_edge
                && cx >= right_edge - threshold
                && cy >= m.y
                && cy <= m.y + m.height
        }
        PanelEdge::Top => {
            cy >= m.y && cy <= m.y + threshold && cx >= m.x && cx <= m.x + m.width
        }
        PanelEdge::Bottom => {
            let bottom_edge = m.y + m.height;
            cy <= bottom_edge
                && cy >= bottom_edge - threshold
                && cx >= m.x
                && cx <= m.x + m.width
        }
    })
}

// ── Panel Window Control ─────────────────────────────────────────────

fn show_panel_and_expand(
    panel_weak: &slint::Weak<crate::PanelWindow>,
    cursor: (i32, i32),
    monitors: &[Monitor],
    edge: &PanelEdge,
    expanded_width: i32,
) {
    let w = panel_weak.clone();
    let monitor_geo = get_trigger_monitor(cursor, monitors, edge);
    let edge_type = *edge;

    let _ = slint::invoke_from_event_loop(move || {
        if let Some(panel) = w.upgrade() {
            // Position at the configured edge of the trigger monitor
            if let Some(geo) = monitor_geo {
                let (px, py) = position_at_edge(geo, edge_type, expanded_width);
                let _ = panel
                    .window()
                    .set_position(slint::PhysicalPosition { x: px, y: py });
            }
            panel.set_panel_expanded(true);
            let _ = panel.window().show();
            panel.window().request_redraw();
        }
    });
}

fn hide_panel(panel_weak: &slint::Weak<crate::PanelWindow>) {
    let w = panel_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(panel) = w.upgrade() {
            panel.set_panel_expanded(false);
            let _ = panel.window().hide();
        }
    });
}

/// Find the monitor whose edge the cursor is near.
/// Returns the monitor that contains the cursor within COLLAPSE_THRESHOLD
/// pixels of the configured edge. Falls back to the first monitor if none match.
fn get_trigger_monitor(
    cursor: (i32, i32),
    monitors: &[Monitor],
    edge: &PanelEdge,
) -> Option<Monitor> {
    let (cx, cy) = cursor;
    let matched = monitors.iter().find(|m| match edge {
        PanelEdge::Left => {
            cx >= m.x && cx <= m.x + COLLAPSE_THRESHOLD && cy >= m.y && cy <= m.y + m.height
        }
        PanelEdge::Right => {
            let right_edge = m.x + m.width;
            cx >= right_edge - COLLAPSE_THRESHOLD
                && cx <= right_edge
                && cy >= m.y
                && cy <= m.y + m.height
        }
        PanelEdge::Top => {
            cy >= m.y && cy <= m.y + COLLAPSE_THRESHOLD && cx >= m.x && cx <= m.x + m.width
        }
        PanelEdge::Bottom => {
            let bottom_edge = m.y + m.height;
            cy >= bottom_edge - COLLAPSE_THRESHOLD
                && cy <= bottom_edge
                && cx >= m.x
                && cx <= m.x + m.width
        }
    });
    matched.or_else(|| monitors.first()).cloned()
}

/// Calculate the (x, y) position for the panel at the given edge of a monitor.
fn position_at_edge(monitor: Monitor, edge: PanelEdge, expanded_width: i32) -> (i32, i32) {
    match edge {
        PanelEdge::Left => (monitor.x, monitor.y),
        PanelEdge::Right => (monitor.x + monitor.width - expanded_width, monitor.y),
        PanelEdge::Top => (monitor.x, monitor.y),
        PanelEdge::Bottom => (monitor.x, monitor.y + monitor.height - expanded_width),
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_monitors() -> Vec<Monitor> {
        vec![
            Monitor {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            Monitor {
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
        ]
    }

    // ── is_in_trigger_zone ──

    #[test]
    fn test_left_edge_triggers() {
        let monitors = make_monitors();
        assert!(is_in_trigger_zone((5, 500), &monitors, &PanelEdge::Left));
    }

    #[test]
    fn test_left_edge_no_trigger() {
        let monitors = make_monitors();
        assert!(!is_in_trigger_zone((50, 500), &monitors, &PanelEdge::Left));
    }

    #[test]
    fn test_right_edge_triggers() {
        let monitors = make_monitors();
        assert!(is_in_trigger_zone((1915, 500), &monitors, &PanelEdge::Right));
    }

    #[test]
    fn test_right_edge_no_trigger() {
        let monitors = make_monitors();
        assert!(!is_in_trigger_zone((1900, 500), &monitors, &PanelEdge::Right));
    }

    #[test]
    fn test_right_edge_second_monitor_triggers() {
        let monitors = make_monitors();
        assert!(is_in_trigger_zone((3835, 500), &monitors, &PanelEdge::Right));
    }

    #[test]
    fn test_top_edge_triggers() {
        let monitors = make_monitors();
        assert!(is_in_trigger_zone((500, 5), &monitors, &PanelEdge::Top));
    }

    #[test]
    fn test_top_edge_no_trigger() {
        let monitors = make_monitors();
        assert!(!is_in_trigger_zone((500, 50), &monitors, &PanelEdge::Top));
    }

    #[test]
    fn test_bottom_edge_triggers() {
        let monitors = make_monitors();
        assert!(is_in_trigger_zone((500, 1075), &monitors, &PanelEdge::Bottom));
    }

    #[test]
    fn test_bottom_edge_no_trigger() {
        let monitors = make_monitors();
        assert!(!is_in_trigger_zone((500, 1000), &monitors, &PanelEdge::Bottom));
    }

    #[test]
    fn test_outside_monitor_bounds_no_trigger() {
        let monitors = make_monitors();
        // Cursor below all monitors
        assert!(!is_in_trigger_zone((500, 2000), &monitors, &PanelEdge::Left));
        // Cursor above all monitors (negative y)
        assert!(!is_in_trigger_zone((500, -50), &monitors, &PanelEdge::Top));
        // Cursor far right of all monitors
        assert!(!is_in_trigger_zone(
            (4000, 500),
            &monitors,
            &PanelEdge::Right
        ));
    }

    // ── position_at_edge ──

    #[test]
    fn test_position_at_right_edge() {
        let m = Monitor {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let (x, y) = position_at_edge(m, PanelEdge::Right, 300);
        assert_eq!(x, 1920 - 300);
        assert_eq!(y, 0);
    }

    #[test]
    fn test_position_at_right_edge_custom_width() {
        let m = Monitor {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let (x, y) = position_at_edge(m, PanelEdge::Right, 350);
        assert_eq!(x, 1920 - 350);
        assert_eq!(y, 0);
    }

    #[test]
    fn test_position_at_left_edge() {
        let m = Monitor {
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let (x, y) = position_at_edge(m, PanelEdge::Left, 300);
        assert_eq!(x, 1920);
        assert_eq!(y, 0);
    }
}
