//! Command line. A running rtify instance receives the arguments of every later invocation, so
//! `rtify --play-pause` from a compositor binding reaches the same actions as the GUI.

use clap::Parser;

use crate::commands::Command;

#[derive(Debug, Clone, Default, Parser)]
#[command(
    name = "rtify",
    version,
    about = "Spotify client with System24 styling, tray, global shortcuts and lyrics"
)]
pub struct Cli {
    /// Start (or keep running) without showing the window.
    #[arg(long, alias = "hidden")]
    pub background: bool,
    #[arg(long, help = Command::ToggleWindow.description())]
    pub toggle_window: bool,
    #[arg(long, help = Command::ShowWindow.description())]
    pub show: bool,
    #[arg(long, help = Command::HideWindow.description())]
    pub hide: bool,
    #[arg(long, help = Command::ToggleLyrics.description())]
    pub toggle_lyrics: bool,
    #[arg(long, help = Command::PlayPause.description())]
    pub play_pause: bool,
    #[arg(long, help = Command::Next.description())]
    pub next: bool,
    #[arg(long, help = Command::Previous.description())]
    pub previous: bool,
    #[arg(long, help = Command::ToggleShuffle.description())]
    pub toggle_shuffle: bool,
    #[arg(long, help = Command::CycleRepeat.description())]
    pub cycle_repeat: bool,
    #[arg(long, help = Command::VolumeUp.description())]
    pub volume_up: bool,
    #[arg(long, help = Command::VolumeDown.description())]
    pub volume_down: bool,
    #[arg(long, help = Command::Mute.description())]
    pub mute: bool,
    #[arg(long, help = Command::FocusSearch.description())]
    pub focus_search: bool,
    #[arg(long, help = Command::ToggleCompact.description())]
    pub toggle_compact: bool,
    /// Re-read the Noctalia palette (use as the template's post_hook).
    #[arg(long)]
    pub reload_theme: bool,
    #[arg(long, help = Command::Preferences.description())]
    pub preferences: bool,
    #[arg(long, help = Command::Quit.description())]
    pub quit: bool,
    /// Print Hyprland key bindings for the configured shortcuts and exit (`lua` or `conf`).
    #[arg(long, value_name = "SYNTAX")]
    pub print_hyprland_binds: Option<String>,
    /// Print the Noctalia user template and its templates.toml entry, then exit.
    #[arg(long)]
    pub print_noctalia_template: bool,
}

impl Cli {
    pub fn commands(&self) -> Vec<Command> {
        let flags = [
            (self.toggle_window, Command::ToggleWindow),
            (self.show, Command::ShowWindow),
            (self.hide, Command::HideWindow),
            (self.toggle_lyrics, Command::ToggleLyrics),
            (self.play_pause, Command::PlayPause),
            (self.next, Command::Next),
            (self.previous, Command::Previous),
            (self.toggle_shuffle, Command::ToggleShuffle),
            (self.cycle_repeat, Command::CycleRepeat),
            (self.volume_up, Command::VolumeUp),
            (self.volume_down, Command::VolumeDown),
            (self.mute, Command::Mute),
            (self.focus_search, Command::FocusSearch),
            (self.toggle_compact, Command::ToggleCompact),
            (self.reload_theme, Command::ReloadTheme),
            (self.preferences, Command::Preferences),
            (self.quit, Command::Quit),
        ];
        flags.into_iter().filter(|(on, _)| *on).map(|(_, c)| c).collect()
    }
}

pub const NOCTALIA_TEMPLATE: &str = include_str!("../data/noctalia/rtify.css");
pub const NOCTALIA_TOML: &str = include_str!("../data/noctalia/templates.toml");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_map_to_commands() {
        let cli = Cli::try_parse_from(["rtify", "--play-pause", "--toggle-window"]).unwrap();
        assert_eq!(cli.commands(), vec![Command::ToggleWindow, Command::PlayPause]);
        assert!(Cli::try_parse_from(["rtify", "--hidden"]).unwrap().background);
        assert!(Cli::try_parse_from(["rtify", "--bogus"]).is_err());
    }

    #[test]
    fn every_command_has_a_flag() {
        for c in Command::ALL {
            let flag = format!(
                "--{}",
                if c == Command::ShowWindow {
                    "show"
                } else if c == Command::HideWindow {
                    "hide"
                } else {
                    c.name()
                }
            );
            let cli = Cli::try_parse_from(["rtify", flag.as_str()]).unwrap_or_else(|e| panic!("{flag}: {e}"));
            assert_eq!(cli.commands(), vec![c]);
        }
    }
}
