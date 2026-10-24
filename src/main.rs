use clap::Parser;
use rtify::cli::{Cli, NOCTALIA_TEMPLATE, NOCTALIA_TOML};
use rtify::desktop::shortcuts;
use rtify::persistence::ConfigStore;

fn main() -> std::process::ExitCode {
    rtify::player::init();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("rtify=info")).init();
    let args: Vec<String> = std::env::args().collect();
    // Parse here first so --help, --version and typos are answered by this process, not
    // forwarded to an already running instance.
    let cli = Cli::parse_from(&args);
    if let Some(syntax) = &cli.print_hyprland_binds {
        let config = ConfigStore::load(&rtify::paths::config_file()).config;
        print!("{}", shortcuts::hyprland_snippet(&config.shortcuts.bindings, syntax != "conf"));
        return std::process::ExitCode::SUCCESS;
    }
    if cli.print_noctalia_template {
        println!("# ~/.config/noctalia/templates/rtify.css\n{NOCTALIA_TEMPLATE}\n{NOCTALIA_TOML}");
        return std::process::ExitCode::SUCCESS;
    }
    rtify::app::run(args).into()
}
