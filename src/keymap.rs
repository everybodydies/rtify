//! Keys inside the window. Every action can be rebound in Settings → shortcuts; the config keeps
//! only the keys that differ from the defaults (`[shortcuts.keys]`, "" = unassigned), so new
//! defaults reach existing setups.
//!
//! A key is written as modifiers and a GDK key name joined by `+`, modifiers in the order
//! ctrl, alt, shift, super: `space`, `shift+n`, `ctrl+k`, `alt+Left`, `F5`.

use std::collections::BTreeMap;

use gtk::gdk;

use crate::commands::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    PlayPause,
    SeekBack,
    SeekForward,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Mute,
    Search,
    Home,
    SearchView,
    Library,
    Releases,
    Back,
    Forward,
    Sidebar,
    Queue,
    Lyrics,
    PlaySelected,
    AddToLastPlaylist,
    Shuffle,
    Repeat,
    Undo,
    FindInList,
    ToggleWindow,
    Compact,
    Settings,
    ReloadTheme,
    Quit,
}

impl Action {
    pub const ALL: [Action; 29] = [
        Self::PlayPause,
        Self::SeekBack,
        Self::SeekForward,
        Self::Next,
        Self::Previous,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::Mute,
        Self::Search,
        Self::Home,
        Self::SearchView,
        Self::Library,
        Self::Releases,
        Self::Back,
        Self::Forward,
        Self::Sidebar,
        Self::Queue,
        Self::Lyrics,
        Self::PlaySelected,
        Self::AddToLastPlaylist,
        Self::Shuffle,
        Self::Repeat,
        Self::Undo,
        Self::FindInList,
        Self::ToggleWindow,
        Self::Compact,
        Self::Settings,
        Self::ReloadTheme,
        Self::Quit,
    ];

    /// The command this key runs, when it is one (the same command can also have a global key).
    pub fn command(self) -> Option<Command> {
        Some(match self {
            Self::PlayPause => Command::PlayPause,
            Self::Next => Command::Next,
            Self::Previous => Command::Previous,
            Self::VolumeUp => Command::VolumeUp,
            Self::VolumeDown => Command::VolumeDown,
            Self::Mute => Command::Mute,
            Self::Search => Command::FocusSearch,
            Self::Lyrics => Command::ToggleLyrics,
            Self::Shuffle => Command::ToggleShuffle,
            Self::Repeat => Command::CycleRepeat,
            Self::ToggleWindow => Command::ToggleWindow,
            Self::Compact => Command::ToggleCompact,
            Self::Settings => Command::Preferences,
            Self::ReloadTheme => Command::ReloadTheme,
            Self::Quit => Command::Quit,
            _ => return None,
        })
    }

