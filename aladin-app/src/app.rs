// aladin-app/src/app.rs

use std::{
    path::PathBuf,
    sync::mpsc::Receiver,
    time::Instant,
};

use egui::Context;
use aladin_core::adb::{list_devices_result, AdbDevice};
use aladin_core::pipeline::PipelineEvent;
use crate::worker::WorkerMsg;

#[derive(Default, PartialEq)]
pub enum AppPhase {
    #[default]
    Idle,
    Pulling { current: usize, total: usize },
    Decrypting { current: usize, total: usize },
    Done,
}

pub struct AladinApp {
    pub devices: Vec<AdbDevice>,
    pub selected_device: Option<String>,
    pub output_dir: Option<PathBuf>,
    pub phase: AppPhase,
    pub logs: Vec<String>,
    pub errors: Vec<String>,
    pub show_errors_popup: bool,
    pub decryption_requested: bool,
    // Decryption
    pub files_per_sec: f32,
    pub eta_secs: f32,
    last_progress_time: Option<Instant>,
    last_progress_count: usize,
    // Pull
    pub pull_eta_secs: f32,
    pull_start: Option<Instant>,
    rx: Option<Receiver<WorkerMsg>>,
    // Device refresh
    pub refreshing: bool,
    pub adb_error: Option<String>,
    device_rx: Option<std::sync::mpsc::Receiver<Result<Vec<AdbDevice>, String>>>,
}

impl Default for AladinApp {
    fn default() -> Self {
        Self {
            devices: vec![],
            selected_device: None,
            output_dir: None,
            phase: AppPhase::Idle,
            logs: vec![],
            errors: vec![],
            show_errors_popup: false,
            decryption_requested: false,
            files_per_sec: 0.0,
            eta_secs: 0.0,
            last_progress_time: None,
            last_progress_count: 0,
            pull_eta_secs: 0.0,
            pull_start: None,
            rx: None,
            refreshing: false,
            adb_error: None,
            device_rx: None,
        }
    }
}

impl AladinApp {
    pub fn refresh_devices(&mut self) {
        if self.refreshing { return; }
        self.refreshing = true;
        self.adb_error = None;
        let (tx, rx) = std::sync::mpsc::channel();
        self.device_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(list_devices_result());
        });
    }

    pub fn poll_devices(&mut self, ctx: &Context) {
        let result = match &self.device_rx {
            Some(rx) => match rx.try_recv() {
                Ok(r) => r,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint();
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.refreshing = false;
                    self.device_rx = None;
                    return;
                }
            },
            None => return,
        };
        self.device_rx = None;
        self.refreshing = false;
        match result {
            Ok(devices) => {
                if let Some(sel) = &self.selected_device {
                    if !devices.iter().any(|d| &d.serial == sel) {
                        self.selected_device = None;
                    }
                }
                self.devices = devices;
            }
            Err(e) => {
                self.adb_error = Some(e);
                self.devices = vec![];
            }
        }
    }

    pub fn start_pull(&mut self) {
        let Some(serial) = self.selected_device.clone() else { return };
        let Some(output_dir) = self.output_dir.clone() else { return };
        let pull_dir = output_dir.join(".cache");
        std::fs::create_dir_all(&pull_dir).ok();
        std::fs::create_dir_all(&output_dir).ok();

        self.phase = AppPhase::Pulling { current: 0, total: 0 };
        self.decryption_requested = false;
        self.logs.clear();
        self.errors.clear();
        self.pull_start = None;
        self.pull_eta_secs = 0.0;
        self.rx = Some(crate::worker::start_worker(serial, output_dir, pull_dir, crate::worker::WorkerAction::PullOnly));
    }

    pub fn start_decrypt_only(&mut self) {
        let Some(serial) = self.selected_device.clone() else { return };
        let Some(output_dir) = self.output_dir.clone() else { return };
        let pull_dir = output_dir.join(".cache");
        std::fs::create_dir_all(&pull_dir).ok();
        std::fs::create_dir_all(&output_dir).ok();

        self.phase = AppPhase::Decrypting { current: 0, total: 0 };
        self.decryption_requested = true;
        self.logs.clear();
        self.errors.clear();
        self.last_progress_time = None;
        self.last_progress_count = 0;
        self.rx = Some(crate::worker::start_worker(serial, output_dir, pull_dir, crate::worker::WorkerAction::DecryptOnly));
    }

    /// Must be called in update() to drain the worker channel.
    pub fn poll_worker(&mut self, ctx: &Context) {
        loop {
            let msg = match &self.rx {
                Some(rx) => match rx.try_recv() {
                    Ok(msg) => Some(msg),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.rx = None;
                        break;
                    }
                },
                None => break,
            };
            if let Some(msg) = msg {
                self.handle_worker_msg(msg);
                ctx.request_repaint();
            }
        }
    }

    fn handle_worker_msg(&mut self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::PullProgress { current, total } => {
                // Start the timer on the first real progress packet
                if self.pull_start.is_none() && total > 0 && current > 0 {
                    self.pull_start = Some(Instant::now());
                }
                if let Some(start) = self.pull_start {
                    if total > 0 && current > 0 && current < total {
                        let elapsed = start.elapsed().as_secs_f32();
                        let progress = current as f32 / total as f32;
                        self.pull_eta_secs = elapsed * (1.0 - progress) / progress;
                    }
                }
                self.phase = AppPhase::Pulling { current, total };
            }
            WorkerMsg::PipelineEvent(ev) => match ev {
                PipelineEvent::Log(s) => self.logs.push(s),
                PipelineEvent::Error(s) => {
                    self.errors.push(s.clone());
                    // Only show non-file errors in logs
                    if !s.contains("[!]") {
                        self.logs.push(s);
                    }
                }
                PipelineEvent::Progress { current, total } => {
                    let now = Instant::now();
                    if let Some(last_t) = self.last_progress_time {
                        let dt = now.duration_since(last_t).as_secs_f32();
                        let delta = (current - self.last_progress_count) as f32;
                        if dt > 0.1 {
                            self.files_per_sec = delta / dt;
                            let remaining = (total - current) as f32;
                            self.eta_secs = if self.files_per_sec > 0.0 {
                                remaining / self.files_per_sec
                            } else {
                                0.0
                            };
                        }
                    }
                    self.last_progress_time = Some(now);
                    self.last_progress_count = current;
                    self.phase = AppPhase::Decrypting { current, total };
                }
                PipelineEvent::Done { decrypted, errors } => {
                    if self.decryption_requested {
                        self.logs.push(format!(
                            "[✓] Done — {} decrypted, {} errors",
                            decrypted, errors
                        ));
                    } else {
                        // It was a Pull-only action
                        // Ensure progress bar shows 100%
                        self.phase = AppPhase::Pulling { current: 100, total: 100 };
                    }
                    self.phase = AppPhase::Done;
                }
            },
        }
    }
}

