//! Custom mobile-style widgets: cards, switches, segmented controls, sliders,
//! the equalizer graph, battery indicators and product illustrations.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Mesh, Pos2, Rect, Response, RichText, Sense,
    Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};

use super::theme::*;
use headphones_control::protocol::{Battery, EQ_BAND_LABELS, EQ_MAX, EQ_MIN};
use headphones_control::transport::FormFactor;

/// A rounded card with an optional title.
pub fn card<R>(ui: &mut Ui, title: Option<&str>, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(t) = title {
                ui.label(RichText::new(t).size(17.0).color(TEXT).strong());
                ui.add_space(4.0);
            }
            add(ui)
        })
        .inner
}

pub fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text.to_uppercase()).size(12.0).color(SUBTEXT));
}

pub fn divider(ui: &mut Ui) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0, DIVIDER);
}

/// iOS / Android style on-off switch.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let size = vec2(50.0, 30.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool(response.id, *on);
    let track = lerp_color(CARD_HI.gamma_multiply(1.6), ACCENT, t);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(15), track);
    let x = egui::lerp(rect.left() + 15.0..=rect.right() - 15.0, t);
    p.circle_filled(
        pos2(x, rect.center().y + 1.0),
        12.5,
        Color32::from_black_alpha(60),
    );
    p.circle_filled(pos2(x, rect.center().y), 12.5, Color32::WHITE);
    response
}

/// A settings row: title, optional subtitle, and a switch on the right.
pub fn switch_row(ui: &mut Ui, title: &str, subtitle: Option<&str>, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width() - 62.0);
            ui.label(RichText::new(title).size(15.5).color(TEXT));
            if let Some(s) = subtitle {
                ui.label(RichText::new(s).size(12.5).color(SUBTEXT));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = switch(ui, on).changed();
        });
    });
    changed
}

/// Pill-shaped segmented control. Returns `true` when the selection changed.
pub fn segmented(ui: &mut Ui, options: &[&str], selected: &mut usize, enabled: bool) -> bool {
    let w = ui.available_width();
    let h = 38.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::same(19), CARD_HI);
    let n = options.len().max(1) as f32;
    let seg_w = (w - 6.0) / n;
    let anim = ui.ctx().animate_value_with_time(
        ui.scope_id().with("seg").with(rect.min.x as i32),
        *selected as f32,
        0.15,
    );
    let sel_rect = Rect::from_min_size(
        pos2(rect.left() + 3.0 + anim * seg_w, rect.top() + 3.0),
        vec2(seg_w, h - 6.0),
    );
    let sel_fill = if enabled { ACCENT } else { MUTED };
    p.rect_filled(sel_rect, CornerRadius::same(16), sel_fill);
    let mut changed = false;
    for (i, label) in options.iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(rect.left() + 3.0 + i as f32 * seg_w, rect.top()),
            vec2(seg_w, h),
        );
        let resp = ui.interact(
            r,
            ui.scope_id().with(("segment", i, rect.min.y as i32)),
            Sense::click(),
        );
        if resp.clicked() && enabled && *selected != i {
            *selected = i;
            changed = true;
        }
        let color = if i == *selected && enabled {
            Color32::BLACK
        } else if enabled {
            TEXT
        } else {
            MUTED
        };
        p.text(
            r.center(),
            Align2::CENTER_CENTER,
            *label,
            FontId::proportional(14.0),
            color,
        );
    }
    changed
}

/// Result of a slider interaction.
#[derive(Default, Clone, Copy)]
pub struct SliderResult {
    pub changed: bool,
    pub released: bool,
}

/// Horizontal level slider with a filled accent track.
pub fn level_slider(ui: &mut Ui, value: &mut u8, min: u8, max: u8, enabled: bool) -> SliderResult {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(
        vec2(w, 32.0),
        if enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    );
    let track = Rect::from_center_size(rect.center(), vec2(w - 24.0, 4.0));
    let mut out = SliderResult::default();
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
        let v = (f32::from(min) + t * f32::from(max - min)).round() as u8;
        if v != *value {
            *value = v;
            out.changed = true;
        }
    }
    out.released = resp.drag_stopped() || resp.clicked();
    let t = f32::from(value.saturating_sub(min)) / f32::from((max - min).max(1));
    let p = ui.painter();
    let x = egui::lerp(track.left()..=track.right(), t);
    let active = if enabled { ACCENT } else { MUTED };
    p.rect_filled(track, CornerRadius::same(2), CARD_HI.gamma_multiply(1.8));
    p.rect_filled(
        Rect::from_min_max(track.min, pos2(x, track.max.y)),
        CornerRadius::same(2),
        active,
    );
    let r = if resp.dragged() { 12.0 } else { 10.0 };
    p.circle_filled(
        pos2(x, rect.center().y),
        r,
        if enabled { Color32::WHITE } else { SUBTEXT },
    );
    out
}

