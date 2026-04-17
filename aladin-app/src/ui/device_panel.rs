// aladin-app/src/ui/device_panel.rs

use egui::{Color32, RichText, ScrollArea, Ui};
use crate::app::AladinApp;
use super::theme::{self, ACCENT, BORDER, ERROR, SURFACE2, TEXT, TEXT_DIM, TEXT_MUTED};

pub fn show(app: &mut AladinApp, ui: &mut Ui) {
    ui.add_space(2.0);

    // ── Header: label + refresh button ───────────────────────────────────────
    ui.horizontal(|ui: &mut Ui| {
        theme::section_label(ui, "ADB DEVICES");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(!app.refreshing, |ui| {
                if theme::icon_button(ui, "↺", egui::vec2(26.0, 26.0))
                    .on_hover_text("Refresh devices")
                    .clicked()
                {
                    app.refresh_devices();
                }
            });
        });
    });

    // ── Status line ───────────────────────────────────────────────────────────
    ui.add_space(4.0);
    if app.refreshing {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(11.0).color(ACCENT));
            ui.label(RichText::new("Scanning…").color(ACCENT).size(11.0));
        });
    } else if let Some(err) = &app.adb_error.clone() {
        ui.label(RichText::new(format!("⚠  {err}")).color(ERROR).size(11.0));
    }

    ui.add_space(6.0);

    // Separator
    ui.painter().hline(
        ui.available_rect_before_wrap().x_range(),
        ui.cursor().top(),
        egui::Stroke::new(1.0, BORDER),
    );
    ui.add_space(10.0);

    // ── Empty state ───────────────────────────────────────────────────────────
    if app.devices.is_empty() && !app.refreshing {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("⚡").size(28.0).color(TEXT_DIM));
            ui.add_space(6.0);
            ui.label(RichText::new("No device").color(TEXT_DIM).size(12.0));
            ui.label(RichText::new("detected").color(TEXT_DIM).size(12.0));
        });
        return;
    }

    // ── Device list ───────────────────────────────────────────────────────────
    ScrollArea::vertical().id_salt("devices").show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        for device in &app.devices {
            let is_online = device.status == "device";
            let is_selected = app
                .selected_device
                .as_deref()
                .map(|s| s == device.serial)
                .unwrap_or(false);

            let dot_color = if is_online { ACCENT } else { ERROR };
            let bg = if is_selected { SURFACE2 } else { Color32::TRANSPARENT };
            let border = if is_selected {
                egui::Stroke::new(1.0, ACCENT)
            } else {
                egui::Stroke::new(1.0, Color32::TRANSPARENT)
            };

            let resp = egui::Frame::none()
                .fill(bg)
                .stroke(border)
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        // status dot
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(8.0, 8.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().circle_filled(rect.center(), 3.5, dot_color);

                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(
                                RichText::new(&device.serial)
                                    .color(if is_online { TEXT } else { TEXT_MUTED })
                                    .size(12.5)
                                    .strong(),
                            );
                            ui.label(
                                RichText::new(&device.status)
                                    .color(if is_online { ACCENT } else { ERROR })
                                    .size(10.5),
                            );
                        });
                    });
                })
                .response;

            if resp.interact(egui::Sense::click()).clicked() && is_online {
                app.selected_device = Some(device.serial.clone());
            }
            if resp.interact(egui::Sense::hover()).hovered() && is_online {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
    });
}