impl eframe::App for AladinApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Apply theme once per frame (idempotent)
        crate::ui::theme::apply(ctx);
        self.poll_worker(ctx);
        self.poll_devices(ctx);

        // ── Top bar ──────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("top_bar")
            .exact_height(44.0)
            .frame(
                egui::Frame::none()
                    .fill(crate::ui::theme::SURFACE)
                    .inner_margin(egui::Margin::symmetric(16.0, 0.0))
                    .stroke(egui::Stroke::new(1.0, crate::ui::theme::BORDER)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Aladin Decryptor")
                                .strong()
                                .size(15.0)
                                .color(crate::ui::theme::TEXT),
                        );
                        ui.label(
                            egui::RichText::new("Pokemon TCGP")
                                .size(12.0)
                                .color(crate::ui::theme::TEXT_DIM),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (status_text, status_color) = match &self.phase {
                                AppPhase::Idle => ("Idle", crate::ui::theme::TEXT_DIM),
                                AppPhase::Pulling { .. } => ("Pulling…", crate::ui::theme::ACCENT),
                                AppPhase::Decrypting { .. } => ("Decrypting…", crate::ui::theme::ACCENT),
                                AppPhase::Done => ("Done", crate::ui::theme::SUCCESS),
                            };
                            ui.label(
                                egui::RichText::new(status_text)
                                    .size(11.5)
                                    .color(status_color),
                            );
                            // status dot
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(8.0, 8.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle_filled(rect.center(), 3.5, status_color);
                        });
                    });
                });
            });

        // ── Bottom action bar ────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("bottom_bar")
            .exact_height(56.0)
            .frame(
                egui::Frame::none()
                    .fill(crate::ui::theme::SURFACE)
                    .inner_margin(egui::Margin::symmetric(16.0, 0.0))
                    .stroke(egui::Stroke::new(1.0, crate::ui::theme::BORDER)),
            )
            .show(ctx, |ui| {
                ui.add_space(11.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::ui::controls_panel::action_buttons(self, ui);
                    ui.add_space(8.0);
                    crate::ui::progress_panel::errors_button(self, ui);
                });
            });

        // ── Left: device list ────────────────────────────────────────────────
        egui::SidePanel::left("devices_panel")
            .resizable(false)
            .exact_width(200.0)
            .frame(
                egui::Frame::none()
                    .fill(crate::ui::theme::BG)
                    .inner_margin(egui::Margin::same(12.0))
                    .stroke(egui::Stroke::new(1.0, crate::ui::theme::BORDER)),
            )
            .show(ctx, |ui| {
                crate::ui::device_panel::show(self, ui);
            });

        // ── Central panel ────────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(crate::ui::theme::BG)
                    .inner_margin(egui::Margin::same(12.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("central_scroll")
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        crate::ui::controls_panel::show(self, ui);
                        ui.add_space(8.0);
                        crate::ui::progress_panel::show(self, ui);
                    });
            });

        if matches!(
            self.phase,
            AppPhase::Pulling { .. } | AppPhase::Decrypting { .. }
        ) {
            ctx.request_repaint();
        }
    }
}
