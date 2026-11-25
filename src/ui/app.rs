//! Application screens: device picker and the per-device control screen with
//! the "Status", "Sound" and "System" tabs.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, CornerRadius, Layout, RichText, Sense, Ui, vec2};

use super::notify;
use super::theme::{self, *};
use super::widgets::*;
use headphones_control::battery_alert::LowBatteryAlert;
use headphones_control::config::Config;
use headphones_control::desktop_entry;
use headphones_control::protocol::*;
use headphones_control::transport::{self, DeviceInfo, FormFactor};
use headphones_control::worker::{Connection, Event};

enum Screen {
    Devices,
    Connecting {
        device: DeviceInfo,
        conn: Connection,
        since: Instant,
    },
    Device(Box<DeviceView>),
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Status,
    Sound,
    System,
}

impl Tab {
    pub fn key(self) -> &'static str {
        match self {
            Tab::Status => "status",
            Tab::Sound => "sound",
            Tab::System => "system",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [Tab::Status, Tab::Sound, Tab::System]
            .into_iter()
            .find(|t| t.key() == key)
    }
}

pub struct App {
    screen: Screen,
    devices: Vec<DeviceInfo>,
    device_note: Option<String>,
    scanning: Option<Receiver<(Vec<DeviceInfo>, Option<String>)>>,
    toast: Option<(String, Instant)>,
    start_tab: Tab,
    /// Current window size, saved on exit.
    window_size: Option<(f32, f32)>,
    show_demo: bool,

    config: Config,
    /// Set when the user disconnects on purpose: no automatic reconnect
    /// until they pick a device again.
    suppress_auto: bool,
    last_scan: Instant,
    /// Whether the running scan was started by the user (shows a spinner).
    scan_visible: bool,
    last_auto_attempt: Option<Instant>,
    /// Whether a `.desktop` entry exists (application menu + Wayland icon).
    menu_installed: bool,
    /// Why the tray icon isn't available, if it isn't.
    tray_note: Option<String>,
    #[cfg(all(feature = "tray", target_os = "linux"))]
    tray: Option<super::tray::Tray>,
    /// Set when "Quit" was chosen in the tray (then closing really quits).
    #[cfg(all(feature = "tray", target_os = "linux"))]
    quitting: bool,
}

/// How often the device list is refreshed in the background.
const BACKGROUND_SCAN: Duration = Duration::from_secs(4);
/// Minimum time between two automatic connection attempts.
const AUTO_RETRY: Duration = Duration::from_secs(20);

/// Command-line options.
#[derive(Default)]
pub struct LaunchOptions {
    /// Show the simulated demo devices in the device list (`--demo`).
    pub show_demo: bool,
    /// Connect straight to a simulated device of this model (`--demo <MODEL>`).
    pub demo: Option<String>,
    /// Tab to open (`--tab`); otherwise the last one used.
    pub tab: Option<Tab>,
}

struct DeviceView {
    info: DeviceInfo,
    form: FormFactor,
    conn: Connection,
    tab: Tab,

    ambient: AmbientSound,
    /// Mode to restore when Ambient Sound Control is switched back on.
    last_on_mode: AmbientMode,
    eq: Equalizer,
    upsampling: bool,
    codec: Option<Codec>,
    firmware: Option<String>,
    battery: Option<Battery>,
    battery_right: Option<Battery>,
    battery_case: Option<Battery>,
    battery_alert: LowBatteryAlert,

    speak_to_chat: bool,
    stc: SpeakToChatConfig,
    touch_panel: bool,
    pause_when_off: bool,
    auto_power_off: AutoPowerOff,
    confirm_power_off: bool,
    dialect: Dialect,

