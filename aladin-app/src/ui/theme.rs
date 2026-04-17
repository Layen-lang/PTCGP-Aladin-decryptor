// aladin-app/src/ui/theme.rs

use egui::{Color32, Rounding, Stroke, Style, Visuals};

// ── Palette ─────────────────────────────────────────────────────────────────
pub const BG:         Color32 = Color32::from_rgb(15,  23,  42);   // #0F172A
pub const SURFACE:    Color32 = Color32::from_rgb(30,  41,  59);   // #1E293B
pub const SURFACE2:   Color32 = Color32::from_rgb(51,  65,  85);   // #334155
pub const ACCENT:     Color32 = Color32::from_rgb(34,  197, 94);   // #22C55E
pub const TEXT:       Color32 = Color32::from_rgb(248, 250, 252);  // #F8FAFC
pub const TEXT_MUTED: Color32 = Color32::from_rgb(148, 163, 184);  // #94A3B8
pub const TEXT_DIM:   Color32 = Color32::from_rgb(100, 116, 139);  // #64748B
pub const BORDER:     Color32 = Color32::from_rgb(71,  85,  105);  // #475569
pub const ERROR:      Color32 = Color32::from_rgb(239, 68,  68);   // #EF4444
pub const WARN:       Color32 = Color32::from_rgb(245, 158, 11);   // #F59E0B
pub const SUCCESS:    Color32 = Color32::from_rgb(34,  197, 94);   // #22C55E

// ── Setup ────────────────────────────────────────────────────────────────────
pub fn apply(ctx: &egui::Context) {
    let mut vis = Visuals::dark();

    vis.panel_fill           = BG;
    vis.window_fill          = SURFACE;
    vis.override_text_color  = Some(TEXT);
    vis.selection.bg_fill    = ACCENT.linear_multiply(0.3);
    vis.selection.stroke     = Stroke::new(1.0, ACCENT);
    vis.hyperlink_color      = ACCENT;

    // widget backgrounds
    let w = &mut vis.widgets;
    w.noninteractive.bg_fill    = SURFACE;
    w.noninteractive.bg_stroke  = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke  = Stroke::new(1.0, TEXT_MUTED);
    w.noninteractive.rounding   = Rounding::same(6.0);

    w.inactive.bg_fill          = SURFACE2;
    w.inactive.bg_stroke        = Stroke::new(1.0, BORDER);
    w.inactive.fg_stroke        = Stroke::new(1.0, TEXT);
    w.inactive.rounding         = Rounding::same(6.0);

    w.hovered.bg_fill           = Color32::from_rgb(63, 79, 103);
    w.hovered.bg_stroke         = Stroke::new(1.0, Color32::from_rgb(100, 116, 139));
    w.hovered.fg_stroke         = Stroke::new(1.5, TEXT);
    w.hovered.rounding          = Rounding::same(6.0);

    w.active.bg_fill            = SURFACE2;
    w.active.bg_stroke          = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke          = Stroke::new(1.5, TEXT);
    w.active.rounding           = Rounding::same(6.0);

    w.open.bg_fill              = SURFACE2;
    w.open.bg_stroke            = Stroke::new(1.0, ACCENT);

    vis.window_rounding      = Rounding::same(10.0);
    vis.menu_rounding        = Rounding::same(8.0);
    vis.clip_rect_margin     = 0.0;
    vis.popup_shadow         = egui::epaint::Shadow::NONE;
    vis.window_shadow        = egui::epaint::Shadow::NONE;

    ctx.set_visuals(vis);

    let mut style = Style::default();
    style.spacing.item_spacing      = egui::vec2(8.0, 6.0);
    style.spacing.button_padding    = egui::vec2(12.0, 7.0);
    style.spacing.window_margin     = egui::Margin::same(16.0);
    style.spacing.scroll.bar_width  = 4.0;
    ctx.set_style(style);
}

// ── Helper: section label ────────────────────────────────────────────────────
pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(10.5)
            .color(TEXT_DIM)
            .strong(),
    );
}

