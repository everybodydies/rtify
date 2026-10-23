//! StatusNotifierItem tray (Noctalia, Waybar, KDE …) via `ksni`, without GTK3/AppIndicator.
//! The tray only sends `Command`s back to the GTK side.

use async_channel::Sender;
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Icon, MenuItem, OfflineReason, Orientation, ToolTip, TrayMethods};

use crate::commands::Command;
use crate::paths::APP_ID;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrayState {
    pub playing: bool,
    pub shuffle: bool,
    pub repeat: String,
    pub title: String,
    pub artist: String,
    pub window_visible: bool,
    pub online: bool,
}

pub struct RtifyTray {
    tx: Sender<Command>,
    pub state: TrayState,
}

impl RtifyTray {
    fn send(&self, c: Command) {
        let _ = self.tx.try_send(c);
    }
}

fn item(label: &str, command: Command) -> MenuItem<RtifyTray> {
    StandardItem {
        label: label.into(),
        activate: Box::new(move |t: &mut RtifyTray| t.send(command)),
        ..Default::default()
    }
    .into()
}

/// A 22×22 ARGB32 glyph: a bracketed play triangle, readable at panel size in light or dark bars.
pub fn glyph(size: i32, playing: bool) -> Icon {
    let s = size as f64;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
            let edge = (s * 0.12).max(1.5);
            // "[ ]" brackets.
            let bracket = (fy > s * 0.12 && fy < s * 0.88)
                && ((fx > s * 0.06 && fx < s * 0.06 + edge)
                    || (fx < s * 0.94 && fx > s * 0.94 - edge)
                    || ((fy < s * 0.12 + edge || fy > s * 0.88 - edge) && (fx < s * 0.28 || fx > s * 0.72)));
            let inner = if playing {
                // Pause bars.
                (fy > s * 0.3 && fy < s * 0.7) && ((fx > s * 0.36 && fx < s * 0.45) || (fx > s * 0.55 && fx < s * 0.64))
            } else {
                // Play triangle.
                let t = (fx - s * 0.36) / (s * 0.32);
                (0.0..=1.0).contains(&t) && (fy - s * 0.5).abs() < (1.0 - t) * s * 0.22
            };
            let a: u8 = if bracket || inner { 255 } else { 0 };
            // Light glyph with a dark outline-free fill: trays recolor or show it on dark panels.
            data.extend_from_slice(&[a, 0xd2, 0xd5, 0xdb]);
        }
    }
    Icon {
        width: size,
        height: size,
        data,
    }
}

impl ksni::Tray for RtifyTray {
    const MENU_ON_ACTIVATE: bool = false;

    fn id(&self) -> String {
        "rtify".into()
    }

    fn title(&self) -> String {
        "rtify".into()
    }

    fn icon_name(&self) -> String {
        APP_ID.into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![glyph(22, self.state.playing), glyph(44, self.state.playing)]
    }

    fn tool_tip(&self) -> ToolTip {
        let description = if self.state.title.is_empty() {
            if self.state.online {
                "nothing playing".into()
            } else {
                "offline".into()
            }
        } else {
            format!(
                "{}{}\n{}",
                if self.state.playing { "▶ " } else { "❚❚ " },
                self.state.title,
                self.state.artist
            )
        };
        ToolTip {
            title: "rtify".into(),
            description,
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Command::ToggleWindow);
    }

    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        self.send(Command::PlayPause);
    }

    fn scroll(&mut self, delta: i32, orientation: Orientation) {
        if orientation == Orientation::Vertical && delta != 0 {
            self.send(if delta < 0 { Command::VolumeUp } else { Command::VolumeDown });
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let s = &self.state;
        let now = if s.title.is_empty() {
            "nothing playing".to_owned()
        } else {
            format!("{} — {}", s.title, s.artist)
        };
        vec![
            StandardItem {
                label: now,
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            item(if s.window_visible { "Hide player" } else { "Show player" }, Command::ToggleWindow),
            item(if s.playing { "Pause" } else { "Play" }, Command::PlayPause),
            item("Next", Command::Next),
            item("Previous", Command::Previous),
            CheckmarkItem {
                label: "Shuffle".into(),
                checked: s.shuffle,
                activate: Box::new(|t: &mut Self| t.send(Command::ToggleShuffle)),
                ..Default::default()
            }
            .into(),
            item(
                &format!("Repeat: {}", if s.repeat.is_empty() { "off" } else { &s.repeat }),
                Command::CycleRepeat,
            ),
            MenuItem::Separator,
            item("Show lyrics", Command::ToggleLyrics),
            item("Settings", Command::Preferences),
            MenuItem::Separator,
            item("Quit", Command::Quit),
        ]
    }

    fn watcher_online(&self) {
        log::info!("tray: StatusNotifierWatcher available");
    }

    fn watcher_offline(&self, reason: OfflineReason) -> bool {
        // Keep the service alive; ksni re-registers when the watcher (e.g. a restarted
        // Noctalia) comes back. The window stays reachable via `rtify` or a shortcut meanwhile.
        log::warn!("tray: watcher offline ({reason:?}); waiting for it to return");
        true
    }
}

pub type TrayHandle = ksni::Handle<RtifyTray>;

/// Must run on the Tokio runtime.
pub async fn spawn(tx: Sender<Command>) -> Result<TrayHandle, ksni::Error> {
    RtifyTray {
        tx,
        state: TrayState::default(),
    }
    .assume_sni_available(true)
    .spawn()
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_has_pixels_and_transparency() {
        let i = glyph(22, false);
        assert_eq!(i.data.len(), 22 * 22 * 4);
        let opaque = i.data.chunks(4).filter(|p| p[0] == 255).count();
        assert!(opaque > 20 && opaque < 22 * 22 / 2, "{opaque}");
        assert_ne!(glyph(22, true).data, i.data);
    }
}
