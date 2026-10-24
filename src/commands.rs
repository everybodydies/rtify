//! Every way of controlling rtify (buttons, keyboard, tray, global shortcuts, CLI) resolves to
//! one of these commands, which the application registers as `app.<name>` actions. That keeps
//! hotkeys from growing a second playback implementation.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    ToggleWindow,
    ShowWindow,
    HideWindow,
    ToggleLyrics,
    PlayPause,
    Next,
    Previous,
    ToggleShuffle,
    CycleRepeat,
    VolumeUp,
    VolumeDown,
    Mute,
    FocusSearch,
    ToggleCompact,
    ReloadTheme,
    Preferences,
    Quit,
}

impl Command {
    pub const ALL: [Command; 17] = [
        Self::ToggleWindow,
        Self::ShowWindow,
        Self::HideWindow,
        Self::ToggleLyrics,
        Self::PlayPause,
        Self::Next,
        Self::Previous,
        Self::ToggleShuffle,
        Self::CycleRepeat,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::Mute,
        Self::FocusSearch,
        Self::ToggleCompact,
        Self::ReloadTheme,
        Self::Preferences,
        Self::Quit,
    ];

    /// Action name (`app.<name>`), CLI flag (`--<name>`) and portal shortcut id.
    pub fn name(self) -> &'static str {
        match self {
            Self::ToggleWindow => "toggle-window",
            Self::ShowWindow => "show",
            Self::HideWindow => "hide",
            Self::ToggleLyrics => "toggle-lyrics",
            Self::PlayPause => "play-pause",
            Self::Next => "next",
            Self::Previous => "previous",
            Self::ToggleShuffle => "toggle-shuffle",
            Self::CycleRepeat => "cycle-repeat",
            Self::VolumeUp => "volume-up",
            Self::VolumeDown => "volume-down",
            Self::Mute => "mute",
            Self::FocusSearch => "focus-search",
            Self::ToggleCompact => "toggle-compact",
            Self::ReloadTheme => "reload-theme",
            Self::Preferences => "preferences",
            Self::Quit => "quit",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ToggleWindow => "Show or hide the player window",
            Self::ShowWindow => "Show the player window",
            Self::HideWindow => "Hide the player window",
            Self::ToggleLyrics => "Show or hide lyrics",
            Self::PlayPause => "Play or pause",
            Self::Next => "Next track",
            Self::Previous => "Previous track",
            Self::ToggleShuffle => "Toggle shuffle",
            Self::CycleRepeat => "Cycle repeat (off, context, track)",
            Self::VolumeUp => "Volume up",
            Self::VolumeDown => "Volume down",
            Self::Mute => "Mute, or unmute to the previous volume",
            Self::FocusSearch => "Focus search",
            Self::ToggleCompact => "Switch compact/full window",
            Self::ReloadTheme => "Reload the theme and external palette",
            Self::Preferences => "Open preferences",
            Self::Quit => "Quit rtify",
        }
    }

    pub fn from_name(name: &str) -> Option<Command> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }

    /// Commands offered to the GlobalShortcuts portal.
    pub fn shortcut_actions() -> &'static [Command] {
        &[
            Self::ToggleWindow,
            Self::ToggleLyrics,
            Self::PlayPause,
            Self::Next,
            Self::Previous,
            Self::FocusSearch,
            Self::ToggleCompact,
            Self::VolumeUp,
            Self::VolumeDown,
            Self::Mute,
            Self::ToggleShuffle,
        ]
    }

    /// Whether running this command from the CLI should leave the window hidden.
    pub fn is_background(self) -> bool {
        !matches!(
            self,
            Self::ShowWindow | Self::ToggleWindow | Self::FocusSearch | Self::Preferences | Self::ToggleLyrics
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_roundtrip() {
        let mut names: Vec<_> = Command::ALL.iter().map(|c| c.name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), Command::ALL.len());
        for c in Command::ALL {
            assert_eq!(Command::from_name(c.name()), Some(c));
        }
    }

    #[test]
    fn default_bindings_name_real_actions() {
        for name in crate::config::Shortcuts::default().bindings.keys() {
            assert!(Command::from_name(name).is_some(), "{name}");
        }
    }
}
