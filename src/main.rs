//! Desktop companion app for Sony headphones and earbuds.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ui;

use ui::app::{LaunchOptions, Tab};

const USAGE: &str = "\
Usage: headphones-control [--demo [MODEL]] [--tab status|sound|system]

  --demo           Also list the built-in simulated devices (for development)
  --demo <MODEL>   Connect straight to a simulated device
                   (WH-1000XM6, WH-1000XM5, WF-1000XM5 or WH-1000XM4)
  --tab <TAB>      Tab to open after connecting
";

fn parse_args() -> Result<LaunchOptions, String> {
    let mut opts = LaunchOptions::default();
    let mut args = std::env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => {
                opts.show_demo = true;
                if let Some(model) = args.next_if(|a| !a.starts_with("--")) {
                    opts.demo = Some(model);
                }
            }
            "--tab" => {
                let key = args.next().unwrap_or_default();
                opts.tab =
                    Some(Tab::from_key(&key).ok_or("--tab must be status, sound or system")?);
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

/// The app icon, embedded in the binary.
fn window_icon() -> eframe::egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/headphones-control-256.png"))
        .unwrap_or_default()
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
            .with_title("Headphones Control")
            .with_app_id("headphones-control")
            .with_inner_size(
                headphones_control::config::Config::load()
                    .window_size
                    .map_or(
                        [ui::theme::PHONE_WIDTH, ui::theme::PHONE_HEIGHT],
                        |(w, h)| [w, h],
                    ),
            )
            .with_min_inner_size([340.0, 560.0])
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Headphones Control",
        options,
        Box::new(|cc| Ok(Box::new(ui::app::App::new(cc, opts)))),
    )
}
