//! The bottom detail panel: Logs / Stats / Inspect / Exec tabs.

use egui::RichText;
use egui_plot::{Line, Plot, PlotPoints};

use crate::app::{stats_key, DetailTab, Section, WslcDesktopApp};
use crate::poller::UiRequest;

impl WslcDesktopApp {
    pub(crate) fn detail_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("detail")
            .resizable(true)
            .default_height(240.0)
            .min_height(120.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let is_container = self.section == Section::Containers;
                    if is_container {
                        self.tab_button(ui, DetailTab::Logs, "Logs");
                        self.tab_button(ui, DetailTab::Stats, "Stats");
                        self.tab_button(ui, DetailTab::Exec, "Exec");
                    }
                    self.tab_button(ui, DetailTab::Inspect, "Inspect");

                    // Show what we are inspecting.
                    let subject = match self.section {
                        Section::Containers => self
                            .selected_container_target()
                            .unwrap_or_else(|| "no container selected".to_string()),
                        Section::Images => self
                            .selected_image
                            .clone()
                            .unwrap_or_else(|| "no image selected".to_string()),
                        Section::Volumes => self
                            .selected_volume
                            .clone()
                            .unwrap_or_else(|| "no volume selected".to_string()),
                        Section::Networks => self
                            .selected_network
                            .clone()
                            .unwrap_or_else(|| "no network selected".to_string()),
                        // The Commands section hides this panel; keep the match
                        // exhaustive.
                        Section::Commands => String::new(),
                    };
                    ui.label(RichText::new(format!("— {subject}")).weak());
                });
                ui.separator();

                match self.detail_tab {
                    DetailTab::Logs => self.logs_view(ui),
                    DetailTab::Stats => self.stats_view(ui),
                    DetailTab::Inspect => self.inspect_view(ui),
                    DetailTab::Exec => self.exec_view(ui),
                }
            });
    }

    fn tab_button(&mut self, ui: &mut egui::Ui, tab: DetailTab, label: &str) {
        if ui.selectable_label(self.detail_tab == tab, label).clicked() {
            self.detail_tab = tab;
        }
    }

    fn logs_view(&mut self, ui: &mut egui::Ui) {
        // Log controls.
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.settings.log_follow, "▶ Follow")
                .on_hover_text("Stream new log lines live")
                .clicked()
            {
                self.settings.log_follow = !self.settings.log_follow;
                self.reload_logs();
            }
            if ui
                .selectable_label(self.settings.log_timestamps, "🕑 Timestamps")
                .clicked()
            {
                self.settings.log_timestamps = !self.settings.log_timestamps;
                self.reload_logs();
            }
            if ui.button("⟳ Reload").clicked() {
                self.reload_logs();
            }
            if ui.button("📋 Copy all").clicked() {
                let text = self.logs_text.clone();
                self.copy_to_clipboard(ui.ctx(), text);
            }
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(&self.logs_text).monospace()).wrap());
            });
    }

    fn inspect_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("📋 Copy").clicked() {
                let text = self.inspect_text.clone();
                self.copy_to_clipboard(ui.ctx(), text);
            }
        });
        ui.separator();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(&self.inspect_text).monospace()).wrap());
            });
    }

    fn exec_view(&mut self, ui: &mut egui::Ui) {
        let target = self.selected_container_target();
        let running = self.selected_container_running();

        ui.horizontal(|ui| {
            ui.label("$");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.exec_command)
                    .hint_text("command, e.g. ls -la /")
                    .desired_width(ui.available_width() - 160.0)
                    .font(egui::TextStyle::Monospace),
            );
            let submit = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let can_run = running && !self.exec_command.trim().is_empty() && target.is_some();
            let clicked = ui
                .add_enabled(can_run, egui::Button::new("▶ Run"))
                .clicked();
            if (submit || clicked) && can_run {
                if let Some(t) = target.clone() {
                    self.exec_output = "running…".to_string();
                    self.send(UiRequest::Exec {
                        target: t,
                        command: self.exec_command.clone(),
                        workdir: String::new(),
                        user: String::new(),
                    });
                }
            }
            if ui.button("📋").on_hover_text("Copy output").clicked() {
                let out = self.exec_output.clone();
                self.copy_to_clipboard(ui.ctx(), out);
            }
        });
        if !running {
            ui.label(
                RichText::new("Container must be running to exec.")
                    .small()
                    .weak(),
            );
        }
        ui.separator();

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(&self.exec_output).monospace()).wrap());
            });
    }

    fn stats_view(&mut self, ui: &mut egui::Ui) {
        // The per-container numeric metrics (CPU / Mem / Net I/O / …) now live
        // inline in the container table, so this tab focuses on live history
        // charts for the current selection.
        let Some(id) = self.selected_container.clone() else {
            ui.label("Select a running container to see live charts.");
            return;
        };
        // 与 stats_by_id 相同：键按 12 位短 ID 归一，容忍 ≥3.x 的完整 ID。
        let Some(history) = self.stats_history.get(&stats_key(&id)) else {
            ui.label("No stats yet — waiting for samples (container running?).");
            return;
        };

        let cpu_points: PlotPoints = history
            .cpu
            .iter()
            .enumerate()
            .map(|(i, v)| [i as f64, *v])
            .collect();
        let mem_mib: Vec<f64> = history.mem.iter().map(|b| b / (1024.0 * 1024.0)).collect();
        let mem_points: PlotPoints = mem_mib
            .iter()
            .enumerate()
            .map(|(i, v)| [i as f64, *v])
            .collect();

        ui.columns(2, |cols| {
            cols[0].label(RichText::new("CPU %").weak());
            Plot::new("cpu_plot")
                .height(110.0)
                .include_y(0.0)
                .include_y(100.0)
                .show_axes([false, true])
                .show(&mut cols[0], |plot| {
                    plot.line(
                        Line::new(cpu_points).color(egui::Color32::from_rgb(0x30, 0xc0, 0xd0)),
                    );
                });

            cols[1].label(RichText::new("Memory (MiB)").weak());
            Plot::new("mem_plot")
                .height(110.0)
                .include_y(0.0)
                .show_axes([false, true])
                .show(&mut cols[1], |plot| {
                    plot.line(
                        Line::new(mem_points).color(egui::Color32::from_rgb(0xd0, 0x60, 0xc0)),
                    );
                });
        });
    }
}