/// Big round mode buttons (Noise Canceling / Ambient Sound / Off).
pub fn mode_button(
    ui: &mut Ui,
    icon: ModeIcon,
    label: &str,
    selected: bool,
    width: f32,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 96.0), Sense::click());
    let p = ui.painter();
    let c = pos2(rect.center().x, rect.top() + 32.0);
    let t = ui.ctx().animate_bool(resp.id, selected);
    let fill = lerp_color(CARD_HI, ACCENT, t);
    p.circle_filled(c, 28.0, fill);
    if resp.hovered() && !selected {
        p.circle_stroke(c, 28.0, Stroke::new(1.5, SUBTEXT));
    }
    let fg = lerp_color(TEXT, Color32::BLACK, t);
    draw_mode_icon(p, icon, c, fg);
    p.text(
        pos2(rect.center().x, rect.top() + 78.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.5),
        if selected { TEXT } else { SUBTEXT },
    );
    resp
}

#[derive(Clone, Copy)]
pub enum ModeIcon {
    NoiseCancelling,
    Ambient,
    Off,
    /// Listening mode "Standard": a music note.
    Standard,
    /// Listening mode "BGM": a speaker in a room.
    Bgm,
    /// Listening mode "Cinema": a screen.
    Cinema,
}

fn draw_mode_icon(p: &egui::Painter, icon: ModeIcon, c: Pos2, color: Color32) {
    let s = Stroke::new(2.2, color);
    match icon {
        ModeIcon::Standard => {
            // Beamed eighth notes.
            p.circle_filled(c + vec2(-7.0, 8.0), 4.5, color);
            p.circle_filled(c + vec2(7.0, 5.0), 4.5, color);
            p.line_segment([c + vec2(-3.0, 8.0), c + vec2(-3.0, -10.0)], s);
            p.line_segment([c + vec2(11.0, 5.0), c + vec2(11.0, -13.0)], s);
            p.line_segment(
                [c + vec2(-3.0, -10.0), c + vec2(11.0, -13.0)],
                Stroke::new(3.5, color),
            );
        }
        ModeIcon::Bgm => {
            // Speaker cabinet with two drivers, and a floor line.
            let cab = Rect::from_center_size(c + vec2(0.0, -1.0), vec2(16.0, 24.0));
            p.rect_stroke(cab, CornerRadius::same(3), s, StrokeKind::Middle);
            p.circle_filled(cab.center() + vec2(0.0, -6.0), 2.5, color);
            p.circle_stroke(cab.center() + vec2(0.0, 4.0), 4.5, Stroke::new(1.8, color));
            p.line_segment(
                [c + vec2(-15.0, 14.0), c + vec2(15.0, 14.0)],
                Stroke::new(1.6, color),
            );
        }
        ModeIcon::Cinema => {
            // Wide screen on a stand, with a play triangle.
            let screen = Rect::from_center_size(c + vec2(0.0, -3.0), vec2(30.0, 19.0));
            p.rect_stroke(screen, CornerRadius::same(2), s, StrokeKind::Middle);
            let t = screen.center();
            p.add(Shape::convex_polygon(
                vec![
                    t + vec2(-3.5, -5.0),
                    t + vec2(5.0, 0.0),
                    t + vec2(-3.5, 5.0),
                ],
                color,
                Stroke::NONE,
            ));
            p.line_segment([c + vec2(-7.0, 12.0), c + vec2(7.0, 12.0)], s);
        }
        ModeIcon::NoiseCancelling => {
            // Head-band + cups, with a "blocked wave" bar.
            small_headphones(p, c + vec2(0.0, 1.0), 0.9, color);
            p.line_segment(
                [c + vec2(-13.0, 13.0), c + vec2(13.0, -13.0)],
                Stroke::new(2.4, color),
            );
        }
        ModeIcon::Ambient => {
            // Ear with sound waves.
            p.circle_filled(c + vec2(-6.0, 0.0), 3.0, color);
            for (i, r) in [8.0f32, 13.0, 18.0].into_iter().enumerate() {
                let pts: Vec<Pos2> = (0..=16)
                    .map(|k| {
                        let a = -0.8 + 1.6 * k as f32 / 16.0;
                        c + vec2(-6.0 + r * a.cos(), r * a.sin())
                    })
                    .collect();
                let alpha = 1.0 - i as f32 * 0.22;
                p.add(Shape::line(
                    pts,
                    Stroke::new(2.2, color.gamma_multiply(alpha)),
                ));
            }
        }
        ModeIcon::Off => {
            // Power glyph.
            let pts: Vec<Pos2> = (0..=28)
                .map(|k| {
                    let a = -std::f32::consts::FRAC_PI_2
                        + 0.6
                        + (std::f32::consts::TAU - 1.2) * k as f32 / 28.0;
                    c + vec2(11.0 * a.cos(), 11.0 * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, s));
            p.line_segment([c + vec2(0.0, -14.0), c + vec2(0.0, -2.0)], s);
        }
    }
}

fn small_headphones(p: &egui::Painter, c: Pos2, scale: f32, color: Color32) {
    let r = 12.0 * scale;
    let pts: Vec<Pos2> = (0..=20)
        .map(|k| {
            let a = std::f32::consts::PI + std::f32::consts::PI * k as f32 / 20.0;
            c + vec2(r * a.cos(), r * a.sin() + 2.0)
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(2.2, color)));
    for dx in [-r, r] {
        let cup =
            Rect::from_center_size(c + vec2(dx, 6.0 * scale), vec2(7.0 * scale, 11.0 * scale));
        p.rect_filled(cup, CornerRadius::same(3), color);
    }
}

/// Battery glyph with percentage label, painted into a fixed 84x64 cell.
pub fn battery_badge(ui: &mut Ui, label: &str, battery: Option<Battery>) {
    let (cell, _) = ui.allocate_exact_size(vec2(84.0, 64.0), Sense::hover());
    let p = ui.painter();
    let body = Rect::from_center_size(
        pos2(cell.center().x - 2.0, cell.top() + 10.0),
        vec2(36.0, 18.0),
    );
    p.rect_stroke(
        body,
        CornerRadius::same(4),
        Stroke::new(1.6, SUBTEXT),
        StrokeKind::Inside,
    );
    p.rect_filled(
        Rect::from_min_size(
            pos2(body.right() + 1.0, body.center().y - 4.0),
            vec2(3.0, 8.0),
        ),
        CornerRadius::same(1),
        SUBTEXT,
    );
    if let Some(b) = battery {
        let frac = f32::from(b.level) / 100.0;
        let color = if b.level <= 15 {
            DANGER
        } else if b.charging {
            GOOD
        } else {
            TEXT
        };
        let inner = body.shrink(3.0);
        p.rect_filled(
            Rect::from_min_size(inner.min, vec2(inner.width() * frac, inner.height())),
            CornerRadius::same(2),
            color,
        );
        if b.charging {
            let c = body.center();
            let bolt = [
                c + vec2(1.5, -6.0),
                c + vec2(-3.5, 1.0),
                c + vec2(0.5, 1.0),
                c + vec2(-1.5, 6.0),
                c + vec2(3.5, -1.0),
                c + vec2(-0.5, -1.0),
            ];
            p.add(Shape::convex_polygon(
                vec![bolt[0], bolt[1], bolt[2]],
                Color32::BLACK,
                Stroke::NONE,
            ));
            p.add(Shape::convex_polygon(
                vec![bolt[3], bolt[4], bolt[5]],
                Color32::BLACK,
                Stroke::NONE,
            ));
            p.add(Shape::convex_polygon(
                vec![bolt[5], bolt[2], bolt[4], bolt[0]],
                Color32::BLACK,
                Stroke::NONE,
            ));
        }
    }
    let text = battery.map_or_else(|| "--%".to_string(), |b| format!("{}%", b.level));
    p.text(
        pos2(cell.center().x, cell.top() + 36.0),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(16.0),
        TEXT,
    );
    if !label.is_empty() {
        p.text(
            pos2(cell.center().x, cell.top() + 56.0),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(11.5),
            SUBTEXT,
        );
    }
}

/// Small coloured status dot followed by a label.
pub fn status_dot(ui: &mut Ui, text: &str, color: Color32) {
    ui.label(RichText::new(text).size(12.5).color(color));
    let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 4.0, color);
}