    /// `None` until the headset reports it (models without it never do).
    listening: Option<ListeningMode>,
    room: RoomSize,
    surround: Option<Surround>,
    /// `Some(false)` when the current Listening Mode blocks the equalizer.
    eq_available: Option<bool>,
    connection: Option<ConnectionPriority>,
    nc_amb_button: Option<NcAmbButton>,
    playback: Option<PlaybackState>,
    volume: Option<u8>,
    voice_guidance: Option<bool>,
    voice_volume: Option<i8>,
    /// `Some(false)` when the current Listening Mode blocks DSEE.
    upsampling_available: Option<bool>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: LaunchOptions) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = Self {
            screen: Screen::Devices,
            devices: Vec::new(),
            device_note: None,
            scanning: None,
            toast: None,
            start_tab: Tab::Status,
            window_size: None,
            show_demo: opts.show_demo || opts.demo.is_some(),
            config: Config::load(),
            suppress_auto: opts.demo.is_some(),
            last_scan: Instant::now(),
            scan_visible: true,
            last_auto_attempt: None,
            menu_installed: desktop_entry::is_installed(),
            tray_note: None,
            #[cfg(all(feature = "tray", target_os = "linux"))]
            tray: None,
            #[cfg(all(feature = "tray", target_os = "linux"))]
            quitting: false,
        };
        #[cfg(all(feature = "tray", target_os = "linux"))]
        match super::tray::Tray::spawn(&cc.egui_ctx) {
            Ok(tray) => app.tray = Some(tray),
            Err(e) => app.tray_note = Some(e),
        }
        app.start_tab = opts
            .tab
            .or_else(|| app.config.last_tab.as_deref().and_then(Tab::from_key))
            .unwrap_or_default();
        app.ensure_menu_entry();
        app.rescan(&cc.egui_ctx, true);
        if let Some(model) = opts.demo {
            let device = DeviceInfo {
                address: format!("demo:{model}"),
                name: model,
                connected: false,
            };
            app.connect(&cc.egui_ctx, device, false);
        }
        app
    }

    fn rescan(&mut self, ctx: &egui::Context, visible: bool) {
        self.last_scan = Instant::now();
        self.scan_visible = visible;
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let include_demo = self.show_demo;
        std::thread::spawn(move || {
            let _ = tx.send(transport::list_devices(include_demo));
            ctx.request_repaint();
        });
        self.scanning = Some(rx);
    }

    /// On Wayland the taskbar icon comes only from the application-menu
    /// entry, so create it on first start (unless the user removed it) and
    /// re-point it when the program it launches has moved.
    fn ensure_menu_entry(&mut self) {
        if self.show_demo {
            return; // development runs don't touch the user's menu
        }
        let check = desktop_entry::check();
        let needed = match check {
            desktop_entry::EntryCheck::Ok => false,
            desktop_entry::EntryCheck::Missing => !self.config.menu_entry_removed,
            desktop_entry::EntryCheck::Stale => true,
        };
        if !needed {
            return;
        }
        match desktop_entry::install() {
            Ok(()) if check == desktop_entry::EntryCheck::Missing => self.toast(
                "Added Headphones Control to your application menu. Restart the app once to see its icon in the taskbar.",
            ),
            Ok(()) => {}
            Err(e) => self.toast(format!("Could not add the application-menu entry: {e}")),
        }
        self.menu_installed = desktop_entry::is_installed();
    }

    fn save_config(&mut self) {
        if let Err(e) = self.config.save() {
            self.toast(format!("Could not save settings: {e}"));
        }
    }

    /// Connect to the remembered headset once BlueZ reports it connected
    /// (e.g. right after it was switched on).
    fn maybe_auto_connect(&mut self, ctx: &egui::Context) {
        if !self.config.auto_connect
            || self.suppress_auto
            || !matches!(self.screen, Screen::Devices)
        {
            return;
        }
        if self
            .last_auto_attempt
            .is_some_and(|t| t.elapsed() < AUTO_RETRY)
        {
            return;
        }
        let Some(last) = &self.config.last_device else {
            return;
        };
        let Some(found) = self
            .devices
            .iter()
            .find(|d| d.address == last.address && d.connected)
            .cloned()
        else {
            return;
        };
        self.last_auto_attempt = Some(Instant::now());
        self.connect(ctx, found, false);
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    /// `chosen` is true when the user picked the device (it becomes the
    /// remembered device) and false for automatic connections.
    fn connect(&mut self, ctx: &egui::Context, device: DeviceInfo, chosen: bool) {
        if chosen {
            self.suppress_auto = false;
            if !device.is_demo()
                && self.config.last_device.as_ref().map(|d| &d.address) != Some(&device.address)
            {
                self.config.last_device = Some(DeviceInfo {
                    connected: false,
                    ..device.clone()
                });
                self.save_config();
            }
        }
        let wake_ctx = ctx.clone();
        let conn = Connection::spawn(device.clone(), move || wake_ctx.request_repaint());
        self.screen = Screen::Connecting {
            device,
            conn,
            since: Instant::now(),
        };
    }

    fn process_events(&mut self) {
        let mut next: Option<Screen> = None;
        let mut toasts = Vec::new();
        match &mut self.screen {
            Screen::Devices => {}
            Screen::Connecting { conn, .. } => {
                while let Ok(ev) = conn.events.try_recv() {
                    match ev {
                        Event::Connected {
                            device,
                            form,
                            dialect,
                        } => {
                            if let Screen::Connecting { conn, .. } =
                                std::mem::replace(&mut self.screen, Screen::Devices)
                            {
                                let mut view = DeviceView::new(device, form, dialect, conn);
                                view.tab = self.start_tab;
                                next = Some(Screen::Device(Box::new(view)));
                            }
                            break;
                        }
                        Event::Disconnected(reason) => {
                            toasts.push(reason.unwrap_or_else(|| "Disconnected".into()));
                            next = Some(Screen::Devices);
                            break;
                        }
                        Event::Warning(_) | Event::Update(_) | Event::DialectChanged(_) => {}
                    }
                }
            }
            Screen::Device(view) => {
                while let Ok(ev) = view.conn.events.try_recv() {
                    match ev {
                        Event::Update(n) => view.apply(n),
                        Event::Warning(w) => toasts.push(w),
                        Event::Disconnected(reason) => {
                            toasts.push(reason.unwrap_or_else(|| "Disconnected".into()));
                            next = Some(Screen::Devices);
                            break;
                        }
                        Event::DialectChanged(d) => view.dialect = d,
                        Event::Connected { .. } => {}
                    }
                }
            }
        }
        if let Some(s) = next {
            self.screen = s;
        }
        if let Some(t) = toasts.pop() {
            self.toast(t);
        }
    }
}

impl eframe::App for App {
    /// Non-UI work; eframe also calls this while the window is hidden in the
    /// tray, so the connection keeps being serviced.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.window_size.is_some() && self.window_size != self.config.window_size {
            self.config.window_size = self.window_size;
            self.save_config();
        }
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(r) = ctx.input(|i| i.viewport().inner_rect)
            && !ctx.input(|i| i.viewport().minimized.unwrap_or(false))
        {
            self.window_size = Some((r.width().round(), r.height().round()));
        }
        // Remember the tab for next time.
        if let Screen::Device(view) = &self.screen {
            let key = view.tab.key();
            if self.config.last_tab.as_deref() != Some(key) {
                self.config.last_tab = Some(key.to_string());
                self.start_tab = view.tab;
                self.save_config();
            }
        }
        if let Some(rx) = &self.scanning
            && let Ok((devices, note)) = rx.try_recv()
        {
            self.devices = devices;
            self.device_note = note;
            self.scanning = None;
            self.maybe_auto_connect(ctx);
        }
        self.process_events();
        if matches!(self.screen, Screen::Devices)
            && self.scanning.is_none()
            && self.last_scan.elapsed() >= BACKGROUND_SCAN
        {
            self.rescan(ctx, false);
        }
        #[cfg(all(feature = "tray", target_os = "linux"))]
        self.tray_logic(ctx);
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG))
            .show(ui, |ui| {
                // Keep a phone-like column even if the window is resized.
                let full = ui.available_rect_before_wrap();
                let width = full.width().min(PHONE_WIDTH + 40.0);
                let col = egui::Rect::from_min_size(
                    egui::pos2(full.center().x - width / 2.0, full.top()),
                    vec2(width, full.height()),
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(col)
                        .layout(Layout::top_down(Align::Min)),
                );
                let mut action = Action::None;
                match &mut self.screen {
                    Screen::Devices => {
                        let auto = self
                            .config
                            .last_device
                            .as_ref()
                            .map(|d| (d.address.as_str(), self.config.auto_connect));
                        action = devices_screen(
                            &mut child,
                            &self.devices,
                            self.device_note.as_deref(),
                            self.scanning.is_some() && self.scan_visible,
                            auto,
                            self.menu_installed,
                            self.tray_note.as_deref(),
                        );
                    }
                    Screen::Connecting { device, since, .. } => {
                        if connecting_screen(&mut child, device, *since) {
                            action = Action::Cancel;
                        }
                    }
                    Screen::Device(view) => {
                        if view.show(&mut child) {
                            action = Action::Cancel;
                        }
                    }
                }
                match action {
                    Action::None => {}
                    Action::Rescan => self.rescan(&ctx, true),
                    Action::Connect(d) => self.connect(&ctx, d, true),
                    Action::SetAutoConnect(on) => {
                        self.config.auto_connect = on;
                        self.save_config();
                    }
                    Action::AddToMenu(add) => {
                        let result = if add {
                            desktop_entry::install()
                        } else {
                            desktop_entry::uninstall()
                        };
                        match result {
                            Ok(()) => self.toast(if add {
                                "Added to the application menu"
                            } else {
                                "Removed from the application menu"
                            }),
                            Err(e) => {
                                self.toast(format!("Could not update the application menu: {e}"))
                            }
                        }
                        self.menu_installed = desktop_entry::is_installed();
                        self.config.menu_entry_removed = !add;
                        self.save_config();
                    }
                    Action::Cancel => {
                        // The user left on purpose: don't reconnect behind their back.
                        self.suppress_auto = true;
                        self.screen = Screen::Devices;
                        self.rescan(&ctx, true);
                    }
                }
            });

        self.show_toast(&ctx);
    }
}

