// aladin-app/src/ui/progress_panel.rs

use egui::{Color32, ProgressBar, RichText, ScrollArea, Stroke, Ui, Window};
use crate::app::{AladinApp, AppPhase};
use super::theme::{
    self, ACCENT, BORDER, ERROR, SURFACE, TEXT_DIM, TEXT_MUTED, SUCCESS, WARN,
};

pub fn show(app: &mut AladinApp, ui: &mut Ui) {
    // ── Pull card ────────────────────────────────────────────────────────────
    let show_pull = !matches!(app.phase, AppPhase::Idle);
    if show_pull {
        pull_card(app, ui);
        ui.add_space(6.0);
    }

    // ── Decrypt card ─────────────────────────────────────────────────────────
    let show_decrypt = matches!(app.phase, AppPhase::Decrypting { .. })
        || (matches!(app.phase, AppPhase::Done) && app.decryption_requested);
    if show_decrypt {
        decrypt_card(app, ui);
        ui.add_space(6.0);
    }

    // ── Logs ─────────────────────────────────────────────────────────────────
    if !app.logs.is_empty() {
        logs_section(app, ui);
    }

    // ── Error popup ──────────────────────────────────────────────────────────
    if app.show_errors_popup {
        errors_popup(app, ui);
    }
}

fn pull_card(app: &AladinApp, ui: &mut Ui) {
    egui::Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                theme::section_label(ui, "PULL");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match &app.phase {
                        AppPhase::Pulling { current, total } if *total > 0 => {
                            let pct = (*current as f32 / *total as f32 * 100.0) as u8;
                            ui.label(
                                RichText::new(format!("{pct} %"))
                                    .color(ACCENT)
                                    .size(11.5)
                                    .strong(),
                            );
                        }
                        AppPhase::Done => {
                            ui.label(RichText::new("✓ Done").color(SUCCESS).size(11.5));
                        }
                        _ => {}
                    }
                });
            });

            ui.add_space(6.0);

            match &app.phase {
                AppPhase::Pulling { current, total } => {
                    if *total == 0 {
                        ui.add(
                            ProgressBar::new(0.0)
                                .desired_height(6.0)
                                .animate(true)
                                .fill(ACCENT),
                        );
                        ui.add_space(4.0);
                        ui.label(RichText::new("Connecting to ADB…").color(TEXT_MUTED).size(11.5));
                    } else {
                        let progress = *current as f32 / *total as f32;
                        ui.add(
                            ProgressBar::new(progress)
                                .desired_height(6.0)
                                .animate(true)
                                .fill(ACCENT),
                        );
                        ui.add_space(4.0);
                        if *total == 100 {
                            // Bulk pull: current = percent
                            let eta_str = eta_label(app.pull_eta_secs);
                            ui.label(
                                RichText::new(format!("{current} %  •  {eta_str}"))
                                    .color(TEXT_MUTED)
                                    .size(11.5),
                            );
                        } else {
                            // Incremental: current = files
                            let eta_str = eta_label(app.pull_eta_secs);
                            ui.label(
                                RichText::new(format!("{current} / {total}  •  {eta_str}"))
                                    .color(TEXT_MUTED)
                                    .size(11.5),
                            );
                        }
                    }
                }
                _ => {
                    // Done or decrypting — show full bar
                    ui.add(
                        ProgressBar::new(1.0)
                            .desired_height(6.0)
                            .fill(ACCENT.linear_multiply(0.5)),
                    );
                    ui.add_space(4.0);
                    ui.label(RichText::new("Pull complete").color(TEXT_DIM).size(11.5));
                }
            }
        });
}

fn decrypt_card(app: &AladinApp, ui: &mut Ui) {
    egui::Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                theme::section_label(ui, "DECRYPT");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match &app.phase {
                        AppPhase::Decrypting { current, total } => {
                            ui.label(
                                RichText::new(format!("{current} / {total}"))
                                    .color(TEXT_MUTED)
                                    .size(11.5),
                            );
                        }
                        AppPhase::Done => {
                            ui.label(RichText::new("✓ Done").color(SUCCESS).size(11.5));
                        }
                        _ => {}
                    }
                });
            });

            ui.add_space(6.0);

            match &app.phase {
                AppPhase::Decrypting { current, total } => {
                    let progress = if *total > 0 {
                        *current as f32 / *total as f32
                    } else {
                        0.0
                    };
                    ui.add(
                        ProgressBar::new(progress)
                            .desired_height(6.0)
                            .animate(true)
                            .fill(ACCENT),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if app.files_per_sec > 0.0 {
                            ui.label(
                                RichText::new(format!("{:.0} f/s", app.files_per_sec))
                                    .color(TEXT_MUTED)
                                    .size(11.5),
                            );
                            ui.label(RichText::new("•").color(TEXT_DIM).size(11.5));
                        }
                        let eta_str = eta_label(app.eta_secs);
                        ui.label(RichText::new(eta_str).color(TEXT_MUTED).size(11.5));
                    });
                }
                AppPhase::Done => {
                    ui.add(
                        ProgressBar::new(1.0)
                            .desired_height(6.0)
                            .fill(ACCENT),
                    );
                    ui.add_space(4.0);
                    ui.label(RichText::new("Decryption complete").color(TEXT_DIM).size(11.5));
                }
                _ => {
                    ui.add(
                        ProgressBar::new(0.0)
                            .desired_height(6.0)
                            .animate(true)
                            .fill(ACCENT),
                    );
                }
            }
        });
}

fn logs_section(app: &AladinApp, ui: &mut Ui) {
    theme::section_label(ui, "LOGS");
    ui.add_space(4.0);

    egui::Frame::none()
        .fill(Color32::from_rgb(10, 15, 28))
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt("logs")
                .max_height(180.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for line in &app.logs {
                        let color = if line.starts_with("[✓]") {
                            SUCCESS
                        } else if line.starts_with("[✗]") {
                            ERROR
                        } else if line.starts_with("[!]") {
                            WARN
                        } else {
                            TEXT_MUTED
                        };
                        ui.label(
                            RichText::new(line)
                                .color(color)
                                .size(11.5)
                                .font(egui::FontId::monospace(11.5)),
                        );
                    }
                });
        });
}

fn errors_popup(app: &mut AladinApp, ui: &mut Ui) {
    let errors = app.errors.clone();
    Window::new(RichText::new("Errors").color(ERROR).strong())
        .open(&mut app.show_errors_popup)
        .resizable(true)
        .vscroll(true)
        .min_width(400.0)
        .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for err in &errors {
                ui.label(RichText::new(err).color(ERROR).size(12.0));
            }
        });
}

fn eta_label(secs: f32) -> String {
    if secs <= 0.0 {
        return "ETA —".into();
    }
    if secs < 60.0 {
        format!("ETA {:.0}s", secs)
    } else {
        format!("ETA {:.0}m {:.0}s", (secs / 60.0).floor(), secs % 60.0)
    }
}

/// Errors button rendered in the BottomPanel.
pub fn errors_button(app: &mut AladinApp, ui: &mut Ui) {
    if !app.errors.is_empty() {
        if theme::danger_button(
            ui,
            &format!("⚠  {} error{}", app.errors.len(), if app.errors.len() > 1 { "s" } else { "" }),
        )
        .clicked()
        {
            app.show_errors_popup = true;
        }
    }
}
