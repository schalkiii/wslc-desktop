//! Modal overlays: confirm, create-container wizard, volume/network/pull
//! dialogs, and the transient toast.

use std::time::Duration;

use egui::{Align2, Color32, RichText};

use crate::app::WslcDesktopApp;
use crate::poller::UiRequest;

/// How long a toast stays visible.
const TOAST_TTL: Duration = Duration::from_secs(4);

impl WslcDesktopApp {
    pub(crate) fn draw_confirm(&mut self, ctx: &egui::Context) {
        if self.confirm.is_none() {
            return;
        }
        let message = self.confirm.as_ref().unwrap().message.clone();

        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new("Confirm")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(&message).size(15.0));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(RichText::new("Confirm").color(Color32::WHITE))
                        .clicked()
                    {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            if let Some(mut state) = self.confirm.take() {
                if let Some(action) = state.action.take() {
                    self.send(UiRequest::Action(action));
                }
            }
        } else if cancelled {
            self.confirm = None;
        }
    }

    pub(crate) fn draw_run_dialog(&mut self, ctx: &egui::Context) {
        if self.run_dialog.is_none() {
            return;
        }

        let mut do_run = false;
        let mut cancel = false;
        let mut preview = String::new();

        if let Some(d) = self.run_dialog.as_mut() {
            egui::Window::new("Create / run container")
                .collapsible(false)
                .resizable(true)
                .default_width(560.0)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                        egui::Grid::new("run_form")
                            .num_columns(2)
                            .spacing([10.0, 8.0])
                            .min_col_width(90.0)
                            .show(ui, |ui| {
                                field(ui, "Image", &mut d.image, "repository:tag");
                                field(ui, "Name", &mut d.name, "(optional)");
                                field(ui, "Ports", &mut d.ports, "8080:80, 5432:5432");
                                field(ui, "Env", &mut d.env, "KEY=VALUE, KEY2=VALUE2");
                                field(ui, "Volumes", &mut d.volumes, "C:/data:/data, named:/var");
                                field(ui, "Network", &mut d.network, "(optional)");
                                field(ui, "Workdir", &mut d.workdir, "/app");
                                field(ui, "User", &mut d.user, "uid[:gid] or name");
                                field(ui, "Hostname", &mut d.hostname, "(optional)");
                                field(ui, "Memory", &mut d.memory, "512M, 1G");
                                field(ui, "CPUs", &mut d.cpus, "0.5, 2");
                                field(ui, "Entrypoint", &mut d.entrypoint, "(optional)");
                                field(ui, "Command", &mut d.command, "override CMD");
                                ui.end_row();
                            });

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut d.detach, "Detached (-d)");
                            ui.checkbox(&mut d.auto_remove, "Auto-remove (--rm)");
                            ui.checkbox(&mut d.publish_all, "Publish all (-P)");
                        });

                        ui.add_space(6.0);
                        preview = d.to_spec().preview();
                        ui.label(RichText::new(&preview).monospace().small().weak());
                    });

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let can_run = !d.image.trim().is_empty();
                        if ui.add_enabled(can_run, egui::Button::new("▶ Run")).clicked() {
                            do_run = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
        }

        if do_run {
            if let Some(d) = self.run_dialog.take() {
                self.send(UiRequest::Run(Box::new(d.to_spec())));
                self.show_toast("run requested…", false);
            }
        } else if cancel {
            self.run_dialog = None;
        }
    }

    pub(crate) fn draw_volume_dialog(&mut self, ctx: &egui::Context) {
        if self.volume_dialog.is_none() {
            return;
        }
        let mut create = false;
        let mut cancel = false;

        if let Some(d) = self.volume_dialog.as_mut() {
            egui::Window::new("Create volume")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("volume_form")
                        .num_columns(2)
                        .spacing([10.0, 10.0])
                        .show(ui, |ui| {
                            field(ui, "Name", &mut d.name, "my-volume");
                            ui.label("Driver");
                            egui::ComboBox::from_id_salt("vol_driver")
                                .selected_text(if d.driver.is_empty() {
                                    "guest".to_string()
                                } else {
                                    d.driver.clone()
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut d.driver, "guest".to_string(), "guest");
                                    ui.selectable_value(&mut d.driver, "vhd".to_string(), "vhd");
                                });
                            ui.end_row();
                        });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let can = !d.name.trim().is_empty();
                        if ui.add_enabled(can, egui::Button::new("➕ Create")).clicked() {
                            create = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
        }

        if create {
            if let Some(d) = self.volume_dialog.take() {
                self.send(UiRequest::CreateVolume {
                    name: d.name.trim().to_string(),
                    driver: d.driver,
                });
            }
        } else if cancel {
            self.volume_dialog = None;
        }
    }

    pub(crate) fn draw_network_dialog(&mut self, ctx: &egui::Context) {
        if self.network_dialog.is_none() {
            return;
        }
        let mut create = false;
        let mut cancel = false;

        if let Some(d) = self.network_dialog.as_mut() {
            egui::Window::new("Create network")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("network_form")
                        .num_columns(2)
                        .spacing([10.0, 10.0])
                        .show(ui, |ui| {
                            field(ui, "Name", &mut d.name, "my-network");
                            ui.end_row();
                        });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let can = !d.name.trim().is_empty();
                        if ui.add_enabled(can, egui::Button::new("➕ Create")).clicked() {
                            create = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
        }

        if create {
            if let Some(d) = self.network_dialog.take() {
                self.send(UiRequest::CreateNetwork {
                    name: d.name.trim().to_string(),
                });
            }
        } else if cancel {
            self.network_dialog = None;
        }
    }

    pub(crate) fn draw_pull_dialog(&mut self, ctx: &egui::Context) {
        if self.pull_dialog.is_none() {
            return;
        }
        let mut pull = false;
        let mut cancel = false;

        if let Some(d) = self.pull_dialog.as_mut() {
            egui::Window::new("Pull image")
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("pull_form")
                        .num_columns(2)
                        .spacing([10.0, 10.0])
                        .show(ui, |ui| {
                            ui.label("Reference");
                            ui.add(
                                egui::TextEdit::singleline(&mut d.reference)
                                    .desired_width(300.0)
                                    .hint_text("docker.io/library/nginx:latest"),
                            );
                            ui.end_row();
                        });
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Pull runs in the background; watch the toast for the result.")
                            .small()
                            .weak(),
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let can = !d.reference.trim().is_empty();
                        if ui.add_enabled(can, egui::Button::new("⬇ Pull")).clicked() {
                            pull = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
        }

        if pull {
            if let Some(d) = self.pull_dialog.take() {
                self.send(UiRequest::Pull {
                    reference: d.reference.trim().to_string(),
                });
                self.show_toast("pull requested…", false);
            }
        } else if cancel {
            self.pull_dialog = None;
        }
    }

    pub(crate) fn draw_command_dialog(&mut self, ctx: &egui::Context) {
        if self.command_dialog.is_none() {
            return;
        }
        let mut save = false;
        let mut cancel = false;

        if let Some(d) = self.command_dialog.as_mut() {
            let title = if d.editing.is_some() {
                "Edit saved command"
            } else {
                "Add saved command"
            };
            egui::Window::new(title)
                .collapsible(false)
                .resizable(true)
                .default_width(600.0)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("command_form")
                        .num_columns(2)
                        .spacing([10.0, 8.0])
                        .min_col_width(90.0)
                        .show(ui, |ui| {
                            field(ui, "Name", &mut d.name, "qinglong");
                            field(ui, "Description", &mut d.description, "青龙面板");
                            ui.end_row();
                        });

                    ui.add_space(6.0);
                    ui.label("Command");
                    ui.add(
                        egui::TextEdit::multiline(&mut d.command)
                            .desired_rows(5)
                            .desired_width(f32::INFINITY)
                            .code_editor()
                            .hint_text("wslc run -d --name my-app -p 8080:80 my/image:latest"),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Tip: paste a full `wslc run …` line. A leading `wslc` is optional. \
                             Runs in the background; watch the toast for the result.",
                        )
                        .small()
                        .weak(),
                    );

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let can = !d.name.trim().is_empty() && !d.command.trim().is_empty();
                        if ui.add_enabled(can, egui::Button::new("💾 Save")).clicked() {
                            save = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
        }

        if save {
            if let Some(d) = self.command_dialog.take() {
                let entry = crate::app::SavedCommand {
                    name: d.name.trim().to_string(),
                    description: d.description.trim().to_string(),
                    command: d.command.trim().to_string(),
                };
                match d.editing {
                    Some(index) if index < self.saved_commands.len() => {
                        self.saved_commands[index] = entry;
                        self.show_toast("command updated", false);
                    }
                    _ => {
                        self.saved_commands.push(entry);
                        self.show_toast("command saved", false);
                    }
                }
            }
        } else if cancel {
            self.command_dialog = None;
        }
    }

    pub(crate) fn draw_toast(&mut self, ctx: &egui::Context) {
        let Some(toast) = &self.toast else { return };
        if toast.created.elapsed() > TOAST_TTL {
            self.toast = None;
            return;
        }
        let (bg, fg) = if toast.is_error {
            (Color32::from_rgb(0x5a, 0x1e, 0x1e), Color32::from_rgb(0xff, 0xd0, 0xd0))
        } else {
            (Color32::from_rgb(0x1e, 0x40, 0x2a), Color32::from_rgb(0xd0, 0xff, 0xdc))
        };
        let message = toast.message.clone();

        egui::Area::new("toast".into())
            .anchor(Align2::CENTER_BOTTOM, [0.0, -40.0])
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(bg)
                    .rounding(6.0)
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.colored_label(fg, message);
                    });
            });

        // Keep repainting so the toast expires on time.
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

/// A labelled single-line text field row inside a 2-column grid.
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(360.0)
            .hint_text(hint),
    );
    ui.end_row();
}