/// Product illustration drawn with vector shapes (no bundled artwork).
pub fn product_art(ui: &mut Ui, form: FormFactor) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 190.0), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    // Soft spotlight
    for (i, a) in [(0, 14u8), (1, 10), (2, 6)] {
        p.circle_filled(
            c + vec2(0.0, 10.0),
            80.0 + 18.0 * i as f32,
            Color32::from_white_alpha(a / 2),
        );
    }
    let body = Color32::from_rgb(0x2C, 0x2C, 0x30);
    let edge = Color32::from_rgb(0x55, 0x55, 0x5C);
    let hi = Color32::from_rgb(0x7A, 0x7A, 0x82);
    match form {
        FormFactor::Headphones => {
            // Headband
            let band: Vec<Pos2> = (0..=48)
                .map(|k| {
                    let a = std::f32::consts::PI + std::f32::consts::PI * k as f32 / 48.0;
                    c + vec2(70.0 * a.cos(), 78.0 * a.sin() + 20.0)
                })
                .collect();
            p.add(Shape::line(band.clone(), Stroke::new(14.0, body)));
            p.add(Shape::line(band, Stroke::new(1.5, edge)));
            // Sliders
            for dx in [-70.0f32, 70.0] {
                p.rect_filled(
                    Rect::from_center_size(c + vec2(dx, 28.0), vec2(8.0, 26.0)),
                    CornerRadius::same(3),
                    hi,
                );
            }
            // Ear cups
            for dx in [-66.0f32, 66.0] {
                let cup = Rect::from_center_size(c + vec2(dx, 62.0), vec2(46.0, 74.0));
                p.rect_filled(cup, CornerRadius::same(22), body);
                p.rect_stroke(
                    cup,
                    CornerRadius::same(22),
                    Stroke::new(1.5, edge),
                    StrokeKind::Inside,
                );
                let pad = cup
                    .shrink2(vec2(10.0, 12.0))
                    .translate(vec2(if dx < 0.0 { 6.0 } else { -6.0 }, 0.0));
                p.rect_filled(
                    pad,
                    CornerRadius::same(14),
                    Color32::from_rgb(0x1A, 0x1A, 0x1D),
                );
                p.circle_filled(
                    cup.center() + vec2(if dx < 0.0 { -14.0 } else { 14.0 }, -24.0),
                    2.0,
                    ACCENT,
                );
            }
        }
        FormFactor::Earbuds => {
            // Case
            let case = Rect::from_center_size(c + vec2(0.0, 52.0), vec2(136.0, 64.0));
            p.rect_filled(case, CornerRadius::same(30), body);
            p.rect_stroke(
                case,
                CornerRadius::same(30),
                Stroke::new(1.5, edge),
                StrokeKind::Inside,
            );
            p.line_segment(
                [
                    pos2(case.left() + 8.0, case.top() + 22.0),
                    pos2(case.right() - 8.0, case.top() + 22.0),
                ],
                Stroke::new(1.0, edge),
            );
            p.circle_filled(pos2(case.center().x, case.bottom() - 12.0), 2.5, GOOD);
            // Buds
            for dx in [-38.0f32, 38.0] {
                let bc = c + vec2(dx * 1.1, -36.0);
                p.circle_filled(bc, 30.0, body);
                p.circle_stroke(bc, 30.0, Stroke::new(1.5, edge));
                p.circle_filled(bc, 16.0, Color32::from_rgb(0x36, 0x36, 0x3B));
                p.circle_stroke(bc, 16.0, Stroke::new(1.0, hi));
                p.circle_filled(
                    bc + vec2(if dx < 0.0 { 20.0 } else { -20.0 }, 14.0),
                    9.0,
                    Color32::from_rgb(0x1A, 0x1A, 0x1D),
                );
            }
        }
    }
}