// ── Helper: accent button ────────────────────────────────────────────────────
pub fn accent_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let padding = ui.spacing().button_padding;
    let font_id = egui::FontId::proportional(13.0);
    let text = egui::WidgetText::from(label).into_galley(ui, None, 120.0, font_id);
    let desired_size = egui::vec2(120.0_f32.max(text.size().x + padding.x * 2.0), 32.0);

    let (rect, response) = ui.allocate_at_least(desired_size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let _visuals = ui.style().interact(&response);
        let is_active = response.is_pointer_button_down_on();
        
        let base_color = ACCENT;
        let shadow_color = Color32::from_rgb(22, 163, 74); // Darker green

        // Draw shadow/depth
        if !is_active {
            ui.painter().rect_filled(
                rect.translate(egui::vec2(0.0, 2.0)),
                Rounding::same(8.0),
                shadow_color,
            );
        }

        let button_rect = if is_active {
            rect.translate(egui::vec2(0.0, 2.0))
        } else {
            rect
        };

        ui.painter().rect_filled(
            button_rect,
            Rounding::same(8.0),
            if response.hovered() { Color32::from_rgb(74, 222, 128) } else { base_color },
        );

        let text_pos = button_rect.center() - text.size() / 2.0;
        ui.painter().galley(text_pos, text, Color32::BLACK);
    }

    response
}

// ── Helper: ghost button ─────────────────────────────────────────────────────
pub fn ghost_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let padding = ui.spacing().button_padding;
    let font_id = egui::FontId::proportional(12.5);
    let text = egui::WidgetText::from(label).into_galley(ui, None, 100.0, font_id);
    let desired_size = egui::vec2(70.0_f32.max(text.size().x + padding.x * 2.0), 28.0);

    let (rect, response) = ui.allocate_at_least(desired_size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let is_active = response.is_pointer_button_down_on();
        
        // Draw shadow/depth
        if !is_active {
            ui.painter().rect_filled(
                rect.translate(egui::vec2(0.0, 2.0)),
                Rounding::same(6.0),
                SURFACE2,
            );
        }

        let button_rect = if is_active {
            rect.translate(egui::vec2(0.0, 2.0))
        } else {
            rect
        };

        ui.painter().rect_filled(
            button_rect,
            Rounding::same(6.0),
            if response.hovered() { SURFACE2 } else { SURFACE },
        );
        ui.painter().rect_stroke(
            button_rect,
            Rounding::same(6.0),
            Stroke::new(1.0, BORDER),
        );

        let text_pos = button_rect.center() - text.size() / 2.0;
        ui.painter().galley(text_pos, text, TEXT_MUTED);
    }

    response
}

// ── Helper: danger button ────────────────────────────────────────────────────
pub fn danger_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let padding = ui.spacing().button_padding;
    let font_id = egui::FontId::proportional(12.5);
    let text = egui::WidgetText::from(label).into_galley(ui, None, 200.0, font_id);
    let desired_size = egui::vec2(text.size().x + padding.x * 2.0, 28.0);

    let (rect, response) = ui.allocate_at_least(desired_size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let is_active = response.is_pointer_button_down_on();
        
        // Draw shadow/depth
        if !is_active {
            ui.painter().rect_filled(
                rect.translate(egui::vec2(0.0, 2.0)),
                Rounding::same(8.0),
                ERROR.linear_multiply(0.3),
            );
        }

        let button_rect = if is_active {
            rect.translate(egui::vec2(0.0, 2.0))
        } else {
            rect
        };

        ui.painter().rect_filled(
            button_rect,
            Rounding::same(8.0),
            if response.hovered() { ERROR.linear_multiply(0.2) } else { SURFACE },
        );
        ui.painter().rect_stroke(
            button_rect,
            Rounding::same(8.0),
            Stroke::new(1.0, if response.hovered() { ERROR } else { ERROR.linear_multiply(0.5) }),
        );

        let text_pos = button_rect.center() - text.size() / 2.0;
        ui.painter().galley(text_pos, text, ERROR);
    }

    response
}

// ── Helper: icon button ──────────────────────────────────────────────────────
pub fn icon_button(ui: &mut egui::Ui, label: &str, size: egui::Vec2) -> egui::Response {
    let font_id = egui::FontId::proportional(14.0);
    let text = egui::WidgetText::from(label).into_galley(ui, None, size.x, font_id);

    let (rect, response) = ui.allocate_at_least(size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let is_active = response.is_pointer_button_down_on();
        
        // Draw shadow/depth
        if !is_active {
            ui.painter().rect_filled(
                rect.translate(egui::vec2(0.0, 1.5)),
                Rounding::same(5.0),
                SURFACE2,
            );
        }

        let button_rect = if is_active {
            rect.translate(egui::vec2(0.0, 1.5))
        } else {
            rect
        };

        ui.painter().rect_filled(
            button_rect,
            Rounding::same(5.0),
            if response.hovered() { SURFACE2 } else { SURFACE },
        );
        ui.painter().rect_stroke(
            button_rect,
            Rounding::same(5.0),
            Stroke::new(1.0, BORDER),
        );

        let text_pos = button_rect.center() - text.size() / 2.0;
        ui.painter().galley(text_pos, text, if response.enabled() { TEXT_MUTED } else { TEXT_DIM });
    }

    response
}

