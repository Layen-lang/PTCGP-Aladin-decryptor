// aladin-app/src/ui/controls_panel.rs

use egui::{RichText, Ui};
use crate::app::{AladinApp, AppPhase};
use super::theme::{self, ACCENT, TEXT, TEXT_DIM, WARN};

pub fn show(app: &mut AladinApp, ui: &mut Ui) {
    theme::section_label(ui, "OUTPUT DIRECTORY");
    ui.add_space(4.0);

    ui.horizontal(|ui: &mut Ui| {
        let path_str = app
            .output_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut display = path_str.clone();
        let text_color = if path_str.is_empty() { TEXT_DIM } else { TEXT };
        let hint = if path_str.is_empty() { "Choose an output folder…" } else { "" };

        // Flexible path width: starts with min_width, grows with text, clamped by max available.
        let min_path_w = 200.0;
        let button_w = 80.0;
        let max_available = (ui.available_width() - button_w - ui.spacing().item_spacing.x).max(min_path_w);
        
        let font_id = egui::FontId::proportional(13.0);
        let galley = ui.fonts(|f| f.layout_no_wrap(display.clone(), font_id.clone(), text_color));
        let text_w = galley.size().x.clamp(min_path_w, max_available);

        let te = egui::TextEdit::singleline(&mut display)
            .hint_text(hint)
            .text_color(text_color)
            .desired_width(text_w)
            .font(font_id);
        ui.add(te);

        if theme::ghost_button(ui, "Browse…").clicked() {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                app.output_dir = Some(folder);
            }
        }
    });

    // Device status hint below path
    ui.add_space(2.0);
    match &app.selected_device {
        Some(s) => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 3.0, ACCENT);
                ui.label(RichText::new(format!("Device: {s}")).color(TEXT_DIM).size(11.5));
            });
        }
        None => {
            ui.label(RichText::new("⚠  No device selected").color(WARN).size(11.5));
        }
    }
}

/// Control buttons rendered in the BottomPanel (called from app.rs).
pub fn action_buttons(app: &mut AladinApp, ui: &mut Ui) {
    let can_action = app.selected_device.is_some()
        && app.output_dir.is_some()
        && matches!(app.phase, AppPhase::Idle | AppPhase::Done);

    let is_running = !matches!(app.phase, AppPhase::Idle | AppPhase::Done);

    ui.add_enabled_ui(can_action && !is_running, |ui| {
        if theme::accent_button(ui, "▶  Decrypt").clicked() {
            app.start_decrypt_only();
        }
        ui.add_space(8.0);
        if theme::accent_button(ui, "↓  Pull").clicked() {
            app.start_pull();
        }
    });

    if is_running {
        ui.add_space(16.0);
        let label = match &app.phase {
            AppPhase::Pulling { .. } => "⟳  Pulling…",
            AppPhase::Decrypting { .. } => "⟳  Decrypting…",
            _ => "Processing…",
        };
        ui.label(RichText::new(label).color(ACCENT).strong());
    }
}
