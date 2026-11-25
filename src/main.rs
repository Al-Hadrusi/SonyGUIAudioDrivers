//! Desktop companion app for Sony headphones and earbuds.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ui;

use ui::app::{LaunchOptions, Tab};

const USAGE: &str = "\
Usage: sony-audio-control [--demo <MODEL>] [--tab status|sound|system]

  --demo <MODEL>   Connect to a built-in simulated device (e.g. WH-1000XM5, WF-1000XM5)
  --tab <TAB>      Tab to open after connecting
";

fn parse_args() -> Result<LaunchOptions, String> {
    let mut opts = LaunchOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => opts.demo = Some(args.next().ok_or("--demo needs a model name")?),
            "--tab" => {
                opts.tab = match args.next().as_deref() {
                    Some("status") => Tab::Status,
                    Some("sound") => Tab::Sound,
                    Some("system") => Tab::System,
                    _ => return Err("--tab must be status, sound or system".into()),
                }
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(opts)
}

fn main() -> eframe::Result {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Headphones")
            .with_app_id("sony-audio-control")
            .with_inner_size([ui::theme::PHONE_WIDTH, ui::theme::PHONE_HEIGHT])
            .with_min_inner_size([340.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Headphones",
        options,
        Box::new(|cc| Ok(Box::new(ui::app::App::new(cc, opts)))),
    )
}