    /// Key in `[shortcuts.keys]`; the command name where there is one.
    pub fn id(self) -> &'static str {
        if let Some(c) = self.command() {
            return c.name();
        }
        match self {
            Self::SeekBack => "seek-back",
            Self::SeekForward => "seek-forward",
            Self::Home => "home",
            Self::SearchView => "search-view",
            Self::Library => "library",
            Self::Releases => "releases",
            Self::Back => "back",
            Self::Forward => "forward",
            Self::Sidebar => "toggle-sidebar",
            Self::Queue => "toggle-queue",
            Self::PlaySelected => "play-selected",
            Self::AddToLastPlaylist => "add-to-last-playlist",
            Self::Undo => "undo",
            Self::FindInList => "find-in-list",
            _ => unreachable!("{self:?} has a command"),
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::PlayPause => "play / pause",
            Self::SeekBack => "seek back 5s",
            Self::SeekForward => "seek forward 5s",
            Self::Next => "next track",
            Self::Previous => "previous track",
            Self::VolumeUp => "volume up",
            Self::VolumeDown => "volume down",
            Self::Mute => "mute / unmute",
            Self::Search => "search",
            Self::Home => "home",
            Self::SearchView => "search view",
            Self::Library => "library",
            Self::Releases => "new releases",
            Self::Back => "back",
            Self::Forward => "forward",
            Self::Sidebar => "toggle sidebar",
            Self::Queue => "toggle queue",
            Self::Lyrics => "toggle lyrics",
            Self::PlaySelected => "play selected",
            Self::AddToLastPlaylist => "add to last playlist",
            Self::Shuffle => "shuffle",
            Self::Repeat => "cycle repeat",
            Self::Undo => "undo",
            Self::FindInList => "search in list",
            Self::ToggleWindow => "show / hide window",
            Self::Compact => "compact / full window",
            Self::Settings => "settings",
            Self::ReloadTheme => "reload theme",
            Self::Quit => "quit",
        }
    }

    pub fn default_key(self) -> &'static str {
        match self {
            Self::PlayPause => "space",
            Self::SeekBack => "Left",
            Self::SeekForward => "Right",
            Self::Next => "n",
            Self::Previous => "shift+n",
            Self::VolumeUp => "Up",
            Self::VolumeDown => "Down",
            Self::Mute => "m",
            Self::Search => "ctrl+k",
            Self::Home => "1",
            Self::SearchView => "2",
            Self::Library => "3",
            Self::Releases => "4",
            Self::Back => "alt+Left",
            Self::Forward => "alt+Right",
            Self::Sidebar => "b",
            Self::Queue => "q",
            Self::Lyrics => "l",
            Self::PlaySelected => "Return",
            Self::AddToLastPlaylist => "p",
            Self::Shuffle => "ctrl+s",
            Self::Repeat => "ctrl+r",
            Self::Undo => "ctrl+z",
            Self::FindInList => "ctrl+f",
            Self::ToggleWindow => "",
            Self::Compact => "ctrl+shift+m",
            Self::Settings => "ctrl+comma",
            Self::ReloadTheme => "F5",
            Self::Quit => "ctrl+q",
        }
    }

    /// The global (portal) shortcut id, for actions offered to the GlobalShortcuts portal.
    pub fn global(self) -> Option<Command> {
        self.command().filter(|c| Command::shortcut_actions().contains(c))
    }
}

/// The key bound to `a`: the override from the config, else the default.
pub fn key_of(keys: &BTreeMap<String, String>, a: Action) -> &str {
    keys.get(a.id()).map(String::as_str).unwrap_or(a.default_key())
}

/// The action bound to `combo` (the first one, when several share it).
pub fn action_for(keys: &BTreeMap<String, String>, combo: &str) -> Option<Action> {
    if combo.is_empty() {
        return None;
    }
    Action::ALL.into_iter().find(|&a| key_of(keys, a) == combo)
}

/// Other actions bound to the same key as `a`.
pub fn clashes(keys: &BTreeMap<String, String>, a: Action) -> Vec<Action> {
    let k = key_of(keys, a);
    if k.is_empty() {
        return Vec::new();
    }
    Action::ALL.into_iter().filter(|&o| o != a && key_of(keys, o) == k).collect()
}

/// Bind `combo` to `a` ("" clears it). An action that had `combo` takes `a`'s old key; it is
/// returned with that key.
pub fn rebind(keys: &mut BTreeMap<String, String>, a: Action, combo: &str) -> Option<(Action, String)> {
    let previous = key_of(keys, a).to_owned();
    let other = action_for(keys, combo).filter(|&o| o != a);
    keys.insert(a.id().to_owned(), combo.to_owned());
    if let Some(o) = other {
        keys.insert(o.id().to_owned(), previous.clone());
    }
    normalize(keys);
    other.map(|o| (o, previous))
}

/// Drop overrides that equal the default, and ones for actions that no longer exist.
pub fn normalize(keys: &mut BTreeMap<String, String>) {
    keys.retain(|id, k| Action::ALL.into_iter().any(|a| a.id() == id && a.default_key() != k));
}

const MODS: [(gdk::ModifierType, &str); 4] = [
    (gdk::ModifierType::CONTROL_MASK, "ctrl"),
    (gdk::ModifierType::ALT_MASK, "alt"),
    (gdk::ModifierType::SHIFT_MASK, "shift"),
    (gdk::ModifierType::SUPER_MASK, "super"),
];