#[cfg(all(feature = "tray", target_os = "linux"))]
impl App {
    fn tray_logic(&mut self, ctx: &egui::Context) {
        use super::tray::{TrayCommand, TrayStatus};
        use egui::ViewportCommand;

        let Some(tray) = &mut self.tray else { return };
        let commands: Vec<TrayCommand> = tray.commands.try_iter().collect();
        let can_hide = tray.is_visible() && std::env::var_os("WAYLAND_DISPLAY").is_none();
        for cmd in commands {
            match cmd {
                TrayCommand::ShowWindow => {
                    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(ViewportCommand::Focus);
                }
                TrayCommand::SetAmbient(mode) => {
                    if let Screen::Device(view) = &mut self.screen {
                        view.set_ambient_mode(mode);
                    }
                }
                TrayCommand::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
        }

        // Closing the window keeps the app running in the tray, but only
        // when a tray host is showing the icon (so it can be reopened).
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && can_hide {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }

        let status = match &self.screen {
            Screen::Device(view) => Some(TrayStatus {
                name: view.info.name.clone(),
                battery: view.battery.map(|b| b.level),
                mode: view.ambient.mode,
            }),
            _ => None,
        };
        if let Some(tray) = &mut self.tray {
            tray.set_status(status);
        }
    }
}

impl App {
    fn show_toast(&mut self, ctx: &egui::Context) {
        let Some((msg, at)) = &self.toast else { return };
        let age = at.elapsed().as_secs_f32();
        if age > 4.0 {
            self.toast = None;
            return;
        }
        let alpha = ((4.0 - age) * 2.0).min(1.0);
        egui::Area::new(egui::Id::unique("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -28.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(0x30, 0x30, 0x35).gamma_multiply(alpha))
                    .corner_radius(CornerRadius::same(20))
                    .inner_margin(egui::Margin::symmetric(18, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(PHONE_WIDTH - 60.0);
                        ui.label(
                            RichText::new(msg.as_str())
                                .size(13.5)
                                .color(TEXT.gamma_multiply(alpha)),
                        );
                    });
            });
        ctx.request_repaint();
    }
}

enum Action {
    None,
    Rescan,
    Connect(DeviceInfo),
    SetAutoConnect(bool),
    /// Add (`true`) or remove the application-menu entry.
    AddToMenu(bool),
    Cancel,
}

// ---------------------------------------------------------------------------
// Device list
// ---------------------------------------------------------------------------

fn devices_screen(
    ui: &mut Ui,
    devices: &[DeviceInfo],
    note: Option<&str>,
    scanning: bool,
    // Remembered device address and whether auto-connect is on.
    auto: Option<(&str, bool)>,
    menu_installed: bool,
    tray_note: Option<&str>,
) -> Action {
    let mut action = Action::None;
    egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Headphones").size(28.0).color(TEXT).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if scanning {
                    ui.spinner();
                } else if ui.add(egui::Button::new(RichText::new("Refresh").color(ACCENT)).frame(false)).clicked() {
                    action = Action::Rescan;
                }
            });
        });
        ui.label(RichText::new("Select the device you want to connect to.").size(14.0).color(SUBTEXT));
        ui.add_space(14.0);

        let (real, demo): (Vec<_>, Vec<_>) = devices.iter().partition(|d| !d.is_demo());
        section_label(ui, "Paired devices");
        if real.is_empty() {
            card(ui, None, |ui| {
                ui.label(
                    RichText::new(if scanning { "Searching…" } else { "No paired Sony headset found." })
                        .size(14.5)
                        .color(SUBTEXT),
                );
                if let Some(n) = note {
                    ui.label(RichText::new(n).size(12.5).color(MUTED));
                } else if !scanning {
                    ui.label(
                        RichText::new("Pair your headset in your system's Bluetooth settings first, then tap Refresh.")
                            .size(12.5)
                            .color(MUTED),
                    );
                }
            });
        }
        for d in real {
            let remembered = auto.is_some_and(|(addr, _)| addr == d.address);
            if device_row(ui, d, remembered) {
                action = Action::Connect(d.clone());
            }
        }
        if let Some((_, on)) = auto {
            ui.add_space(8.0);
            card(ui, None, |ui| {
                let mut on = on;
                if switch_row(
                    ui,
                    "Connect automatically",
                    Some("Connect to your last headset as soon as it is turned on"),
                    &mut on,
                ) {
                    action = Action::SetAutoConnect(on);
                }
            });
        }
        ui.add_space(8.0);
        section_label(ui, "App");
        card(ui, None, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - 90.0);
                    ui.label(RichText::new("Application menu").size(15.5).color(TEXT));
                    let sub = if menu_installed {
                        "Headphones Control is in your application menu."
                    } else {
                        "Add a launcher and icon to your application menu (also shows the app icon in the taskbar on Wayland)."
                    };
                    ui.label(RichText::new(sub).size(12.5).color(SUBTEXT));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (label, add) = if menu_installed { ("Remove", false) } else { ("Add", true) };
                    if ui.add(egui::Button::new(RichText::new(label).color(ACCENT)).frame(false)).clicked() {
                        action = Action::AddToMenu(add);
                    }
                });
            });
            if let Some(note) = tray_note {
                divider(ui);
                ui.label(RichText::new("Tray icon not available").size(15.5).color(TEXT));
                ui.label(RichText::new(note).size(12.0).color(MUTED));
            }
        });
        if !demo.is_empty() {
            ui.add_space(8.0);
            section_label(ui, "Demo (simulated)");
        }
        for d in demo {
            if device_row(ui, d, false) {
                action = Action::Connect(d.clone());
            }
        }
    });
    action
}

