//! Colours and global style, modelled on the dark theme of the mobile
//! companion app: black background, raised charcoal cards, amber accent.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x0B, 0x0B, 0x0D);
pub const CARD: Color32 = Color32::from_rgb(0x1C, 0x1C, 0x1F);
pub const CARD_HI: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x2E);
pub const DIVIDER: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x33);
pub const TEXT: Color32 = Color32::from_rgb(0xF4, 0xF4, 0xF6);
pub const SUBTEXT: Color32 = Color32::from_rgb(0x9C, 0x9C, 0xA3);
pub const MUTED: Color32 = Color32::from_rgb(0x5E, 0x5E, 0x66);
pub const ACCENT: Color32 = Color32::from_rgb(0xF5, 0xA3, 0x1B);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x5A, 0x3E, 0x10);
pub const GOOD: Color32 = Color32::from_rgb(0x4C, 0xD9, 0x64);
pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x45, 0x3A);

pub const CARD_RADIUS: u8 = 16;
pub const PHONE_WIDTH: f32 = 412.0;
pub const PHONE_HEIGHT: f32 = 880.0;

pub fn apply(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = CARD;
    visuals.extreme_bg_color = CARD_HI;
    visuals.faint_bg_color = CARD;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = ACCENT_DIM;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.window_corner_radius = CornerRadius::same(CARD_RADIUS);
    visuals.menu_corner_radius = CornerRadius::same(12);
    visuals.window_stroke = Stroke::new(1.0, DIVIDER);
    for w in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(10);
        w.bg_stroke = Stroke::NONE;
    }
    visuals.widgets.inactive.weak_bg_fill = CARD_HI;
    visuals.widgets.inactive.bg_fill = CARD_HI;
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x34, 0x34, 0x39);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0x3C, 0x3C, 0x42);
    visuals.widgets.open.weak_bg_fill = Color32::from_rgb(0x34, 0x34, 0x39);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, DIVIDER);

    ctx.all_styles_mut(|style| {
        style.visuals = visuals.clone();
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(24.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(15.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(14.0, 8.0);
        style.spacing.interact_size.y = 32.0;
    });
}