/// The combo for a key press; None for a modifier on its own.
pub fn combo(key: gdk::Key, state: gdk::ModifierType) -> Option<String> {
    if is_modifier(key) {
        return None;
    }
    let name = key.to_lower().name()?;
    let mut parts: Vec<&str> = MODS.iter().filter(|(m, _)| state.contains(*m)).map(|(_, n)| *n).collect();
    parts.push(name.as_str());
    Some(parts.join("+"))
}

fn is_modifier(key: gdk::Key) -> bool {
    use gdk::Key as K;
    [
        K::Shift_L,
        K::Shift_R,
        K::Control_L,
        K::Control_R,
        K::Alt_L,
        K::Alt_R,
        K::Meta_L,
        K::Meta_R,
        K::Super_L,
        K::Super_R,
        K::Hyper_L,
        K::Hyper_R,
        K::ISO_Level3_Shift,
        K::ISO_Level5_Shift,
        K::Caps_Lock,
        K::Num_Lock,
    ]
    .contains(&key)
}

fn split(combo: &str) -> (Vec<&str>, &str) {
    match combo.rsplit_once('+') {
        // "ctrl++" would not come out of `combo`, but a hand-edited config might say "plus".
        Some((mods, key)) if !key.is_empty() => (mods.split('+').collect(), key),
        _ => (Vec::new(), combo),
    }
}

/// Whether the combo holds ctrl, alt or super, so it can act while a text field has focus.
pub fn has_command_modifier(combo: &str) -> bool {
    split(combo).0.iter().any(|m| matches!(*m, "ctrl" | "alt" | "super"))
}

/// Keys a focused text field handles itself (clipboard, undo, word movement).
pub fn is_text_editing(combo: &str) -> bool {
    let (mods, key) = split(combo);
    let ctrl = mods.contains(&"ctrl") && !mods.contains(&"alt") && !mods.contains(&"super");
    ctrl && matches!(
        key,
        "a" | "c" | "v" | "x" | "z" | "y" | "Left" | "Right" | "Home" | "End" | "BackSpace" | "Delete"
    )
}

/// Arrow, paging and activation keys that a focused list, slider or menu needs for itself.
pub fn is_navigation(combo: &str) -> bool {
    let (mods, key) = split(combo);
    mods.iter().all(|m| *m == "shift")
        && matches!(
            key,
            "Up" | "Down" | "Left" | "Right" | "Home" | "End" | "Page_Up" | "Page_Down" | "Return" | "KP_Enter"
        )
}