fn device_row(ui: &mut Ui, d: &DeviceInfo, remembered: bool) -> bool {
    let resp = egui::Frame::new()
        .fill(CARD)
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 22.0, CARD_HI);
                let initial = if FormFactor::from_name(&d.name) == FormFactor::Earbuds {
                    "WF"
                } else {
                    "WH"
                };
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    initial,
                    egui::FontId::proportional(13.0),
                    ACCENT,
                );
                ui.vertical(|ui| {
                    ui.label(RichText::new(&d.name).size(16.0).color(TEXT));
                    let mut sub = if d.is_demo() {
                        "Simulated device".to_string()
                    } else if d.connected {
                        format!("Connected · {}", d.address)
                    } else {
                        d.address.clone()
                    };
                    if remembered {
                        sub.push_str(" · Last used");
                    }
                    ui.label(RichText::new(sub).size(12.5).color(if d.connected {
                        GOOD
                    } else {
                        SUBTEXT
                    }));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("›").size(24.0).color(MUTED));
                });
            });
        })
        .response;
    let resp = ui.interact(
        resp.rect,
        ui.scope_id().with(("dev", &d.address)),
        Sense::click(),
    );
    if resp.hovered() {
        ui.painter().rect_stroke(
            resp.rect,
            CornerRadius::same(CARD_RADIUS),
            egui::Stroke::new(1.0, MUTED),
            egui::StrokeKind::Inside,
        );
    }
    resp.clicked()
}

fn connecting_screen(ui: &mut Ui, device: &DeviceInfo, since: Instant) -> bool {
    let mut cancel = false;
    ui.add_space(140.0);
    ui.vertical_centered(|ui| {
        product_art(ui, FormFactor::from_name(&device.name));
        ui.add_space(20.0);
        ui.label(RichText::new(&device.name).size(22.0).color(TEXT).strong());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(ui.available_width() / 2.0 - 60.0);
            ui.spinner();
            ui.label(RichText::new("Connecting…").size(15.0).color(SUBTEXT));
        });
        if since.elapsed() > Duration::from_secs(4) {
            ui.label(
                RichText::new("Make sure the headset is turned on and nearby.")
                    .size(12.5)
                    .color(MUTED),
            );
        }
        ui.add_space(30.0);
        if ui
            .add(egui::Button::new(RichText::new("Cancel").color(ACCENT)).frame(false))
            .clicked()
        {
            cancel = true;
        }
    });
    cancel
}

// ---------------------------------------------------------------------------
// Device screen
// ---------------------------------------------------------------------------

impl DeviceView {
    fn new(info: DeviceInfo, form: FormFactor, dialect: Dialect, conn: Connection) -> Self {
        Self {
            info,
            form,
            dialect,
            conn,
            tab: Tab::Status,
            ambient: AmbientSound::default(),
            last_on_mode: AmbientMode::NoiseCancelling,
            eq: Equalizer::default(),
            upsampling: false,
            codec: None,
            firmware: None,
            battery: None,
            battery_right: None,
            battery_case: None,
            battery_alert: LowBatteryAlert::default(),
            speak_to_chat: false,
            stc: SpeakToChatConfig::default(),
            touch_panel: true,
            pause_when_off: true,
            auto_power_off: AutoPowerOff::Off,
            confirm_power_off: false,
            listening: None,
            room: RoomSize::Small,
            surround: None,
            eq_available: None,
            connection: None,
            nc_amb_button: None,
            playback: None,
            volume: None,
            voice_guidance: None,
            voice_volume: None,
            upsampling_available: None,
        }
    }

    /// Warn (desktop notification) when the headset battery runs low. For
    /// earbuds the emptier bud counts; the charging case doesn't.
    fn check_battery(&mut self) {
        let buds = [self.battery, self.battery_right];
        let Some(lowest) = buds.iter().flatten().map(|b| b.level).min() else {
            return;
        };
        let charging = buds.iter().flatten().all(|b| b.charging);
        if let Some(alert) = self.battery_alert.update(lowest, charging) {
            notify::battery(&self.info.name, alert);
        }
    }

    fn apply(&mut self, n: Notification) {
        let battery_update = matches!(n, Notification::Battery { .. });
        self.apply_inner(n);
        if battery_update {
            self.check_battery();
        }
    }

    fn apply_inner(&mut self, n: Notification) {
        match n {
            Notification::AmbientSound(a) => {
                self.ambient = a;
                if a.mode != AmbientMode::Off {
                    self.last_on_mode = a.mode;
                }
            }
            Notification::Equalizer(eq) => self.eq = eq,
            Notification::AudioUpsampling(on) => self.upsampling = on,
            Notification::Codec(c) => self.codec = Some(c),
            Notification::FirmwareVersion(v) => self.firmware = Some(v),
            Notification::Battery { kind, left, right } => match kind {
                BatteryKind::Single => self.battery = Some(left),
                BatteryKind::Dual | BatteryKind::Dual2 => {
                    self.battery = Some(left);
                    self.battery_right = right;
                }
                BatteryKind::Case => self.battery_case = Some(left),
            },
            Notification::BgmMode { on, room } => {
                self.room = room;
                let current = self.listening.unwrap_or_default();
                self.listening = Some(if on {
                    ListeningMode::Bgm
                } else if current == ListeningMode::Bgm {
                    ListeningMode::Standard
                } else {
                    current
                });
            }
            Notification::CinemaMode(on) => {
                let current = self.listening.unwrap_or_default();
                self.listening = Some(if on {
                    ListeningMode::Cinema
                } else if current == ListeningMode::Cinema {
                    ListeningMode::Standard
                } else {
                    current
                });
            }
            Notification::Surround(mode) => self.surround = Some(mode),
            Notification::EqAvailable(ok) => self.eq_available = Some(ok),
            Notification::ConnectionPriority(p) => self.connection = Some(p),
            Notification::NcAmbButton(b) => self.nc_amb_button = Some(b),
            Notification::Playback(state) => self.playback = Some(state),
            Notification::Volume(v) => self.volume = Some(v),
            Notification::VoiceGuidance(on) => self.voice_guidance = Some(on),
            Notification::VoiceGuidanceVolume(v) => self.voice_volume = Some(v),
            Notification::UpsamplingAvailable(ok) => self.upsampling_available = Some(ok),
            Notification::SpeakToChat(on) => self.speak_to_chat = on,
            Notification::SpeakToChatConfig(cfg) => {
                let focus = self.stc.focus_on_voice;
                self.stc = cfg;
                if self.dialect.is_v2() {
                    self.stc.focus_on_voice = focus;
                }
            }
            Notification::TouchSensor(on) => self.touch_panel = on,
            Notification::PauseWhenTakenOff(on) => self.pause_when_off = on,
            Notification::AutoPowerOff(apo) => self.auto_power_off = apo,
            Notification::InitReply(_) | Notification::Unknown(_) => {}
        }
    }

