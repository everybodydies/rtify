//! Global shortcuts through `org.freedesktop.portal.GlobalShortcuts` (xdg-desktop-portal-hyprland,
//! -kde, -gnome …). We register named actions with preferred triggers; the compositor or the user
//! decides the final binding. When no portal is available, compositor key bindings that run
//! `rtify --<action>` reach the same commands through the CLI.

use std::collections::BTreeMap;

use ashpd::desktop::CreateSessionOptions;
use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
use async_channel::Sender;
use futures_util::StreamExt;

use crate::commands::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum ShortcutEvent {
    Command(Command),
    /// Portal accepted the session; `(action, trigger description)` as finally bound.
    Bound(Vec<(String, String)>),
    Unavailable(String),
}

pub fn requested(bindings: &BTreeMap<String, String>) -> Vec<NewShortcut> {
    Command::shortcut_actions()
        .iter()
        .map(|c| {
            let s = NewShortcut::new(c.name(), c.description());
            match bindings.get(c.name()).filter(|t| !t.is_empty()) {
                Some(t) => s.preferred_trigger(t.as_str()),
                None => s,
            }
        })
        .collect()
}

/// Runs for the lifetime of the app on the Tokio runtime.
pub async fn run(bindings: BTreeMap<String, String>, tx: Sender<ShortcutEvent>) {
    if let Err(e) = session(bindings, &tx).await {
        log::info!("global shortcuts portal unavailable: {e}");
        let _ = tx.send(ShortcutEvent::Unavailable(e.to_string())).await;
    }
}

async fn session(bindings: BTreeMap<String, String>, tx: &Sender<ShortcutEvent>) -> Result<(), ashpd::Error> {
    let portal = GlobalShortcuts::new().await?;
    let session = portal.create_session(CreateSessionOptions::default()).await?;
    let request = portal
        .bind_shortcuts(&session, &requested(&bindings), None, BindShortcutsOptions::default())
        .await?;
    let bound = request.response()?;
    let list = bound
        .shortcuts()
        .iter()
        .map(|s| (s.id().to_owned(), s.trigger_description().to_owned()))
        .collect();
    let _ = tx.send(ShortcutEvent::Bound(list)).await;
    let mut activated = portal.receive_activated().await?;
    while let Some(ev) = activated.next().await {
        if let Some(c) = Command::from_name(ev.shortcut_id())
            && tx.send(ShortcutEvent::Command(c)).await.is_err()
        {
            break;
        }
    }
    // Keep the session alive as long as we listen.
    drop(session);
    Ok(())
}

/// Hyprland key bindings equivalent to the portal registration, for configs that prefer them.
/// Lua syntax for Hyprland 0.55+, hyprlang for older releases.
pub fn hyprland_snippet(bindings: &BTreeMap<String, String>, lua: bool) -> String {
    let mut out = String::new();
    for c in Command::shortcut_actions() {
        let Some(trigger) = bindings.get(c.name()).filter(|t| !t.is_empty()) else {
            continue;
        };
        let parts: Vec<&str> = trigger.split('+').map(str::trim).collect();
        let (key, mods) = parts
            .split_last()
            .map(|(k, m)| (*k, m.to_vec()))
            .unwrap_or((trigger.as_str(), vec![]));
        let media = key.starts_with("XF86");
        if lua {
            let combo = parts.join(" + ");
            let locked = if media { ", { locked = true }" } else { "" };
            out.push_str(&format!(
                "hl.bind(\"{combo}\", hl.dsp.exec_cmd(\"rtify --{}\"){locked})\n",
                c.name()
            ));
        } else {
            let kw = if media { "bindl" } else { "bind" };
            out.push_str(&format!("{kw} = {}, {key}, exec, rtify --{}\n", mods.join(" "), c.name()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Shortcuts;

    #[test]
    fn hyprland_snippets() {
        let b = Shortcuts::default().bindings;
        let lua = hyprland_snippet(&b, true);
        assert!(lua.contains("hl.bind(\"SUPER + ALT + m\", hl.dsp.exec_cmd(\"rtify --toggle-window\"))"));
        assert!(lua.contains("hl.bind(\"XF86AudioPlay\", hl.dsp.exec_cmd(\"rtify --play-pause\"), { locked = true })"));
        let conf = hyprland_snippet(&b, false);
        assert!(conf.contains("bind = SUPER ALT, m, exec, rtify --toggle-window"));
        assert!(conf.contains("bindl = , XF86AudioNext, exec, rtify --next"));
    }

    #[test]
    fn every_shortcut_is_requested() {
        assert_eq!(requested(&Shortcuts::default().bindings).len(), Command::shortcut_actions().len());
    }
}