pub fn is_function_key(combo: &str) -> bool {
    let key = split(combo).1;
    key.strip_prefix('F')
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// How a key is shown: `shift+n`, `alt+←`, `enter`, `—` when unassigned.
pub fn label(combo: &str) -> String {
    if combo.is_empty() {
        return "—".into();
    }
    let (mut parts, key) = split(combo);
    let key = match key {
        "Left" => "←".to_owned(),
        "Right" => "→".to_owned(),
        "Up" => "↑".to_owned(),
        "Down" => "↓".to_owned(),
        "Return" | "KP_Enter" => "enter".to_owned(),
        "Escape" => "esc".to_owned(),
        "comma" => ",".to_owned(),
        "period" => ".".to_owned(),
        "slash" => "/".to_owned(),
        "minus" => "-".to_owned(),
        "equal" => "=".to_owned(),
        "Page_Up" => "page up".to_owned(),
        "Page_Down" => "page down".to_owned(),
        other => other.to_lowercase(),
    };
    parts.push(&key);
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_defaults_distinct() {
        let mut ids: Vec<_> = Action::ALL.iter().map(|a| a.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), Action::ALL.len());
        let keys = BTreeMap::new();
        for a in Action::ALL {
            assert!(clashes(&keys, a).is_empty(), "{a:?} shares its default key");
        }
    }

    #[test]
    fn every_global_shortcut_has_a_key_row() {
        for c in Command::shortcut_actions() {
            assert!(Action::ALL.iter().any(|a| a.global() == Some(*c)), "{c:?}");
        }
    }

    #[test]
    fn defaults_follow_the_handoff() {
        let keys = BTreeMap::new();
        assert_eq!(action_for(&keys, "space"), Some(Action::PlayPause));
        assert_eq!(action_for(&keys, "shift+n"), Some(Action::Previous));
        assert_eq!(action_for(&keys, "m"), Some(Action::Mute));
        assert_eq!(action_for(&keys, "ctrl+k"), Some(Action::Search));
        assert_eq!(action_for(&keys, "Return"), Some(Action::PlaySelected));
        assert_eq!(action_for(&keys, ""), None);
    }

    #[test]
    fn rebinding_swaps_and_stores_only_changes() {
        let mut keys = BTreeMap::new();
        // n is "next": next takes mute's old key.
        assert_eq!(rebind(&mut keys, Action::Mute, "n"), Some((Action::Next, "m".to_owned())));
        assert_eq!(key_of(&keys, Action::Mute), "n");
        assert_eq!(key_of(&keys, Action::Next), "m");
        // Swapping back leaves nothing to store.
        rebind(&mut keys, Action::Mute, "m");
        assert!(keys.is_empty(), "{keys:?}");
        // Clearing is kept; a free key swaps nothing.
        assert_eq!(rebind(&mut keys, Action::Queue, ""), None);
        assert_eq!(key_of(&keys, Action::Queue), "");
        assert_eq!(rebind(&mut keys, Action::ToggleWindow, "ctrl+w"), None);
        assert_eq!(keys.len(), 2);
        // The action that loses its key takes the old one of the action that got it.
        assert_eq!(
            rebind(&mut keys, Action::Lyrics, "ctrl+w"),
            Some((Action::ToggleWindow, "l".to_owned()))
        );
    }

    #[test]
    fn hand_edited_clashes_are_reported_both_ways() {
        let mut keys = BTreeMap::new();
        keys.insert("mute".to_owned(), "n".to_owned());
        assert_eq!(clashes(&keys, Action::Mute), vec![Action::Next]);
        assert_eq!(clashes(&keys, Action::Next), vec![Action::Mute]);
        keys.insert("gone".to_owned(), "x".to_owned());
        normalize(&mut keys);
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn combos_from_key_presses() {
        let none = gdk::ModifierType::empty();
        assert_eq!(combo(gdk::Key::space, none).as_deref(), Some("space"));
        assert_eq!(combo(gdk::Key::N, gdk::ModifierType::SHIFT_MASK).as_deref(), Some("shift+n"));
        assert_eq!(
            combo(gdk::Key::k, gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::LOCK_MASK).as_deref(),
            Some("ctrl+k")
        );
        assert_eq!(combo(gdk::Key::Left, gdk::ModifierType::ALT_MASK).as_deref(), Some("alt+Left"));
        assert_eq!(combo(gdk::Key::Shift_L, gdk::ModifierType::SHIFT_MASK), None);
    }

    #[test]
    fn classes_and_labels() {
        assert!(has_command_modifier("ctrl+k") && has_command_modifier("alt+Left"));
        assert!(!has_command_modifier("shift+n") && !has_command_modifier("space"));
        assert!(is_text_editing("ctrl+z") && is_text_editing("ctrl+Left") && !is_text_editing("ctrl+k"));
        assert!(is_navigation("Down") && is_navigation("shift+Up") && is_navigation("Return"));
        assert!(!is_navigation("alt+Left") && !is_navigation("space"));
        assert!(is_function_key("F5") && is_function_key("F11") && !is_function_key("f") && !is_function_key("F"));
        assert_eq!(label("shift+n"), "shift+n");
        assert_eq!(label("alt+Left"), "alt+←");
        assert_eq!(label("Return"), "enter");
        assert_eq!(label("ctrl+comma"), "ctrl+,");
        assert_eq!(label("F5"), "f5");
        assert_eq!(label(""), "—");
    }
}