    /// Switch NC / ambient / off (used by the tray menu).
    #[cfg_attr(not(all(feature = "tray", target_os = "linux")), allow(dead_code))]
    fn set_ambient_mode(&mut self, mode: AmbientMode) {
        if self.ambient.mode == mode {
            return;
        }
        self.ambient.mode = mode;
        if mode != AmbientMode::Off {
            self.last_on_mode = mode;
        }
        self.send_ambient(false);
    }

    fn send_ambient(&self, adjusting: bool) {
        self.conn.send(Request::SetAmbientSound {
            value: self.ambient,
            adjusting,
        });
    }

    /// Returns `true` when the user leaves the device screen.
    fn show(&mut self, ui: &mut Ui) -> bool {
        let mut leave = false;
        // Top app bar
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(8, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("‹").size(26.0).color(TEXT))
                                .frame(false),
                        )
                        .on_hover_text("Devices")
                        .clicked()
                    {
                        self.conn.disconnect();
                        leave = true;
                    }
                    ui.label(
                        RichText::new(&self.info.name)
                            .size(17.0)
                            .color(TEXT)
                            .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(6.0);
                        status_dot(ui, "Connected", GOOD);
                    });
                });
            });

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 16,
                        right: 16,
                        top: 0,
                        bottom: 24,
                    })
                    .show(ui, |ui| {
                        self.hero(ui);
                        ui.add_space(10.0);
                        self.tabs(ui);
                        ui.add_space(12.0);
                        match self.tab {
                            Tab::Status => self.status_tab(ui),
                            Tab::Sound => self.sound_tab(ui),
                            Tab::System => {
                                if self.system_tab(ui) {
                                    leave = true;
                                }
                            }
                        }
                    });
            });
        self.power_off_dialog(ui.ctx());
        leave
    }

    fn hero(&mut self, ui: &mut Ui) {
        product_art(ui, self.form);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let count = if self.form == FormFactor::Earbuds {
                3.0
            } else {
                1.0
            };
            ui.add_space(
                ((ui.available_width() - 84.0 * count - 8.0 * (count - 1.0)) / 2.0 - 8.0).max(0.0),
            );
            match self.form {
                FormFactor::Headphones => battery_badge(ui, "", self.battery),
                FormFactor::Earbuds => {
                    battery_badge(ui, "L", self.battery);
                    battery_badge(ui, "R", self.battery_right);
                    battery_badge(ui, "Case", self.battery_case);
                }
            }
        });
        ui.add_space(2.0);
        ui.vertical_centered(|ui| {
            let mut parts = Vec::new();
            if let Some(c) = self.codec {
                parts.push(c.label().to_string());
            }
            if self.upsampling {
                parts.push("DSEE Extreme".into());
            }
            if !parts.is_empty() {
                ui.label(RichText::new(parts.join("  ·  ")).size(12.5).color(SUBTEXT));
            }
        });
    }

    fn tabs(&mut self, ui: &mut Ui) {
        let tabs = [
            (Tab::Status, "Status"),
            (Tab::Sound, "Sound"),
            (Tab::System, "System"),
        ];
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(w, 42.0), Sense::hover());
        let tab_w = w / tabs.len() as f32;
        let p = ui.painter().clone();
        p.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(1.0, DIVIDER),
        );
        let sel = tabs.iter().position(|(t, _)| *t == self.tab).unwrap_or(0);
        let x = ui
            .ctx()
            .animate_value_with_time(ui.scope_id().with("tab_ind"), sel as f32, 0.18);
        let ind = egui::Rect::from_min_size(
            egui::pos2(rect.left() + x * tab_w + tab_w * 0.2, rect.bottom() - 3.0),
            vec2(tab_w * 0.6, 3.0),
        );
        p.rect_filled(ind, CornerRadius::same(2), ACCENT);
        for (i, (tab, label)) in tabs.iter().enumerate() {
            let r = egui::Rect::from_min_size(
                egui::pos2(rect.left() + i as f32 * tab_w, rect.top()),
                vec2(tab_w, 42.0),
            );
            let resp = ui.interact(r, ui.scope_id().with(("tab", i)), Sense::click());
            if resp.clicked() {
                self.tab = *tab;
            }
            let color = if *tab == self.tab {
                TEXT
            } else if resp.hovered() {
                SUBTEXT
            } else {
                MUTED
            };
            p.text(
                r.center() - vec2(0.0, 2.0),
                egui::Align2::CENTER_CENTER,
                *label,
                egui::FontId::proportional(15.5),
                color,
            );
        }
    }

    // ---------------- Status ----------------

    fn status_tab(&mut self, ui: &mut Ui) {
        card(ui, None, |ui| {
            let mut on = self.ambient.mode != AmbientMode::Off;
            let title_changed = switch_row(ui, "Ambient Sound Control", None, &mut on);
            if title_changed {
                self.ambient.mode = if on {
                    self.last_on_mode
                } else {
                    AmbientMode::Off
                };
                self.send_ambient(false);
            }
            ui.add_space(8.0);

            let w = (ui.available_width() / 3.0).floor();
            let mut picked = None;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let m = self.ambient.mode;
                let nc = matches!(m, AmbientMode::NoiseCancelling | AmbientMode::WindReduction);
                if mode_button(ui, ModeIcon::NoiseCancelling, "Noise Canceling", nc, w).clicked() {
                    picked = Some(AmbientMode::NoiseCancelling);
                }
                if mode_button(
                    ui,
                    ModeIcon::Ambient,
                    "Ambient Sound",
                    m == AmbientMode::AmbientSound,
                    w,
                )
                .clicked()
                {
                    picked = Some(AmbientMode::AmbientSound);
                }
                if mode_button(ui, ModeIcon::Off, "Off", m == AmbientMode::Off, w).clicked() {
                    picked = Some(AmbientMode::Off);
                }
            });
            if let Some(mode) = picked
                && mode != self.ambient.mode
            {
                self.ambient.mode = mode;
                if mode != AmbientMode::Off {
                    self.last_on_mode = mode;
                }
                self.send_ambient(false);
            }

            if self.ambient.mode == AmbientMode::AmbientSound {
                ui.add_space(6.0);
                divider(ui);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Ambient sound level").size(14.5).color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(self.ambient.level.to_string())
                                .size(15.0)
                                .color(ACCENT)
                                .strong(),
                        );
                    });
                });
                let r = level_slider(
                    ui,
                    &mut self.ambient.level,
                    AMBIENT_LEVEL_MIN,
                    AMBIENT_LEVEL_MAX,
                    true,
                );
                if r.changed {
                    self.send_ambient(true);
                }
                if r.released {
                    self.send_ambient(false);
                }
                ui.add_space(4.0);
                if switch_row(
                    ui,
                    "Focus on Voice",
                    Some("Hear voices and announcements more clearly"),
                    &mut self.ambient.focus_on_voice,
                ) {
                    self.send_ambient(false);
                }
                if self.dialect.ambient == AmbientType::NoiseAdaptive && self.dialect.is_v2() {
                    divider(ui);
                    if switch_row(
                        ui,
                        "Auto Ambient Sound",
                        Some("Adjusts the ambient sound level to the noise around you"),
                        &mut self.ambient.auto_ambient,
                    ) {
                        self.send_ambient(false);
                    }
                    if self.ambient.auto_ambient {
                        ui.label(RichText::new("Sensitivity").size(14.0).color(SUBTEXT));
                        let labels: Vec<&str> = AutoAmbientSensitivity::ALL
                            .iter()
                            .map(|x| x.label())
                            .collect();
                        let mut idx = AutoAmbientSensitivity::ALL
                            .iter()
                            .position(|x| *x == self.ambient.auto_sensitivity)
                            .unwrap_or(0);
                        if segmented(ui, &labels, &mut idx, true) {
                            self.ambient.auto_sensitivity = AutoAmbientSensitivity::ALL[idx];
                            self.send_ambient(false);
                        }
                    }
                }
            } else if matches!(
                self.ambient.mode,
                AmbientMode::NoiseCancelling | AmbientMode::WindReduction
            ) && self.dialect.supports_wind_reduction()
            {
                ui.add_space(6.0);
                divider(ui);
                ui.add_space(8.0);
                let mut wind = self.ambient.mode == AmbientMode::WindReduction;
                if switch_row(
                    ui,
                    "Wind Noise Reduction",
                    Some("Reduces wind noise outdoors"),
                    &mut wind,
                ) {
                    self.ambient.mode = if wind {
                        AmbientMode::WindReduction
                    } else {
                        AmbientMode::NoiseCancelling
                    };
                    self.last_on_mode = self.ambient.mode;
                    self.send_ambient(false);
                }
            } else if self.ambient.mode == AmbientMode::Off {
                ui.label(
                    RichText::new("Noise canceling and ambient sound are turned off.")
                        .size(12.5)
                        .color(SUBTEXT),
                );
            }
        });

        ui.add_space(12.0);
        card(ui, None, |ui| {
            if switch_row(
                ui,
                "Speak-to-Chat",
                Some("Music pauses and ambient sound is let in when you start talking."),
                &mut self.speak_to_chat,
            ) {
                self.conn.send(Request::SetSpeakToChat(self.speak_to_chat));
            }
        });

        ui.add_space(12.0);
        let mut rows: Vec<(&str, String)> = Vec::new();
        if let Some(mode) = self.listening {
            let label = match mode {
                ListeningMode::Standard => "Standard".to_string(),
                ListeningMode::Bgm => format!("BGM · {}", self.room.label()),
                ListeningMode::Cinema => "Cinema".to_string(),
            };
            rows.push(("Listening Mode", label));
        }
        if let Some(sur) = self.surround {
            rows.push(("Surround (VPT)", sur.label().to_string()));
        }
        rows.push(("Equalizer", self.eq.preset.label().to_string()));
        card(ui, None, |ui| {
            for (i, (title, value)) in rows.iter().enumerate() {
                if i > 0 {
                    divider(ui);
                }
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(*title).size(15.5).color(TEXT));
                        ui.label(RichText::new(value).size(12.5).color(ACCENT));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Edit ›").color(SUBTEXT))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            self.tab = Tab::Sound;
                        }
                    });
                });
            }
        });

        self.playback_card(ui);

        ui.add_space(12.0);
        card(ui, Some("Connection"), |ui| {
            info_row(ui, "Codec", self.codec.map_or("—", |c| c.label()));
            info_row(ui, "Device", &self.info.name);
        });
    }

    // ---------------- Sound ----------------

    /// Switch listening mode the way the phone app does: turning BGM or
    /// Cinema off returns to Standard.
    fn set_listening(&mut self, mode: ListeningMode) {
        let current = self.listening.unwrap_or_default();
        if mode == current {
            return;
        }
        let request = match mode {
            ListeningMode::Bgm => Request::SetBgmMode {
                on: true,
                room: self.room,
            },
            ListeningMode::Cinema => Request::SetCinemaMode(true),
            ListeningMode::Standard if current == ListeningMode::Bgm => Request::SetBgmMode {
                on: false,
                room: self.room,
            },
            ListeningMode::Standard => Request::SetCinemaMode(false),
        };
        self.listening = Some(mode);
        self.conn.send(request);
    }

    fn listening_card(&mut self, ui: &mut Ui) {
        let Some(current) = self.listening else {
            return;
        };
        card(ui, Some("Listening Mode"), |ui| {
            let w = (ui.available_width() / 3.0).floor();
            let mut picked = None;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (mode, icon, label) in [
                    (ListeningMode::Standard, ModeIcon::Standard, "Standard"),
                    (ListeningMode::Bgm, ModeIcon::Bgm, "BGM"),
                    (ListeningMode::Cinema, ModeIcon::Cinema, "Cinema"),
                ] {
                    if mode_button(ui, icon, label, current == mode, w).clicked() {
                        picked = Some(mode);
                    }
                }
            });
            if let Some(mode) = picked {
                self.set_listening(mode);
            }
            let description = match self.listening.unwrap_or_default() {
                ListeningMode::Standard => "Plays music as the artist intended.",
                ListeningMode::Bgm => {
                    "Sounds as if the music is playing from speakers in a room, so it stays in the background."
                }
                ListeningMode::Cinema => "Spatial, theater-like sound for movies and videos.",
            };
            ui.label(RichText::new(description).size(12.5).color(SUBTEXT));
            if self.listening == Some(ListeningMode::Bgm) {
                ui.add_space(6.0);
                divider(ui);
                ui.add_space(6.0);
                ui.label(RichText::new("Room size").size(14.0).color(SUBTEXT));
                let labels: Vec<&str> = RoomSize::ALL.iter().map(|r| r.label()).collect();
                let mut idx = RoomSize::ALL
                    .iter()
                    .position(|r| *r == self.room)
                    .unwrap_or(0);
                if segmented(ui, &labels, &mut idx, true) {
                    self.room = RoomSize::ALL[idx];
                    self.conn.send(Request::SetBgmMode {
                        on: true,
                        room: self.room,
                    });
                }
            }
        });
        ui.add_space(12.0);
    }

    fn surround_card(&mut self, ui: &mut Ui) {
        let Some(current) = self.surround else { return };
        card(ui, Some("Surround (VPT)"), |ui| {
            let labels: Vec<&str> = Surround::ALL.iter().map(|s| s.label()).collect();
            let selected = Surround::ALL.iter().position(|s| *s == current);
            if let Some(i) = chips(ui, "surround", &labels, selected) {
                self.surround = Some(Surround::ALL[i]);
                self.conn.send(Request::SetSurround(Surround::ALL[i]));
            }
        });
        ui.add_space(12.0);
    }

    fn sound_tab(&mut self, ui: &mut Ui) {
        self.listening_card(ui);
        self.surround_card(ui);
        card(ui, Some("Equalizer"), |ui| {
            let layout = self.eq.layout;
            let available = self.eq_available != Some(false);
            if !available {
                ui.label(
                    RichText::new("The equalizer can't be used in the current Listening Mode. Switch Listening Mode to Standard to use it.")
                        .size(12.5)
                        .color(ACCENT),
                );
                ui.add_space(4.0);
            }
            let presets = layout.presets();
            let labels: Vec<&str> = presets.iter().map(|p| p.label()).collect();
            let selected = presets.iter().position(|p| *p == self.eq.preset);
            if let Some(i) = chips(ui, "eq_presets", &labels, selected).filter(|_| available) {
                let preset = presets[i];
                self.eq.preset = preset;
                if !preset.is_editable() && layout == EqLayout::ClearBassFiveBand {
                    self.eq = Equalizer::classic(preset, preset.default_bands());
                }
                // Selecting a preset only; the headset reports the preset's curve.
                self.conn.send(Request::SetEqualizerPreset(preset));
            }
            ui.add_space(8.0);
            let editable = available && self.eq.preset.is_editable();
            let r = eq_graph(
                ui,
                self.eq.values_mut(),
                layout.labels(),
                layout.range(),
                layout == EqLayout::ClearBassFiveBand,
                editable,
            );
            if r.released || r.changed {
                self.conn.send(Request::SetEqualizerBands(self.eq));
            }
            if !editable {
                let hint = if layout == EqLayout::ClearBassFiveBand {
                    "Select Manual or Custom to adjust the bands. CLEAR BASS boosts low frequencies without distortion."
                } else {
                    "Select Manual or Custom to adjust the bands."
                };
                ui.label(RichText::new(hint).size(12.0).color(SUBTEXT));
            } else if ui
                .add(egui::Button::new(RichText::new("Reset").color(ACCENT)).frame(false))
                .clicked()
            {
                self.eq.values_mut().fill(0);
                self.conn.send(Request::SetEqualizerBands(self.eq));
            }
        });

        ui.add_space(12.0);
        card(ui, None, |ui| {
            let available = self.upsampling_available != Some(false);
            ui.add_enabled_ui(available, |ui| {
                if switch_row(
                    ui,
                    "DSEE Extreme",
                    Some(
                        "Upscales compressed digital music files in real time. (Battery life is shortened.)",
                    ),
                    &mut self.upsampling,
                ) {
                    self.conn.send(Request::SetAudioUpsampling(self.upsampling));
                }
            });
            if !available {
                ui.label(
                    RichText::new("Not available in the current Listening Mode. Switch Listening Mode to Standard to use it.")
                        .size(12.5)
                        .color(ACCENT),
                );
            }
        });

        if let Some(current) = self.connection {
            ui.add_space(12.0);
            card(ui, Some("Bluetooth Connection Quality"), |ui| {
                let labels = ["Sound Quality", "Stable Connection"];
                let mut idx = usize::from(current == ConnectionPriority::StableConnection);
                if segmented(ui, &labels, &mut idx, true) {
                    let prio = ConnectionPriority::CHOICES[idx];
                    self.connection = Some(prio);
                    self.conn.send(Request::SetConnectionPriority(prio));
                }
                ui.label(
                    RichText::new(format!(
                        "{}. Changing this briefly interrupts the audio.",
                        self.connection.unwrap_or_default().label()
                    ))
                    .size(12.0)
                    .color(SUBTEXT),
                );
            });
        }
    }

    fn playback_card(&mut self, ui: &mut Ui) {
        if self.playback.is_none() && self.volume.is_none() {
            return;
        }
        ui.add_space(12.0);
        card(ui, Some("Playback"), |ui| {
            if let Some(state) = self.playback {
                let playing = state == PlaybackState::Playing;
                let w = ui.available_width();
                ui.horizontal(|ui| {
                    ui.add_space((w - 3.0 * 64.0 - 2.0 * 16.0) / 2.0);
                    ui.spacing_mut().item_spacing.x = 16.0;
                    if round_icon_button(ui, ModeIcon::Previous, 56.0, false).clicked() {
                        self.conn.send(Request::Playback(PlaybackControl::Previous));
                    }
                    let icon = if playing {
                        ModeIcon::Pause
                    } else {
                        ModeIcon::Play
                    };
                    if round_icon_button(ui, icon, 64.0, true).clicked() {
                        let control = if playing {
                            PlaybackControl::Pause
                        } else {
                            PlaybackControl::Play
                        };
                        self.playback = Some(if playing {
                            PlaybackState::Paused
                        } else {
                            PlaybackState::Playing
                        });
                        self.conn.send(Request::Playback(control));
                    }
                    if round_icon_button(ui, ModeIcon::Next, 56.0, false).clicked() {
                        self.conn.send(Request::Playback(PlaybackControl::Next));
                    }
                });
            }
            if let Some(mut volume) = self.volume {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Volume").size(14.5).color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(volume.to_string())
                                .size(15.0)
                                .color(ACCENT)
                                .strong(),
                        );
                    });
                });
                let r = level_slider(ui, &mut volume, 0, VOLUME_MAX, true);
                if r.changed || r.released {
                    self.volume = Some(volume);
                    self.conn.send(Request::SetVolume(volume));
                }
            }
        });
    }

    // ---------------- System ----------------

    /// Returns `true` if the user disconnected.
    fn system_tab(&mut self, ui: &mut Ui) -> bool {
        let mut leave = false;
        card(ui, Some("Speak-to-Chat settings"), |ui| {
            ui.add_enabled_ui(self.speak_to_chat, |ui| {
                let mut changed = false;
                ui.label(
                    RichText::new("Voice detection sensitivity")
                        .size(14.0)
                        .color(SUBTEXT),
                );
                let mut idx = StcSensitivity::ALL
                    .iter()
                    .position(|s| *s == self.stc.sensitivity)
                    .unwrap_or(0);
                let labels: Vec<&str> = StcSensitivity::ALL.iter().map(|s| s.label()).collect();
                if segmented(ui, &labels, &mut idx, self.speak_to_chat) {
                    self.stc.sensitivity = StcSensitivity::ALL[idx];
                    changed = true;
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Mode duration").size(14.0).color(SUBTEXT));
                egui::ComboBox::from_id_salt("stc_timeout")
                    .width(ui.available_width())
                    .selected_text(self.stc.timeout.label())
                    .show_ui(ui, |ui| {
                        for t in StcTimeout::ALL {
                            if ui
                                .selectable_value(&mut self.stc.timeout, t, t.label())
                                .changed()
                            {
                                changed = true;
                            }
                        }
                    });
                ui.add_space(4.0);
                if !self.dialect.is_v2()
                    && switch_row(ui, "Voice focus", None, &mut self.stc.focus_on_voice)
                {
                    changed = true;
                }
                if changed {
                    self.conn.send(Request::SetSpeakToChatConfig(self.stc));
                }
            });
            if !self.speak_to_chat {
                ui.label(
                    RichText::new("Turn on Speak-to-Chat in the Status tab to change these.")
                        .size(12.0)
                        .color(MUTED),
                );
            }
        });

        ui.add_space(12.0);
        card(ui, None, |ui| {
            if self.dialect.supports_touch_sensor() {
                if switch_row(
                    ui,
                    "Touch sensor control panel",
                    Some("Control playback and calls by touch"),
                    &mut self.touch_panel,
                ) {
                    self.conn.send(Request::SetTouchSensor(self.touch_panel));
                }
                divider(ui);
            }
            let title = if self.form == FormFactor::Earbuds {
                "Pause music when earbuds are removed"
            } else {
                "Pause when headphones are taken off"
            };
            if switch_row(ui, title, None, &mut self.pause_when_off) {
                self.conn
                    .send(Request::SetPauseWhenTakenOff(self.pause_when_off));
            }
            if let Some(current) = self.nc_amb_button {
                divider(ui);
                ui.label(RichText::new("NC/AMB button").size(15.5).color(TEXT));
                let mut chosen = current;
                egui::ComboBox::from_id_salt("nc_amb_button")
                    .width(ui.available_width())
                    .selected_text(chosen.label())
                    .show_ui(ui, |ui| {
                        for b in NcAmbButton::ALL {
                            ui.selectable_value(&mut chosen, b, b.label());
                        }
                    });
                if chosen != current {
                    self.nc_amb_button = Some(chosen);
                    self.conn.send(Request::SetNcAmbButton(chosen));
                }
            }
            divider(ui);
            ui.label(RichText::new("Automatic power off").size(15.5).color(TEXT));
            let before = self.auto_power_off;
            egui::ComboBox::from_id_salt("apo")
                .width(ui.available_width())
                .selected_text(self.auto_power_off.label())
                .show_ui(ui, |ui| {
                    for &a in self.dialect.auto_power_off_options() {
                        ui.selectable_value(&mut self.auto_power_off, a, a.label());
                    }
                });
            if before != self.auto_power_off {
                self.conn
                    .send(Request::SetAutoPowerOff(self.auto_power_off));
            }
        });

        if let Some(mut on) = self.voice_guidance {
            ui.add_space(12.0);
            card(ui, None, |ui| {
                if switch_row(
                    ui,
                    "Voice guidance",
                    Some("Spoken notices for power, battery and mode changes"),
                    &mut on,
                ) {
                    self.voice_guidance = Some(on);
                    self.conn.send(Request::SetVoiceGuidance(on));
                }
                if let (true, Some(vol)) = (on, self.voice_volume) {
                    ui.label(
                        RichText::new("Voice guidance volume")
                            .size(14.0)
                            .color(SUBTEXT),
                    );
                    let labels = ["-2", "-1", "0", "+1", "+2"];
                    let (lo, _) = VOICE_VOLUME_RANGE;
                    let mut idx = usize::try_from(i16::from(vol) - i16::from(lo))
                        .unwrap_or(2)
                        .min(4);
                    if segmented(ui, &labels, &mut idx, true) {
                        let v = i8::try_from(idx).unwrap_or(2) + lo;
                        self.voice_volume = Some(v);
                        self.conn.send(Request::SetVoiceGuidanceVolume(v));
                    }
                }
            });
        }

        ui.add_space(12.0);
        card(ui, Some("Device information"), |ui| {
            info_row(ui, "Model", &self.info.name);
            info_row(ui, "Firmware", self.firmware.as_deref().unwrap_or("—"));
            info_row(
                ui,
                "Protocol",
                if self.dialect.is_v2() { "V2" } else { "V1" },
            );
            info_row(ui, "Address", &self.info.address);
            info_row(ui, "Codec", self.codec.map_or("—", |c| c.label()));
        });

        ui.add_space(12.0);
        if wide_button(ui, "Disconnect", TEXT).clicked() {
            self.conn.disconnect();
            leave = true;
        }
        ui.add_space(6.0);
        if wide_button(ui, "Power Off", DANGER).clicked() {
            self.confirm_power_off = true;
        }
        leave
    }

    fn power_off_dialog(&mut self, ctx: &egui::Context) {
        if !self.confirm_power_off {
            return;
        }
        egui::Window::new("power_off")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(300.0, 0.0))
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Turn off the headset?")
                        .size(17.0)
                        .color(TEXT)
                        .strong(),
                );
                ui.label(
                    RichText::new("The connection will end.")
                        .size(13.5)
                        .color(SUBTEXT),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.confirm_power_off = false;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Power Off").color(Color32::WHITE))
                                    .fill(DANGER),
                            )
                            .clicked()
                        {
                            self.conn.send(Request::PowerOff);
                            self.confirm_power_off = false;
                        }
                    });
                });
            });
    }
}
