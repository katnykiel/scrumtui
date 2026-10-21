//! Keyboard layout detection.
//!
//! When the active system layout is Colemak, navigation keys are remapped so
//! that the physical QWERTY h/j/k/l positions keep working: Colemak puts
//! h/n/e/i there, so n↔j, e↔k and i↔l are swapped (h is already in place).
//! The layout is re-checked periodically so switching mid-session works.
//! `SCRUMTUI_LAYOUT=colemak|qwerty` overrides detection.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static COLEMAK: AtomicBool = AtomicBool::new(false);

const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Start tracking the active keyboard layout on a background thread.
pub fn start_detection() {
    if let Ok(v) = std::env::var("SCRUMTUI_LAYOUT") {
        COLEMAK.store(v.eq_ignore_ascii_case("colemak"), Ordering::Relaxed);
        return;
    }
    let _ = std::thread::Builder::new()
        .name("layout-detect".into())
        .spawn(|| loop {
            if let Some(colemak) = detect_colemak() {
                COLEMAK.store(colemak, Ordering::Relaxed);
            }
            std::thread::sleep(POLL_INTERVAL);
        });
}

/// Returns None when the layout can't be determined (keeps the last value).
fn detect_colemak() -> Option<bool> {
    let out = if cfg!(target_os = "macos") {
        Command::new("defaults")
            .args(["read", "com.apple.HIToolbox", "AppleSelectedInputSources"])
            .output()
            .ok()?
    } else if cfg!(target_os = "linux") {
        Command::new("setxkbmap").arg("-query").output().ok()?
    } else {
        return None;
    };
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_lowercase().contains("colemak"))
}

/// Translate a navigation-context key for the active layout. Only call this
/// where keys are commands, never where the user is typing text.
pub fn remap(key: KeyEvent) -> KeyEvent {
    if !COLEMAK.load(Ordering::Relaxed) {
        return key;
    }
    let KeyCode::Char(c) = key.code else { return key };
    // Ctrl-I is indistinguishable from Tab, so Ctrl-L must stay Ctrl-L.
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(c, 'i' | 'l' | 'I' | 'L') {
        return key;
    }
    let swapped = match c {
        'n' => 'j', 'j' => 'n',
        'e' => 'k', 'k' => 'e',
        'i' => 'l', 'l' => 'i',
        'N' => 'J', 'J' => 'N',
        'E' => 'K', 'K' => 'E',
        'I' => 'L', 'L' => 'I',
        _ => return key,
    };
    KeyEvent { code: KeyCode::Char(swapped), ..key }
}

/// Like `remap`, but leaves Ctrl-chords untouched.
pub fn remap_plain(key: KeyEvent) -> KeyEvent {
    if key.modifiers.contains(KeyModifiers::CONTROL) { key } else { remap(key) }
}
