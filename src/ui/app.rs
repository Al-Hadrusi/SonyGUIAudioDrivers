//! Application screens: device picker and the per-device control screen with
//! the "Status", "Sound" and "System" tabs.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, CornerRadius, Layout, RichText, Sense, Ui, vec2};

use super::theme::{self, *};
use super::widgets::*;
use sony_audio_control::protocol::*;
use sony_audio_control::transport::{self, DeviceInfo, FormFactor};
use sony_audio_control::worker::{Connection, Event};

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

pub struct App {
    screen: Screen,
    devices: Vec<DeviceInfo>,
    device_note: Option<String>,
    scanning: Option<Receiver<(Vec<DeviceInfo>, Option<String>)>>,
    toast: Option<(String, Instant)>,
    start_tab: Tab,
    show_demo: bool,
}

/// Command-line options.
#[derive(Default)]
pub struct LaunchOptions {
    /// Show the simulated demo devices in the device list (`--demo`).
    pub show_demo: bool,
    /// Connect straight to a simulated device of this model (`--demo <MODEL>`).
    pub demo: Option<String>,
    pub tab: Tab,
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
            start_tab: opts.tab,
            show_demo: opts.show_demo || opts.demo.is_some(),
        };
        app.rescan(&cc.egui_ctx);
        if let Some(model) = opts.demo {
            let device = DeviceInfo {
                address: format!("demo:{model}"),
                name: model,
                connected: false,
            };
            app.connect(&cc.egui_ctx, device);
        }
        app
    }

    fn rescan(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let include_demo = self.show_demo;
        std::thread::spawn(move || {
            let _ = tx.send(transport::list_devices(include_demo));
            ctx.request_repaint();
        });
        self.scanning = Some(rx);
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn connect(&mut self, ctx: &egui::Context, device: DeviceInfo) {
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
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(rx) = &self.scanning
            && let Ok((devices, note)) = rx.try_recv()
        {
            self.devices = devices;
            self.device_note = note;
            self.scanning = None;
        }
        self.process_events();

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
                        action = devices_screen(
                            &mut child,
                            &self.devices,
                            self.device_note.as_deref(),
                            self.scanning.is_some(),
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
                    Action::Rescan => self.rescan(&ctx),
                    Action::Connect(d) => self.connect(&ctx, d),
                    Action::Cancel => {
                        self.screen = Screen::Devices;
                        self.rescan(&ctx);
                    }
                }
            });

        self.show_toast(&ctx);
        // Keep animations and the simulated device ticking.
        ctx.request_repaint_after(Duration::from_millis(250));
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
            if device_row(ui, d) {
                action = Action::Connect(d.clone());
            }
        }
        if !demo.is_empty() {
            ui.add_space(8.0);
            section_label(ui, "Demo (simulated)");
        }
        for d in demo {
            if device_row(ui, d) {
                action = Action::Connect(d.clone());
            }
        }
    });
    action
}

fn device_row(ui: &mut Ui, d: &DeviceInfo) -> bool {
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
                    let sub = if d.is_demo() {
                        "Simulated device".to_string()
                    } else if d.connected {
                        format!("Connected · {}", d.address)
                    } else {
                        d.address.clone()
                    };
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
            speak_to_chat: false,
            stc: SpeakToChatConfig::default(),
            touch_panel: true,
            pause_when_off: true,
            auto_power_off: AutoPowerOff::Off,
            confirm_power_off: false,
            listening: None,
            room: RoomSize::Small,
            surround: None,
        }
    }

    fn apply(&mut self, n: Notification) {
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
            let labels: Vec<&str> = EqPreset::ALL.iter().map(|p| p.label()).collect();
            let selected = EqPreset::ALL.iter().position(|p| *p == self.eq.preset);
            if let Some(i) = chips(ui, "eq_presets", &labels, selected) {
                let preset = EqPreset::ALL[i];
                self.eq.preset = preset;
                if !preset.is_editable() {
                    self.eq.bands = preset.default_bands();
                }
                // Selecting a preset only (the headset restores its own bands).
                self.conn.send(Request::SetEqualizerPreset(preset));
            }
            ui.add_space(8.0);
            let editable = self.eq.preset.is_editable();
            let r = eq_graph(ui, &mut self.eq.bands, editable);
            if r.released || r.changed {
                self.conn.send(Request::SetEqualizerBands(self.eq));
            }
            if !editable {
                ui.label(
                    RichText::new("Select Manual or Custom to adjust the bands. CLEAR BASS boosts low frequencies without distortion.")
                        .size(12.0)
                        .color(SUBTEXT),
                );
            } else if ui
                .add(egui::Button::new(RichText::new("Reset").color(ACCENT)).frame(false))
                .clicked()
            {
                self.eq.bands = [0; 6];
                self.conn.send(Request::SetEqualizerBands(self.eq));
            }
        });

        ui.add_space(12.0);
        card(ui, None, |ui| {
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
