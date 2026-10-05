//! Top bar, left navigation sidebar, status bar, and the central resource table.

use egui::{Align, Layout, RichText};

use crate::app::{
    state_color, stats_key, CommandSortKey, ContainerSortKey, DetailTab, NetworkDialog, PullDialog,
    RunDialog, Section, VolumeDialog, WslcDesktopApp,
};
use crate::poller::{Action, UiRequest};
use crate::wslc::types::relative_time;

impl WslcDesktopApp {
    pub(crate) fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("wslc-desktop");
                if let Some(version) = &self.version {
                    ui.label(RichText::new(format!("wslc {version}")).weak());
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Theme toggle.
                    let theme_icon = if self.settings.dark_mode {
                        "☀"
                    } else {
                        "🌙"
                    };
                    if ui
                        .button(theme_icon)
                        .on_hover_text("Toggle light / dark theme")
                        .clicked()
                    {
                        self.toggle_theme(ctx);
                    }

                    // Global UI scale (scales layout + fonts together).
                    // Applied on drag release only: re-scaling mid-drag makes
                    // the slider fight its own cursor and snap back, so the
                    // value never settles. Keyboard / scroll changes apply live.
                    ui.scope(|ui| {
                        ui.set_max_width(150.0);
                        let scale_resp = ui
                            .add(
                                egui::Slider::new(&mut self.settings.ui_scale, 0.8..=2.5)
                                    .step_by(0.05)
                                    .fixed_decimals(2)
                                    .suffix("× UI"),
                            )
                            .on_hover_text("Global UI scale — affects layout and font size");
                        if scale_resp.drag_stopped()
                            || (scale_resp.changed() && !scale_resp.dragged())
                        {
                            ctx.set_pixels_per_point(self.settings.ui_scale);
                        }
                    });

                    // Auto-refresh toggle.
                    if ui
                        .selectable_label(self.settings.auto_refresh, "⟲ Auto")
                        .on_hover_text("Auto-refresh every 2s")
                        .clicked()
                    {
                        self.settings.auto_refresh = !self.settings.auto_refresh;
                        self.send(UiRequest::SetAutoRefresh(self.settings.auto_refresh));
                    }

                    if ui.button("⟳ Refresh").on_hover_text("F5").clicked() {
                        self.send(UiRequest::RefreshNow);
                    }

                    if ui.button("⬇ Pull…").clicked() {
                        self.pull_dialog = Some(PullDialog::default());
                    }

                    if ui.button("▶ Run…").clicked() {
                        let image = self
                            .selected_image
                            .clone()
                            .or_else(|| self.images.first().map(|i| i.reference()))
                            .unwrap_or_default();
                        self.run_dialog = Some(RunDialog::with_image(image));
                    }

                    ui.add_space(12.0);
                    ui.label("🔍");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .hint_text("filter…")
                            .desired_width(170.0),
                    );
                });
            });
            ui.add_space(4.0);
        });
    }

    pub(crate) fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(190.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                let running = self.running_count();
                self.nav_item(
                    ui,
                    Section::Containers,
                    "📦 Containers",
                    self.containers.len(),
                    Some(running),
                );
                self.nav_item(ui, Section::Images, "🖼 Images", self.images.len(), None);
                self.nav_item(ui, Section::Volumes, "💾 Volumes", self.volumes.len(), None);
                self.nav_item(
                    ui,
                    Section::Networks,
                    "🌐 Networks",
                    self.networks.len(),
                    None,
                );

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
                self.nav_item(
                    ui,
                    Section::Commands,
                    "⭐ Commands",
                    self.saved_commands.len(),
                    None,
                );

                ui.add_space(12.0);
                ui.separator();

                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.add_space(6.0);
                    if let Some(err) = &self.snapshot_error {
                        ui.colored_label(
                            egui::Color32::from_rgb(0xc0, 0x50, 0x50),
                            RichText::new("⚠ wslc error").strong(),
                        );
                        ui.label(RichText::new(err.clone()).small().weak());
                    }
                });
            });
    }

    fn nav_item(
        &mut self,
        ui: &mut egui::Ui,
        section: Section,
        label: &str,
        count: usize,
        running: Option<usize>,
    ) {
        let selected = self.section == section;
        let text = if let Some(running) = running {
            format!("{label}   {running}/{count}")
        } else {
            format!("{label}   {count}")
        };
        if ui
            .selectable_label(selected, RichText::new(text).size(15.0))
            .clicked()
        {
            self.section = section;
        }
        ui.add_space(2.0);
    }

    /// A one-line status bar pinned to the very bottom of the window.
    pub(crate) fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let running = self.running_count();
                ui.label(
                    RichText::new(format!(
                        "📦 {} ({} running)   🖼 {}   💾 {}   🌐 {}",
                        self.containers.len(),
                        running,
                        self.images.len(),
                        self.volumes.len(),
                        self.networks.len(),
                    ))
                    .small(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(t) = self.last_update {
                        let secs = t.elapsed().as_secs();
                        let live = if self.settings.auto_refresh {
                            "live"
                        } else {
                            "paused"
                        };
                        ui.label(
                            RichText::new(format!("{live} · updated {secs}s ago"))
                                .small()
                                .weak(),
                        );
                    }
                });
            });
            ui.add_space(2.0);
        });
    }

    pub(crate) fn central_table(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| match self.section {
            Section::Containers => self.container_table(ui),
            Section::Images => self.image_table(ui),
            Section::Volumes => self.volume_table(ui),
            Section::Networks => self.network_table(ui),
            Section::Commands => self.commands_table(ui),
        });
    }

    fn filter_matches(&self, haystack: &[&str]) -> bool {
        if self.filter.trim().is_empty() {
            return true;
        }
        let needle = self.filter.to_ascii_lowercase();
        haystack
            .iter()
            .any(|h| h.to_ascii_lowercase().contains(&needle))
    }

    fn container_table(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        // Snapshot the rows we need so we don't borrow self during the closure.
        struct Row {
            id: String,
            name: String,
            image: String,
            state: crate::wslc::ContainerState,
            created: String,
            changed: String,
            ports: Vec<(String, u16)>, // (display, host_port)
            target: String,
            running: bool,
            // Live resource usage, surfaced inline from `wslc stats`. Each is a
            // ready-to-display string ("-" when the container reports no stats).
            cpu: String,
            mem: String,
            mem_perc: String,
            net_io: String,
            block_io: String,
            pids: String,
            // Sortable primitives (raw/numeric) used by header-click sorting.
            cpu_f: f64,
            mem_f: f64,
            mem_perc_f: f64,
            net_f: f64,
            block_f: f64,
            pids_u: u32,
            created_at: i64,
            state_ord: u8,
        }
        let mut rows: Vec<Row> = self
            .containers
            .iter()
            .filter(|c| self.filter_matches(&[&c.name, &c.image]))
            .map(|c| {
                let stat = self.stats_by_id.get(&stats_key(&c.id));
                Row {
                    id: c.id.clone(),
                    name: c.display_name(),
                    image: c.image.clone(),
                    state: c.state(),
                    created: relative_time(c.created_at),
                    // ≥3.x 的 Status 行（如 "Exited (255) 21 minutes ago"）比相对
                    // 时间信息更准；≤2.x 无该字段时回退到相对时间。
                    changed: if c.status.is_empty() {
                        relative_time(c.state_changed_at)
                    } else {
                        c.status.clone()
                    },
                    ports: c.ports.iter().map(|p| (p.display(), p.host_port)).collect(),
                    target: c.target(),
                    running: c.state().is_running(),
                    cpu: stat.map(|s| s.cpu_perc.clone()).unwrap_or_else(dash),
                    mem: stat.map(|s| s.mem_usage.clone()).unwrap_or_else(dash),
                    mem_perc: stat.map(|s| s.mem_perc.clone()).unwrap_or_else(dash),
                    net_io: stat.map(|s| s.net_io.clone()).unwrap_or_else(dash),
                    block_io: stat.map(|s| s.block_io.clone()).unwrap_or_else(dash),
                    pids: stat.map(|s| s.pids.to_string()).unwrap_or_else(dash),
                    cpu_f: stat.map(|s| s.cpu_percent()).unwrap_or(0.0),
                    mem_f: stat.map(|s| s.mem_used_bytes()).unwrap_or(0.0),
                    mem_perc_f: stat.map(|s| s.mem_percent()).unwrap_or(0.0),
                    net_f: stat.map(|s| s.net_rx_bytes()).unwrap_or(0.0),
                    block_f: stat.map(|s| s.block_rx_bytes()).unwrap_or(0.0),
                    pids_u: stat.map(|s| s.pids).unwrap_or(0),
                    created_at: c.created_at,
                    state_ord: match c.state() {
                        crate::wslc::ContainerState::Running => 0,
                        crate::wslc::ContainerState::Paused => 1,
                        crate::wslc::ContainerState::Created => 2,
                        crate::wslc::ContainerState::Exited => 3,
                        crate::wslc::ContainerState::Unknown(_) => 4,
                    },
                }
            })
            .collect();

        // Sort rows by the active header (clicking a header toggles direction).
        {
            let key = self.container_sort_key;
            let asc = self.container_sort_asc;
            rows.sort_by(|a, b| {
                let ord = match key {
                    ContainerSortKey::Name => a.name.cmp(&b.name),
                    ContainerSortKey::Image => a.image.cmp(&b.image),
                    ContainerSortKey::State => a.state_ord.cmp(&b.state_ord),
                    ContainerSortKey::Cpu => a.cpu_f.total_cmp(&b.cpu_f),
                    ContainerSortKey::Mem => a.mem_f.total_cmp(&b.mem_f),
                    ContainerSortKey::MemPerc => a.mem_perc_f.total_cmp(&b.mem_perc_f),
                    ContainerSortKey::NetIo => a.net_f.total_cmp(&b.net_f),
                    ContainerSortKey::BlockIo => a.block_f.total_cmp(&b.block_f),
                    ContainerSortKey::Pids => a.pids_u.cmp(&b.pids_u),
                    ContainerSortKey::Created => a.created_at.cmp(&b.created_at),
                };
                if asc {
                    ord
                } else {
                    ord.reverse()
                }
            });
        }

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("containers_grid")
                    .num_columns(13)
                    .striped(true)
                    .spacing([12.0, 8.0])
                    .min_col_width(46.0)
                    .show(ui, |ui| {
                        // Clickable headers: clicking sorts by that column;
                        // clicking the active column toggles ascending/descending.
                        use crate::app::ContainerSortKey as CSK;
                        let headers: &[(&str, Option<CSK>)] = &[
                            ("", None),
                            ("Name", Some(CSK::Name)),
                            ("Image", Some(CSK::Image)),
                            ("State", Some(CSK::State)),
                            ("CPU", Some(CSK::Cpu)),
                            ("Mem", Some(CSK::Mem)),
                            ("Mem %", Some(CSK::MemPerc)),
                            ("Net I/O", Some(CSK::NetIo)),
                            ("Block I/O", Some(CSK::BlockIo)),
                            ("PIDs", Some(CSK::Pids)),
                            ("Created", Some(CSK::Created)),
                            ("Ports", None),
                            ("Actions", None),
                        ];
                        for (label, key) in headers {
                            if let Some(k) = key {
                                let active = self.container_sort_key == *k;
                                let arrow = if active {
                                    if self.container_sort_asc {
                                        " ▲"
                                    } else {
                                        " ▼"
                                    }
                                } else {
                                    ""
                                };
                                if ui
                                    .button(RichText::new(format!("{label}{arrow}")).small())
                                    .clicked()
                                {
                                    if self.container_sort_key == *k {
                                        self.container_sort_asc = !self.container_sort_asc;
                                    } else {
                                        self.container_sort_key = *k;
                                        self.container_sort_asc = true;
                                    }
                                }
                            } else {
                                ui.label(RichText::new(*label).strong());
                            }
                        }
                        ui.end_row();

                        for row in rows {
                            let selected =
                                self.selected_container.as_deref() == Some(row.id.as_str());

                            ui.colored_label(state_color(row.state), "●")
                                .on_hover_text(format!("changed {}", row.changed));

                            if ui
                                .selectable_label(selected, RichText::new(&row.name).strong())
                                .on_hover_text("Click to select · right-click to copy")
                                .clicked()
                            {
                                self.selected_container = Some(row.id.clone());
                                self.exec_output.clear();
                            }

                            ui.label(RichText::new(truncate(&row.image, 40)).weak())
                                .on_hover_text(&row.image);

                            ui.label(row.state.label());

                            // Live resource-usage columns (from `wslc stats`).
                            ui.label(RichText::new(&row.cpu).monospace());
                            ui.label(RichText::new(truncate(&row.mem, 22)).monospace())
                                .on_hover_text(&row.mem);
                            ui.label(RichText::new(&row.mem_perc).monospace());
                            ui.label(RichText::new(truncate(&row.net_io, 20)).monospace())
                                .on_hover_text(&row.net_io);
                            ui.label(RichText::new(truncate(&row.block_io, 20)).monospace())
                                .on_hover_text(&row.block_io);
                            ui.label(RichText::new(&row.pids).monospace());

                            ui.label(RichText::new(row.created).weak());

                            // Ports as clickable links (open host port in browser).
                            ui.horizontal(|ui| {
                                if row.ports.is_empty() {
                                    ui.label(RichText::new("-").weak());
                                }
                                for (display, host_port) in &row.ports {
                                    if ui
                                        .link(RichText::new(truncate(display, 26)).monospace())
                                        .on_hover_text(format!("Open http://127.0.0.1:{host_port}"))
                                        .clicked()
                                    {
                                        ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                                            "http://127.0.0.1:{host_port}"
                                        )));
                                    }
                                }
                            });

                            ui.horizontal(|ui| {
                                if row.running {
                                    if ui.small_button("⏹").on_hover_text("Stop").clicked() {
                                        self.send(UiRequest::Action(Action::Stop(
                                            row.target.clone(),
                                        )));
                                    }
                                    if ui
                                        .small_button("🔄")
                                        .on_hover_text("Restart (stop+start)")
                                        .clicked()
                                    {
                                        self.send(UiRequest::Action(Action::Restart(
                                            row.target.clone(),
                                        )));
                                    }
                                    if ui
                                        .small_button("💀")
                                        .on_hover_text("Kill (SIGKILL)")
                                        .clicked()
                                    {
                                        self.confirm(
                                            format!("Kill container \"{}\"?", row.name),
                                            Action::Kill(row.target.clone()),
                                        );
                                    }
                                } else if ui.small_button("▶").on_hover_text("Start").clicked() {
                                    self.send(UiRequest::Action(Action::Start(row.target.clone())));
                                }
                                if ui.small_button("📋").on_hover_text("Copy name").clicked() {
                                    let name = row.name.clone();
                                    self.copy_to_clipboard(ui.ctx(), name);
                                }
                                if ui.small_button("🗑").on_hover_text("Remove").clicked() {
                                    self.confirm(
                                        format!("Remove container \"{}\"?", row.name),
                                        Action::RemoveContainer(row.target.clone()),
                                    );
                                }
                            });
                            ui.end_row();
                        }
                    });

                ui.add_space(10.0);
                if ui.button("🧹 Prune stopped containers").clicked() {
                    self.confirm(
                        "Remove all stopped containers?".to_string(),
                        Action::PruneContainers,
                    );
                }
            });
    }

    fn image_table(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let rows: Vec<(String, String, String, String, usize)> = self
            .images
            .iter()
            .filter(|i| self.filter_matches(&[&i.repository, &i.tag]))
            .map(|i| {
                let in_use = self.containers.iter().filter(|c| c.uses_image(i)).count();
                (
                    i.reference(),
                    i.short_id(),
                    i.size_display(),
                    relative_time(i.created),
                    in_use,
                )
            })
            .collect();

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("images_grid")
                    .num_columns(6)
                    .striped(true)
                    .spacing([12.0, 8.0])
                    .min_col_width(50.0)
                    .show(ui, |ui| {
                        for h in [
                            "Repository:Tag",
                            "ID",
                            "Size",
                            "Created",
                            "In Use",
                            "Actions",
                        ] {
                            ui.label(RichText::new(h).strong());
                        }
                        ui.end_row();

                        for (reference, short_id, size, created, in_use) in rows {
                            let selected =
                                self.selected_image.as_deref() == Some(reference.as_str());
                            if ui
                                .selectable_label(
                                    selected,
                                    RichText::new(truncate(&reference, 48)).strong(),
                                )
                                .on_hover_text(&reference)
                                .clicked()
                            {
                                self.selected_image = Some(reference.clone());
                                self.detail_tab = DetailTab::Inspect;
                            }
                            ui.label(RichText::new(short_id).monospace().weak());
                            ui.label(size);
                            ui.label(created);
                            // In-use status: green count when referenced by ≥1
                            // container, click to jump to the Containers view
                            // filtered by this image.
                            let in_use_resp = if in_use > 0 {
                                ui.label(
                                    RichText::new(format!("● {in_use}"))
                                        .color(egui::Color32::from_rgb(0x3f, 0xb9, 0x50)),
                                )
                                .on_hover_text("Click to show containers using this image")
                            } else {
                                ui.label(RichText::new("—").weak())
                            };
                            if in_use > 0 && in_use_resp.clicked() {
                                self.filter = reference.clone();
                                self.section = Section::Containers;
                            }
                            ui.horizontal(|ui| {
                                if ui.small_button("▶ Run…").clicked() {
                                    self.run_dialog =
                                        Some(RunDialog::with_image(reference.clone()));
                                }
                                if ui
                                    .small_button("📋")
                                    .on_hover_text("Copy reference")
                                    .clicked()
                                {
                                    let r = reference.clone();
                                    self.copy_to_clipboard(ui.ctx(), r);
                                }
                                if ui.small_button("🗑").on_hover_text("Remove image").clicked() {
                                    self.confirm(
                                        format!("Remove image \"{reference}\"?"),
                                        Action::RemoveImage(reference.clone()),
                                    );
                                }
                            });
                            ui.end_row();
                        }
                    });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("⬇ Pull image…").clicked() {
                        self.pull_dialog = Some(PullDialog::default());
                    }
                    if ui.button("🧹 Prune unused images").clicked() {
                        self.confirm(
                            "Remove all dangling images?".to_string(),
                            Action::PruneImages,
                        );
                    }
                });
            });
    }

    fn volume_table(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let rows: Vec<(String, String)> = self
            .volumes
            .iter()
            .filter(|v| self.filter_matches(&[&v.name]))
            .map(|v| (v.name.clone(), v.driver.clone()))
            .collect();

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("volumes_grid")
                    .num_columns(3)
                    .striped(true)
                    .spacing([12.0, 8.0])
                    .min_col_width(50.0)
                    .show(ui, |ui| {
                        for h in ["Name", "Driver", "Actions"] {
                            ui.label(RichText::new(h).strong());
                        }
                        ui.end_row();

                        for (name, driver) in rows {
                            let selected = self.selected_volume.as_deref() == Some(name.as_str());
                            if ui
                                .selectable_label(
                                    selected,
                                    RichText::new(truncate(&name, 44)).monospace(),
                                )
                                .on_hover_text(&name)
                                .clicked()
                            {
                                self.selected_volume = Some(name.clone());
                                self.detail_tab = DetailTab::Inspect;
                            }
                            ui.label(driver);
                            ui.horizontal(|ui| {
                                if ui.small_button("📋").on_hover_text("Copy name").clicked() {
                                    let n = name.clone();
                                    self.copy_to_clipboard(ui.ctx(), n);
                                }
                                if ui
                                    .small_button("🗑")
                                    .on_hover_text("Remove volume")
                                    .clicked()
                                {
                                    self.confirm(
                                        format!("Remove volume \"{}\"?", truncate(&name, 24)),
                                        Action::RemoveVolume(name.clone()),
                                    );
                                }
                            });
                            ui.end_row();
                        }
                    });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("➕ Create volume…").clicked() {
                        self.volume_dialog = Some(VolumeDialog {
                            driver: "guest".to_string(),
                            ..Default::default()
                        });
                    }
                    if ui.button("🧹 Prune unused volumes").clicked() {
                        self.confirm(
                            "Remove all unused volumes?".to_string(),
                            Action::PruneVolumes,
                        );
                    }
                });
            });
    }

    fn network_table(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let rows: Vec<(String, String, String, String)> = self
            .networks
            .iter()
            .filter(|n| self.filter_matches(&[&n.name, &n.driver]))
            .map(|n| {
                (
                    n.display_name(),
                    n.short_id(),
                    n.driver.clone(),
                    n.scope.clone(),
                )
            })
            .collect();

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if rows.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("No networks. Create one below.").weak());
                }
                egui::Grid::new("networks_grid")
                    .num_columns(5)
                    .striped(true)
                    .spacing([12.0, 8.0])
                    .min_col_width(50.0)
                    .show(ui, |ui| {
                        for h in ["Name", "ID", "Driver", "Scope", "Actions"] {
                            ui.label(RichText::new(h).strong());
                        }
                        ui.end_row();

                        for (name, short_id, driver, scope) in rows {
                            let selected = self.selected_network.as_deref() == Some(name.as_str());
                            if ui
                                .selectable_label(
                                    selected,
                                    RichText::new(truncate(&name, 40)).strong(),
                                )
                                .clicked()
                            {
                                self.selected_network = Some(name.clone());
                                self.detail_tab = DetailTab::Inspect;
                            }
                            ui.label(RichText::new(short_id).monospace().weak());
                            ui.label(driver);
                            ui.label(scope);
                            if ui
                                .small_button("🗑")
                                .on_hover_text("Remove network")
                                .clicked()
                            {
                                self.confirm(
                                    format!("Remove network \"{name}\"?"),
                                    Action::RemoveNetwork(name.clone()),
                                );
                            }
                            ui.end_row();
                        }
                    });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("➕ Create network…").clicked() {
                        self.network_dialog = Some(NetworkDialog::default());
                    }
                    if ui.button("🧹 Prune unused networks").clicked() {
                        self.confirm(
                            "Remove all unused networks?".to_string(),
                            Action::PruneNetworks,
                        );
                    }
                });
            });
    }

    /// The saved-command library: run / copy / edit / delete reusable `wslc`
    /// command lines, seeded from `wslc-menu.ps1`.
    fn commands_table(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading("⭐ Saved commands");
            ui.label(
                RichText::new("run · copy · edit · delete your reusable wslc command lines")
                    .small()
                    .weak(),
            );
        });
        ui.add_space(4.0);

        // Snapshot (index, name, description, command) so we don't borrow self
        // while iterating; deferred actions are applied after the grid.
        let mut rows: Vec<(usize, String, String, String)> = self
            .saved_commands
            .iter()
            .enumerate()
            .filter(|(_, c)| self.filter_matches(&[&c.name, &c.description, &c.command]))
            .map(|(i, c)| (i, c.name.clone(), c.description.clone(), c.command.clone()))
            .collect();

        // Sort by the active commands-table column (click toggles direction).
        {
            let key = self.command_sort_key;
            let asc = self.command_sort_asc;
            rows.sort_by(|a, b| {
                let ord = match key {
                    CommandSortKey::Name => a.1.cmp(&b.1),
                    CommandSortKey::Description => a.2.cmp(&b.2),
                    CommandSortKey::Command => a.3.cmp(&b.3),
                };
                if asc {
                    ord
                } else {
                    ord.reverse()
                }
            });
        }

        let mut run: Option<usize> = None;
        let mut copy: Option<String> = None;
        let mut edit: Option<usize> = None;
        let mut delete: Option<usize> = None;

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.saved_commands.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("No saved commands. Add one below.").weak());
                }
                egui::Grid::new("commands_grid")
                    .num_columns(4)
                    .striped(true)
                    .spacing([12.0, 8.0])
                    .min_col_width(50.0)
                    .show(ui, |ui| {
                        // Clickable headers (sort by Name / Description / Command).
                        use crate::app::CommandSortKey as CMK;
                        let headers: &[(&str, Option<CMK>)] = &[
                            ("Name", Some(CMK::Name)),
                            ("Description", Some(CMK::Description)),
                            ("Command", Some(CMK::Command)),
                            ("Actions", None),
                        ];
                        for (label, key) in headers {
                            if let Some(k) = key {
                                let active = self.command_sort_key == *k;
                                let arrow = if active {
                                    if self.command_sort_asc {
                                        " ▲"
                                    } else {
                                        " ▼"
                                    }
                                } else {
                                    ""
                                };
                                if ui
                                    .button(RichText::new(format!("{label}{arrow}")).small())
                                    .clicked()
                                {
                                    if self.command_sort_key == *k {
                                        self.command_sort_asc = !self.command_sort_asc;
                                    } else {
                                        self.command_sort_key = *k;
                                        self.command_sort_asc = true;
                                    }
                                }
                            } else {
                                ui.label(RichText::new(*label).strong());
                            }
                        }
                        ui.end_row();

                        for (index, name, description, command) in &rows {
                            ui.label(RichText::new(truncate(name, 24)).strong())
                                .on_hover_text(name);
                            ui.label(RichText::new(truncate(description, 22)).weak())
                                .on_hover_text(description);
                            ui.label(RichText::new(truncate(command, 60)).monospace().small())
                                .on_hover_text(command);
                            ui.horizontal(|ui| {
                                if ui.small_button("▶").on_hover_text("Run this command").clicked() {
                                    run = Some(*index);
                                }
                                if ui.small_button("📋").on_hover_text("Copy command").clicked() {
                                    copy = Some(command.clone());
                                }
                                if ui.small_button("📝").on_hover_text("Edit").clicked() {
                                    edit = Some(*index);
                                }
                                if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                                    delete = Some(*index);
                                }
                            });
                            ui.end_row();
                        }
                    });

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("➕ Add command…").clicked() {
                        self.command_dialog = Some(crate::app::CommandDialog::default());
                    }
                    if ui.button("♻️ Restore presets").on_hover_text(
                        "Re-import the containers from wslc-menu.ps1 (does not remove your own entries)",
                    ).clicked() {
                        self.restore_command_presets();
                    }
                });
            });

        // Apply deferred actions after the immutable borrow ends.
        if let Some(index) = run {
            if let Some(cmd) = self.saved_commands.get(index) {
                let label = format!("run {}", cmd.name);
                let command = cmd.command.clone();
                self.send(UiRequest::RunRawCommand { label, command });
                self.show_toast(format!("running \"{}\"…", cmd.name), false);
            }
        }
        if let Some(text) = copy {
            self.copy_to_clipboard(ui.ctx(), text);
        }
        if let Some(index) = edit {
            if let Some(cmd) = self.saved_commands.get(index) {
                self.command_dialog = Some(crate::app::CommandDialog {
                    editing: Some(index),
                    name: cmd.name.clone(),
                    description: cmd.description.clone(),
                    command: cmd.command.clone(),
                });
            }
        }
        if let Some(index) = delete {
            if index < self.saved_commands.len() {
                let removed = self.saved_commands.remove(index);
                self.show_toast(format!("deleted \"{}\"", removed.name), false);
            }
        }
    }
}

/// Placeholder for an empty metric cell.
fn dash() -> String {
    "-".to_string()
}

/// Truncate a string to `max` chars with an ellipsis.
pub(crate) fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let taken: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{taken}…")
}