/// Interactive equalizer graph: six draggable points joined by a smooth curve.
/// Returns `(changed, released)`.
pub fn eq_graph(ui: &mut Ui, bands: &mut [i8; 6], editable: bool) -> SliderResult {
    let w = ui.available_width();
    let h = 210.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let plot = Rect::from_min_max(rect.min + vec2(28.0, 12.0), rect.max - vec2(8.0, 38.0));
    let p = ui.painter().clone();
    let span = f32::from(EQ_MAX - EQ_MIN);
    let y_of = |v: f32| plot.bottom() - (v - f32::from(EQ_MIN)) / span * plot.height();
    let v_of = |y: f32| ((plot.bottom() - y) / plot.height() * span + f32::from(EQ_MIN)).round();

    for (v, label) in [(EQ_MAX, "+10"), (0, "0"), (EQ_MIN, "-10")] {
        let y = y_of(f32::from(v));
        p.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0, if v == 0 { MUTED } else { DIVIDER }),
        );
        p.text(
            pos2(rect.left(), y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(10.5),
            SUBTEXT,
        );
    }
    let n = bands.len();
    let col_w = plot.width() / n as f32;
    let xs: Vec<f32> = (0..n)
        .map(|i| plot.left() + col_w * (i as f32 + 0.5))
        .collect();
    for (i, x) in xs.iter().enumerate() {
        p.line_segment(
            [pos2(*x, plot.top()), pos2(*x, plot.bottom())],
            Stroke::new(1.0, DIVIDER),
        );
        let label = EQ_BAND_LABELS[i];
        p.text(
            pos2(*x, plot.bottom() + 18.0),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(if i == 0 { 9.5 } else { 11.5 }),
            if i == 0 { ACCENT } else { SUBTEXT },
        );
    }

    let mut out = SliderResult::default();
    for i in 0..n {
        let hit = Rect::from_center_size(
            pos2(xs[i], plot.center().y),
            vec2(col_w, plot.height() + 24.0),
        );
        let sense = if editable {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        };
        let resp = ui.interact(hit, ui.scope_id().with(("eq_band", i)), sense);
        if editable {
            if let Some(pos) = resp.interact_pointer_pos()
                && (resp.dragged() || resp.clicked())
            {
                let v = v_of(pos.y.clamp(plot.top(), plot.bottom())) as i8;
                if v != bands[i] {
                    bands[i] = v;
                    out.changed = true;
                }
            }
            if resp.drag_stopped() || resp.clicked() {
                out.released = true;
            }
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
        }
    }

    // Smooth curve (Catmull-Rom) through the points.
    let pts: Vec<Pos2> = xs
        .iter()
        .zip(bands.iter())
        .map(|(x, v)| pos2(*x, y_of(f32::from(*v))))
        .collect();
    let mut curve = Vec::new();
    for i in 0..pts.len() - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        for k in 0..12 {
            let t = k as f32 / 12.0;
            let t2 = t * t;
            let t3 = t2 * t;
            let f = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b
                    + (-a + c) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            curve.push(pos2(
                f(p0.x, p1.x, p2.x, p3.x),
                f(p0.y, p1.y, p2.y, p3.y).clamp(plot.top(), plot.bottom()),
            ));
        }
    }
    if let Some(last) = pts.last() {
        curve.push(*last);
    }
    let line_color = if editable {
        ACCENT
    } else {
        ACCENT.gamma_multiply(0.75)
    };
    // Gradient fill under the curve.
    let mut mesh = Mesh::default();
    let base = y_of(0.0);
    for pair in curve.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let idx = mesh.vertices.len() as u32;
        let fill = line_color.gamma_multiply(0.22);
        let clear = line_color.gamma_multiply(0.02);
        mesh.colored_vertex(a, fill);
        mesh.colored_vertex(b, fill);
        mesh.colored_vertex(pos2(b.x, base), clear);
        mesh.colored_vertex(pos2(a.x, base), clear);
        mesh.add_triangle(idx, idx + 1, idx + 2);
        mesh.add_triangle(idx, idx + 2, idx + 3);
    }
    p.add(Shape::mesh(mesh));
    p.add(Shape::line(curve, Stroke::new(2.5, line_color)));
    for (i, pt) in pts.iter().enumerate() {
        p.circle_filled(*pt, 8.0, if editable { Color32::WHITE } else { SUBTEXT });
        p.circle_stroke(*pt, 8.0, Stroke::new(2.0, line_color));
        if editable {
            p.text(
                *pt + vec2(0.0, -17.0),
                Align2::CENTER_CENTER,
                format!("{:+}", bands[i]),
                FontId::proportional(10.5),
                TEXT,
            );
        }
    }
    out
}

/// Horizontal list of selectable chips. Returns the index clicked, if any.
pub fn chips(ui: &mut Ui, id: &str, labels: &[&str], selected: Option<usize>) -> Option<usize> {
    let mut clicked = None;
    egui::ScrollArea::horizontal()
        .id_salt(("chips", id))
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, label) in labels.iter().enumerate() {
                    let is_sel = selected == Some(i);
                    let text = RichText::new(*label).size(13.5).color(if is_sel {
                        Color32::BLACK
                    } else {
                        TEXT
                    });
                    let btn = egui::Button::new(text)
                        .fill(if is_sel { ACCENT } else { CARD_HI })
                        .corner_radius(CornerRadius::same(16))
                        .min_size(vec2(0.0, 32.0));
                    if ui.add(btn).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
    clicked
}

/// A full-width flat button (used for "Disconnect", "Power off" ...).
pub fn wide_button(ui: &mut Ui, label: &str, color: Color32) -> Response {
    let btn = egui::Button::new(RichText::new(label).size(15.0).color(color))
        .fill(CARD)
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .min_size(vec2(ui.available_width(), 48.0));
    ui.add(btn)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| {
        (f32::from(x) + (f32::from(y) - f32::from(x)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_unmultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

/// Value row like "Codec .......... LDAC".
pub fn info_row(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).size(14.5).color(SUBTEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).size(14.5).color(TEXT));
        });
    });
}
