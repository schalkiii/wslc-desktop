//! The egui application: state, event handling, and the top-level layout.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use egui::Color32;
use serde::{Deserialize, Serialize};

use crate::poller::{self, Action, UiRequest, WorkerEvent, WorkerHandle};
use crate::wslc::{Container, Image, Network, RunSpec, Stat, Volume};

/// Max samples kept per container for the CPU/memory history plots.
pub(crate) const MAX_HISTORY: usize = 60;

/// Normalize a container id into the stats-map key.
///
/// `wslc list` 只给 12 位短 ID，而 ≥3.x 的 `stats` 返回 64 位完整 ID，
/// 直接拿原串互查永远 miss；两边统一截前 12 位再对齐（≤2.x 两侧本就
/// 一致，截短后同样成立）。
pub(crate) fn stats_key(id: &str) -> String {
    id.chars().take(12).collect()
}

/// eframe storage key for [`Settings`].
const SETTINGS_KEY: &str = "wslc_desktop_settings";

/// eframe storage key for the saved-commands library.
const SAVED_COMMANDS_KEY: &str = "wslc_saved_commands";

/// Persisted user preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Settings {
    pub dark_mode: bool,
    pub auto_refresh: bool,
    pub log_timestamps: bool,
    pub log_follow: bool,
    /// Global UI scaling factor (1.0 = 100%). Applied via
    /// `Context::set_pixels_per_point` so it scales both layout and fonts.
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
}

/// Default UI scale: 110% so the whole interface and fonts read a bit larger
/// out of the box (overridable in the top bar; persisted once changed).
fn default_ui_scale() -> f32 {
    1.1
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dark_mode: true,
            auto_refresh: true,
            log_timestamps: false,
            log_follow: true,
            ui_scale: default_ui_scale(),
        }
    }
}

/// Which resource list is shown in the sidebar/main table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Section {
    Containers,
    Images,
    Volumes,
    Networks,
    Commands,
}

/// A reusable, user-editable `wslc` command line (e.g. a full `wslc run …`).
///
/// The library is seeded on first launch from the homelab containers defined in
/// `wslc-menu.ps1`, then persisted via eframe storage so edits survive restarts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct SavedCommand {
    /// Short display name, e.g. `qinglong`.
    pub name: String,
    /// Human-readable purpose, e.g. `青龙面板`.
    pub description: String,
    /// The full command line, e.g. `wslc run -d --name qinglong …`.
    pub command: String,
}

/// Which tab of the detail panel is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailTab {
    Logs,
    Stats,
    Inspect,
    Exec,
}

/// A rolling CPU%/memory-bytes history for one container.
#[derive(Default)]
pub(crate) struct StatHistory {
    pub cpu: VecDeque<f64>,
    pub mem: VecDeque<f64>,
}

impl StatHistory {
    fn push(&mut self, cpu: f64, mem: f64) {
        self.cpu.push_back(cpu);
        self.mem.push_back(mem);
        while self.cpu.len() > MAX_HISTORY {
            self.cpu.pop_front();
        }
        while self.mem.len() > MAX_HISTORY {
            self.mem.pop_front();
        }
    }
}

/// A pending destructive action awaiting confirmation.
pub(crate) struct ConfirmState {
    pub message: String,
    pub action: Option<Action>,
}

/// State for the full "create / run container" wizard.
#[derive(Default)]
pub(crate) struct RunDialog {
    pub image: String,
    pub name: String,
    pub ports: String,
    pub env: String,
    pub volumes: String,
    pub network: String,
    pub workdir: String,
    pub user: String,
    pub hostname: String,
    pub memory: String,
    pub cpus: String,
    pub entrypoint: String,
    pub command: String,
    pub detach: bool,
    pub auto_remove: bool,
    pub publish_all: bool,
}

impl RunDialog {
    pub fn with_image(image: String) -> Self {
        Self {
            image,
            detach: true,
            ..Default::default()
        }
    }

    /// Split a comma/space-separated field into trimmed, non-empty tokens.
    fn list(field: &str) -> Vec<String> {
        field
            .split([',', '\n'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn to_spec(&self) -> RunSpec {
        RunSpec {
            image: self.image.trim().to_string(),
            name: self.name.trim().to_string(),
            ports: Self::list(&self.ports),
            env: Self::list(&self.env),
            volumes: Self::list(&self.volumes),
            network: self.network.trim().to_string(),
            workdir: self.workdir.trim().to_string(),
            user: self.user.trim().to_string(),
            hostname: self.hostname.trim().to_string(),
            memory: self.memory.trim().to_string(),
            cpus: self.cpus.trim().to_string(),
            entrypoint: self.entrypoint.trim().to_string(),
            command: self.command.trim().to_string(),
            detach: self.detach,
            auto_remove: self.auto_remove,
            publish_all: self.publish_all,
        }
    }
}

/// Small single-field dialogs.
#[derive(Default)]
pub(crate) struct VolumeDialog {
    pub name: String,
    pub driver: String,
}

#[derive(Default)]
pub(crate) struct NetworkDialog {
    pub name: String,
}

#[derive(Default)]
pub(crate) struct PullDialog {
    pub reference: String,
}

/// Add / edit form for a [`SavedCommand`]. `editing` is the index being edited,
/// or `None` when creating a new entry.
#[derive(Default)]
pub(crate) struct CommandDialog {
    pub editing: Option<usize>,
    pub name: String,
    pub description: String,
    pub command: String,
}

/// A transient status message.
pub(crate) struct Toast {
    pub message: String,
    pub is_error: bool,
    pub created: Instant,
}

/// Which column the container table is currently sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ContainerSortKey {
    #[default]
    Name,
    Image,
    State,
    Cpu,
    Mem,
    MemPerc,
    NetIo,
    BlockIo,
    Pids,
    Created,
}

/// Which column the saved-commands table is currently sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CommandSortKey {
    #[default]
    Name,
    Description,
    Command,
}

pub struct WslcDesktopApp {
    pub(crate) worker: WorkerHandle,
    pub(crate) version: Option<String>,

    pub(crate) containers: Vec<Container>,
    pub(crate) images: Vec<Image>,
    pub(crate) volumes: Vec<Volume>,
    pub(crate) networks: Vec<Network>,
    pub(crate) stats_by_id: HashMap<String, Stat>,
    pub(crate) stats_history: HashMap<String, StatHistory>,

    pub(crate) section: Section,
    pub(crate) selected_container: Option<String>,
    pub(crate) selected_image: Option<String>,
    pub(crate) selected_volume: Option<String>,
    pub(crate) selected_network: Option<String>,
    pub(crate) detail_tab: DetailTab,

    pub(crate) logs_text: String,
    pub(crate) logs_target: Option<String>,
    pub(crate) inspect_text: String,
    pub(crate) inspect_target: Option<String>,
    pub(crate) exec_command: String,
    pub(crate) exec_output: String,

    pub(crate) filter: String,
    pub(crate) settings: Settings,
    pub(crate) saved_commands: Vec<SavedCommand>,

    // Table sorting state (M2: clickable column headers).
    pub(crate) container_sort_key: ContainerSortKey,
    pub(crate) container_sort_asc: bool,
    pub(crate) command_sort_key: CommandSortKey,
    pub(crate) command_sort_asc: bool,

    /// Ensures the saved UI scale is applied exactly once, on the first
    /// `update()` frame (after egui knows the native/OS scale factor). See
    /// `new()` for why it must not be applied there.
    pub(crate) scale_initialized: bool,

    pub(crate) confirm: Option<ConfirmState>,
    pub(crate) run_dialog: Option<RunDialog>,
    pub(crate) volume_dialog: Option<VolumeDialog>,
    pub(crate) network_dialog: Option<NetworkDialog>,
    pub(crate) pull_dialog: Option<PullDialog>,
    pub(crate) command_dialog: Option<CommandDialog>,
    pub(crate) toast: Option<Toast>,

    pub(crate) snapshot_error: Option<String>,
    pub(crate) last_update: Option<Instant>,
}

impl WslcDesktopApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load persisted settings (falls back to defaults).
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, SETTINGS_KEY))
            .unwrap_or_default();

        // Load the saved-command library, seeding the ps1 presets on first run.
        let saved_commands: Vec<SavedCommand> = cc
            .storage
            .and_then(|s| eframe::get_value(s, SAVED_COMMANDS_KEY))
            .unwrap_or_else(default_saved_commands);

        // Register a bundled CJK font so Chinese command descriptions and any
        // other non-Latin UI text render instead of tofu boxes (□).
        install_fonts(&cc.egui_ctx);

        // Slightly larger default text for a desktop dashboard feel.
        let mut style = (*cc.egui_ctx.style()).clone();
        for font_id in style.text_styles.values_mut() {
            font_id.size *= 1.05;
        }
        cc.egui_ctx.set_style(style);
        apply_theme(&cc.egui_ctx, settings.dark_mode);
        // NOTE: the saved UI scale is applied in `update()` on the first frame
        // (see `scale_initialized`), NOT here. Calling `set_pixels_per_point`
        // during `new()` runs before egui knows the OS/native scale factor, so
        // it mis-computes the zoom and compounds the native scale once the first
        // frame arrives (startup looked ~2.25x on a 150% display instead of the
        // intended 1.5x). Applying on the first `update()` frame, where
        // `native_pixels_per_point` is already known, makes startup and
        // drag-to-rescale behave identically.

        let worker = poller::spawn(cc.egui_ctx.clone());
        let _ = worker
            .tx
            .send(UiRequest::SetAutoRefresh(settings.auto_refresh));

        Self {
            worker,
            version: None,
            containers: Vec::new(),
            images: Vec::new(),
            volumes: Vec::new(),
            networks: Vec::new(),
            stats_by_id: HashMap::new(),
            stats_history: HashMap::new(),
            section: Section::Containers,
            selected_container: None,
            selected_image: None,
            selected_volume: None,
            selected_network: None,
            detail_tab: DetailTab::Logs,
            logs_text: String::new(),
            logs_target: None,
            inspect_text: String::new(),
            inspect_target: None,
            exec_command: String::new(),
            exec_output: String::new(),
            filter: String::new(),
            settings,
            saved_commands,
            container_sort_key: ContainerSortKey::default(),
            container_sort_asc: true,
            command_sort_key: CommandSortKey::default(),
            command_sort_asc: true,
            scale_initialized: false,
            confirm: None,
            run_dialog: None,
            volume_dialog: None,
            network_dialog: None,
            pull_dialog: None,
            command_dialog: None,
            toast: None,
            snapshot_error: None,
            last_update: None,
        }
    }

    /// Send a request to the worker thread.
    pub(crate) fn send(&self, request: UiRequest) {
        let _ = self.worker.tx.send(request);
    }

    /// Show a transient status message.
    pub(crate) fn show_toast(&mut self, message: impl Into<String>, is_error: bool) {
        self.toast = Some(Toast {
            message: message.into(),
            is_error,
            created: Instant::now(),
        });
    }

    /// Queue a destructive action behind a confirmation dialog.
    pub(crate) fn confirm(&mut self, message: impl Into<String>, action: Action) {
        self.confirm = Some(ConfirmState {
            message: message.into(),
            action: Some(action),
        });
    }

    /// Toggle dark/light theme and apply immediately.
    pub(crate) fn toggle_theme(&mut self, ctx: &egui::Context) {
        self.settings.dark_mode = !self.settings.dark_mode;
        apply_theme(ctx, self.settings.dark_mode);
    }

    /// Copy text to the clipboard and toast.
    pub(crate) fn copy_to_clipboard(&mut self, ctx: &egui::Context, text: String) {
        ctx.copy_text(text.clone());
        self.show_toast(format!("copied: {text}"), false);
    }

    /// Drain all pending worker events into app state.
    fn drain_events(&mut self) {
        while let Ok(event) = self.worker.rx.try_recv() {
            match event {
                WorkerEvent::Version(result) => {
                    self.version = Some(match result {
                        Ok(v) => v,
                        Err(e) => format!("unavailable ({e})"),
                    });
                }
                WorkerEvent::Snapshot {
                    containers,
                    images,
                    volumes,
                    networks,
                } => {
                    self.containers = containers;
                    self.images = images;
                    self.volumes = volumes;
                    self.networks = networks;
                    self.snapshot_error = None;
                    self.last_update = Some(Instant::now());
                    self.ensure_valid_selection();
                }
                WorkerEvent::Stats(stats) => self.ingest_stats(stats),
                WorkerEvent::Logs { target, text } => {
                    if self.selected_container.as_deref() == Some(target.as_str())
                        || self.selected_container_target().as_deref() == Some(target.as_str())
                    {
                        self.logs_text = text.unwrap_or_else(|e| format!("logs error: {e}"));
                    }
                }
                WorkerEvent::LogReset { target } => {
                    if self.selected_container_target().as_deref() == Some(target.as_str()) {
                        self.logs_text.clear();
                    }
                }
                WorkerEvent::LogLine { target, line } => {
                    if self.selected_container_target().as_deref() == Some(target.as_str()) {
                        if !self.logs_text.is_empty() {
                            self.logs_text.push('\n');
                        }
                        self.logs_text.push_str(&line);
                    }
                }
                WorkerEvent::Inspect { target, text } => {
                    if self.current_inspect_target().as_deref() == Some(target.as_str()) {
                        self.inspect_text = text.unwrap_or_else(|e| format!("inspect error: {e}"));
                    }
                }
                WorkerEvent::Exec { target, text } => {
                    if self.selected_container_target().as_deref() == Some(target.as_str()) {
                        self.exec_output = text.unwrap_or_else(|e| format!("exec error: {e}"));
                    }
                }
                WorkerEvent::ActionDone { label, result } => match result {
                    Ok(_) => self.show_toast(format!("{label} ✓"), false),
                    Err(e) => self.show_toast(format!("{label} failed: {e}"), true),
                },
                WorkerEvent::SnapshotError(e) => {
                    self.snapshot_error = Some(e);
                }
            }
        }
    }

    fn ingest_stats(&mut self, stats: Vec<Stat>) {
        let mut seen = std::collections::HashSet::new();
        self.stats_by_id.clear();
        for stat in stats {
            // ≥3.x stats 的 ID 是 64 位完整 ID，统一截短后再作为键。
            let key = stats_key(&stat.id);
            seen.insert(key.clone());
            let cpu = stat.cpu_percent();
            let mem = stat.mem_used_bytes();
            self.stats_history
                .entry(key.clone())
                .or_default()
                .push(cpu, mem);
            self.stats_by_id.insert(key, stat);
        }
        // Drop history for containers that no longer report stats.
        self.stats_history.retain(|id, _| seen.contains(id));
    }

    /// Ensure the current selection still points at an existing item.
    fn ensure_valid_selection(&mut self) {
        if let Some(id) = &self.selected_container {
            if !self.containers.iter().any(|c| &c.id == id) {
                self.selected_container = None;
            }
        }
        if self.selected_container.is_none() {
            self.selected_container = self.containers.first().map(|c| c.id.clone());
        }
        if let Some(reference) = &self.selected_image {
            if !self.images.iter().any(|i| &i.reference() == reference) {
                self.selected_image = None;
            }
        }
        if let Some(name) = &self.selected_volume {
            if !self.volumes.iter().any(|v| &v.name == name) {
                self.selected_volume = None;
            }
        }
        if let Some(name) = &self.selected_network {
            if !self.networks.iter().any(|n| &n.display_name() == name) {
                self.selected_network = None;
            }
        }
    }

    /// The target (name or id) of the selected container, if any.
    pub(crate) fn selected_container_target(&self) -> Option<String> {
        let id = self.selected_container.as_ref()?;
        self.containers
            .iter()
            .find(|c| &c.id == id)
            .map(Container::target)
    }

    /// Whether the selected container is currently running.
    pub(crate) fn selected_container_running(&self) -> bool {
        self.selected_container
            .as_ref()
            .and_then(|id| self.containers.iter().find(|c| &c.id == id))
            .map(|c| c.state().is_running())
            .unwrap_or(false)
    }

    /// The inspect target depends on which section is active.
    fn current_inspect_target(&self) -> Option<String> {
        match self.section {
            Section::Containers => self.selected_container_target(),
            Section::Images => self.selected_image.clone(),
            Section::Volumes => self.selected_volume.clone(),
            Section::Networks => self.selected_network.clone(),
            Section::Commands => None,
        }
    }

    /// Fetch detail data lazily when the selection/tab changes.
    fn sync_detail_requests(&mut self) {
        // Only containers support Logs/Stats/Exec; other sections show Inspect.
        if self.section != Section::Containers
            && matches!(
                self.detail_tab,
                DetailTab::Logs | DetailTab::Stats | DetailTab::Exec
            )
        {
            self.detail_tab = DetailTab::Inspect;
        }

        match self.detail_tab {
            DetailTab::Logs => {
                if self.section == Section::Containers {
                    if let Some(target) = self.selected_container_target() {
                        if self.logs_target.as_deref() != Some(target.as_str()) {
                            self.logs_target = Some(target.clone());
                            self.logs_text = "loading logs…".to_string();
                            if self.settings.log_follow {
                                self.send(UiRequest::StartLogFollow {
                                    target,
                                    timestamps: self.settings.log_timestamps,
                                });
                            } else {
                                self.send(UiRequest::StopLogFollow);
                                self.send(UiRequest::Logs {
                                    target,
                                    timestamps: self.settings.log_timestamps,
                                });
                            }
                        }
                    }
                }
            }
            DetailTab::Inspect => {
                if let Some(target) = self.current_inspect_target() {
                    if self.inspect_target.as_deref() != Some(target.as_str()) {
                        self.inspect_target = Some(target.clone());
                        self.inspect_text = "loading…".to_string();
                        self.send(UiRequest::Inspect { target });
                    }
                }
            }
            DetailTab::Stats | DetailTab::Exec => {}
        }

        // Stop following logs once when the Logs tab stops being the active view.
        if self.detail_tab != DetailTab::Logs && self.logs_target.is_some() {
            self.logs_target = None;
            self.send(UiRequest::StopLogFollow);
        }
    }

    /// Reload the Logs view for the current selection using current follow/ts.
    pub(crate) fn reload_logs(&mut self) {
        self.logs_target = None; // force sync_detail_requests to refetch
    }

    pub(crate) fn running_count(&self) -> usize {
        self.containers
            .iter()
            .filter(|c| c.state().is_running())
            .count()
    }

    /// Re-import the `wslc-menu.ps1` presets, appending only those whose name is
    /// not already in the library (user-added and edited entries are preserved).
    pub(crate) fn restore_command_presets(&mut self) {
        let existing: std::collections::HashSet<String> =
            self.saved_commands.iter().map(|c| c.name.clone()).collect();
        let mut added = 0usize;
        for preset in default_saved_commands() {
            if !existing.contains(&preset.name) {
                self.saved_commands.push(preset);
                added += 1;
            }
        }
        if added == 0 {
            self.show_toast("all presets already present", false);
        } else {
            self.show_toast(format!("restored {added} preset command(s)"), false);
        }
    }
}

impl eframe::App for WslcDesktopApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply the saved UI scale once, on the first frame. By now egui has
        // received the native/OS scale factor, so `set_pixels_per_point` computes
        // the zoom correctly and startup scaling matches a later drag-to-rescale.
        if !self.scale_initialized {
            ctx.set_pixels_per_point(self.settings.ui_scale);
            self.scale_initialized = true;
        }

        self.drain_events();

        // Global keyboard shortcuts.
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.send(UiRequest::RefreshNow);
        }

        self.sync_detail_requests();

        self.top_bar(ctx);
        self.sidebar(ctx);
        self.status_bar(ctx);
        // The saved-commands library has no per-item detail; give it the full pane.
        if self.section != Section::Commands {
            self.detail_panel(ctx);
        }
        self.central_table(ctx);

        self.draw_confirm(ctx);
        self.draw_run_dialog(ctx);
        self.draw_volume_dialog(ctx);
        self.draw_network_dialog(ctx);
        self.draw_pull_dialog(ctx);
        self.draw_command_dialog(ctx);
        self.draw_toast(ctx);

        // Keep the UI live even without input so polling data flows in.
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_KEY, &self.settings);
        eframe::set_value(storage, SAVED_COMMANDS_KEY, &self.saved_commands);
    }
}

impl Drop for WslcDesktopApp {
    fn drop(&mut self) {
        // Ask the worker to stop any log stream and exit cleanly.
        let _ = self.worker.tx.send(UiRequest::Shutdown);
    }
}

/// Register the bundled Noto Sans SC subset so CJK text renders correctly.
///
/// egui's default fonts cover only Latin/symbol glyphs, so Chinese would show
/// as tofu boxes (□). We append our subset as a fallback on both the
/// proportional and monospace families: Latin keeps egui's crisp default face,
/// and any Chinese codepoint falls through to Noto Sans SC.
fn install_fonts(ctx: &egui::Context) {
    const CJK_FONT: &[u8] = include_bytes!("../assets/fonts/NotoSansSC-Subset.otf");

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto_sans_sc".to_owned(),
        egui::FontData::from_static(CJK_FONT),
    );

    // Append (not prepend) so ASCII still uses egui's default proportional font;
    // CJK codepoints are picked up from our fallback.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("noto_sans_sc".to_owned());
    }

    ctx.set_fonts(fonts);
}

/// Apply the egui dark or light visuals.
fn apply_theme(ctx: &egui::Context, dark: bool) {
    ctx.set_visuals(if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    });
}

/// The seed library imported from `wslc-menu.ps1`: 27 homelab containers, each a
/// full `wslc run …` command line ready to run or edit.
fn default_saved_commands() -> Vec<SavedCommand> {
    let sc = |name: &str, description: &str, command: &str| SavedCommand {
        name: name.to_string(),
        description: description.to_string(),
        command: command.to_string(),
    };
    vec![
        sc(
            "qinglong",
            "青龙面板",
            r#"wslc run -d -v "C:\docker\qinglong\data:/ql/data" -p 8383:5700 -e QlBaseUrl="/" -e QlPort="5700" --name qinglong -h qinglong docker.1panel.live/whyour/qinglong:latest"#,
        ),
        sc(
            "cookiecloud",
            "Cookie Cloud",
            r#"wslc run -d -p 8082:8088 --name cookiecloud -e API_ROOT=/cookie docker.1panel.live/easychen/cookiecloud:latest"#,
        ),
        sc(
            "cross-seed",
            "Cross-Seed 辅种",
            r#"wslc run -d --name cross-seed -p 2468:2468 -v "C:\docker\cross-seed\config:/config" docker.1panel.live/crossseed/cross-seed:latest daemon"#,
        ),
        sc(
            "network-panel",
            "Net Panel 刷流",
            r#"wslc run -d --name network-panel -p 8080:80 docker.1panel.live/netart/network-panel:latest"#,
        ),
        sc(
            "openspeedtest",
            "OpenSpeedTest 测速",
            r#"wslc run --name openspeedtest -d -p 4000:3000 -p 4001:3001 docker.1panel.live/openspeedtest/latest"#,
        ),
        sc(
            "bili_tool_web",
            "B站工具箱",
            r#"wslc run -d --name bili_tool_web -t -v "C:\docker\bili_tool_web\Logs:/app/Logs" -v "C:\docker\bili_tool_web\config:/app/config" -p 2233:8080 -e TZ=Asia/Shanghai -e DailyTaskConfig__Cron="0 0 15 * * ?" ghcr.nju.edu.cn/raywangqvq/bili_tool_web"#,
        ),
        sc(
            "jackett",
            "Jackett 索引器",
            r#"wslc run -d --name jackett -e PUID=1000 -e PGID=1000 -e TZ=Etc/UTC -e AUTO_UPDATE=true -p 9117:9117 -v "C:\docker\jackett\config:/config" -v "C:\docker\jackett\downloads:/downloads" -v "C:\docker\mi-gpt\resolv.conf:/etc/resolv.conf" docker.1panel.live/linuxserver/jackett:latest"#,
        ),
        sc(
            "home-assistant",
            "Home Assistant",
            r#"wslc run -d --name home-assistant -e TZ=Asia/Shanghai -v "C:\docker\home_assistant\config:/config" -p 8123:8123 docker.1panel.live/homeassistant/home-assistant"#,
        ),
        sc(
            "IYUUPlus",
            "IYUUPlus 辅种",
            r#"wslc run -d -v "C:\docker\IYUU\db:/IYUU/db" -v "C:\Users\Schal\AppData\Local\qBittorrent\BT_backup:/BT_backup" -p 8787:8787 --name IYUUPlus docker.1panel.live/iyuucn/iyuuplus"#,
        ),
        sc(
            "IYUUPlus-dev",
            "IYUUPlus 开发版",
            r#"wslc run -itd -v "C:\docker\iyuu-dev\iyuu:/iyuu" -v "C:\docker\iyuu-dev\data:/data" -p 8780:8780 --name IYUUPlus-dev docker.1panel.live/iyuucn/iyuuplus-dev:latest"#,
        ),
        sc(
            "elmmb",
            "饿了么点赞",
            r#"wslc run -id --name elmmb -h elmmb -p 3002:3000 -v "C:\docker\elmmb:/etc/lb/Config" docker.1panel.live/luobook/elmmb:latest"#,
        ),
        sc(
            "github-rss-aggregator",
            "GitHub RSS 聚合",
            r#"wslc run -d --name github-rss-aggregator --network bridge -p 5000:5000 -e TZ=Asia/Shanghai -e http_proxy=http://<proxy-host>:7890 -e https_proxy=http://<proxy-host>:7890 -e all_proxy=http://<proxy-host>:7890 -v "C:\docker\github-rss-aggregator:/app" -w /app docker.1panel.live/library/python:3.11 bash -c "apt-get update && apt-get install -y git curl && rm -rf /tmp/repo && git clone https://github.com/NOwin111/GitHub-RSS-Aggregator.git /tmp/repo && cp -r /tmp/repo/* /app/ && pip install flask feedparser requests && python github_rss_aggregator.py""#,
        ),
        sc(
            "qdtoday",
            "QD今日签到",
            r#"wslc run -d -p 8923:80 -v "C:\docker\qdtoday\config:/usr/src/app/config" --name qdtoday docker.1panel.live/qdtoday/qd"#,
        ),
        sc(
            "quark-auto-save",
            "夸克自动转存",
            r#"wslc run -d -p 5005:5005 -e WEBUI_USERNAME=admin -e WEBUI_PASSWORD=<your-password> -v "C:\docker\quark-auto-save\config:/app/config" -v "C:\docker\quark-auto-save\media:/media" --name quark-auto-save registry.cn-shenzhen.aliyuncs.com/cp0204/quark-auto-save:latest"#,
        ),
        sc(
            "rabbitpro",
            "RabbitPro",
            r#"wslc run --name rabbitpro -p 5701:1234 -d -v "C:\docker\rabbit\data:/Rabbit/data" -it docker.1panel.live/ht944/rabbitpro:latest"#,
        ),
        sc(
            "peerbanhelper",
            "PeerBanHelper 封禁",
            r#"wslc run -d --name peerbanhelper -p 9898:9898 -v "C:\docker\peerbanhelper:/app/data/" registry.cn-hangzhou.aliyuncs.com/ghostchu/peerbanhelper"#,
        ),
        sc(
            "postgresql_mp",
            "PostgreSQL (MoviePilot)",
            r#"wslc run -d --name postgresql_mp -p 5433:5432 -e POSTGRES_DB=moviepilot -e POSTGRES_USER=moviepilot -e POSTGRES_PASSWORD="<your-password>" -v "C:\docker\postgresql_mp:/var/lib/postgresql" docker.1panel.live/library/postgres"#,
        ),
        sc(
            "redis_mp",
            "Redis (MoviePilot)",
            r#"wslc run --name redis_mp -p 6379:6379 -v "C:\docker\redis\data:/data" -d docker.1panel.live/library/redis redis-server --save 600 1 --requirepass "<your-password>""#,
        ),
        sc(
            "smartdns",
            "SmartDNS",
            r#"wslc run -d --name smartdns --network host -p 9053:9053/udp -p 6080:6080 -v "C:\docker\smartdns\data\etc\smartdns:/etc/smartdns" -v "C:\docker\smartdns\data\var\lib\smartdns:/var/lib/smartdns" -v "C:\docker\smartdns\data\var\log\smartdns:/var/log/smartdns" docker.1panel.live/pymumu/smartdns:latest"#,
        ),
        sc(
            "pt-accelerator",
            "PT加速器",
            r#"wslc run -d --name pt-accelerator --network host -v "C:\Windows\System32\drivers\etc\hosts:/etc/hosts" -v "C:\docker\PT-Accelerator\config:/app/config" -v "C:\docker\PT-Accelerator\logs:/app/logs" -e TZ=Asia/Shanghai docker.1panel.live/eternalcurse/pt-accelerator:latest"#,
        ),
        sc(
            "seedcross",
            "SeedCross",
            r#"wslc run -d --name seedcross --network host -v "C:\docker\seedcross\db:/code/seedcross\db" -p 8019:8019 docker.1panel.live/ccf2012/seedcross:latest"#,
        ),
        sc(
            "seedhound",
            "ReseedHound 自动补种",
            r#"wslc run -d --name seedhound -e SEEDHOUND_MODE=schedule -v "C:\docker\seedhound:/app" ghcr.io/schalkiii/reseedhound:latest"#,
        ),
        sc(
            "mtranserver",
            "MT翻译服务",
            r#"wslc run -d --name mtranserver -p 8989:8989 -v "C:\docker\mtranserver\models:/app/models" -v "C:\docker\mtranserver\config.ini:/app/config.ini" docker.1panel.live/xxnuo/mtranserver:latest"#,
        ),
        sc(
            "pansou-app",
            "Pansou 网盘搜索",
            r#"wslc run -d --name pansou-app -p 8111:80 -e DOMAIN=localhost -e PANSOU_PORT=8888 -e PANSOU_HOST=127.0.0.1 -e SOCKS5_PROXY=socks5://<proxy-host>:7890 -e HTTP_PROXY=http://<proxy-host>:7890 -e HTTPS_PROXY=https://<proxy-host>:7890 -v "pansou-data:/app/data" -v "pansou-logs:/app/logs" ghcr.nju.edu.cn/fish2018/pansou-web:latest"#,
        ),
        sc(
            "Reseed-Puppy-Dev",
            "Reseed Puppy Dev",
            r#"wslc run -d --name Reseed-Puppy-Dev -v "C:\docker\reseed-puppy-dev\database:/reseed-puppy/database" -v "C:\CommonTools\qBittorrent_4.6.7_portable\Profile\qBittorrent\data\BT_backup:/reseed-puppy/public/qb" -p 8091:1997 docker.1panel.live/szzhoubanxian/reseed-puppy:dev"#,
        ),
        sc(
            "reseed-puppy",
            "Reseed Puppy PHP",
            r#"wslc run -d --name reseed-puppy -v "C:\docker\reseed-puppy-php\database:/reseed-puppy-php/database" -v "C:\CommonTools\qBittorrent_4.6.7_portable\Profile\qBittorrent\data\BT_backup:/reseed-puppy-php/public/torrents" -p 8081:1919 docker.1panel.live/szzhoubanxian/reseed-puppy:latest"#,
        ),
        sc(
            "pt-invite-watcher",
            "PT邀请监控",
            r#"wslc run -d --name pt-invite-watcher -p 8003:8080 -v "C:\docker\pt_invite_watcher\data:/data" -e PTIW_DB_PATH="/data/ptiw.db" docker.1panel.live/helloworldz1024/pt-invite-watcher:latest"#,
        ),
    ]
}

/// Color for a container state badge.
pub(crate) fn state_color(state: crate::wslc::ContainerState) -> Color32 {
    use crate::wslc::ContainerState::*;
    match state {
        Running => Color32::from_rgb(0x3f, 0xb9, 0x50), // green
        Exited => Color32::from_rgb(0x9a, 0x9a, 0x9a),  // grey
        Paused => Color32::from_rgb(0xe0, 0xa0, 0x30),  // amber
        Created => Color32::from_rgb(0x4a, 0x9e, 0xff), // blue
        Unknown(_) => Color32::from_rgb(0xc0, 0x50, 0x50),
    }
}

#[cfg(test)]
mod tests {
    use ab_glyph::{Font, FontRef};

    /// The bundled CJK subset must parse with egui's rasterizer (ab_glyph) and
    /// actually contain the Chinese glyphs we use, otherwise Chinese text would
    /// silently fall back to tofu boxes at runtime.
    #[test]
    fn bundled_cjk_font_has_chinese_glyphs() {
        const CJK_FONT: &[u8] = include_bytes!("../assets/fonts/NotoSansSC-Subset.otf");
        let font = FontRef::try_from_slice(CJK_FONT).expect("bundled CJK font should parse");
        for ch in ['青', '龙', '面', '板', '镜', '像', '容', '器'] {
            assert_ne!(
                font.glyph_id(ch).0,
                0,
                "font is missing glyph for {ch:?} (.notdef)"
            );
        }
        // ASCII must still be present for command lines / paths.
        assert_ne!(font.glyph_id('A').0, 0);

        // Geometric-shape sort arrows + the start/run glyph are bundled in the
        // CJK font itself (Noto Sans SC), so they must be present; otherwise
        // they would fall back to tofu on a host lacking the emoji font.
        for ch in ['▲', '▼', '▶'] {
            assert_ne!(
                font.glyph_id(ch).0,
                0,
                "font is missing UI glyph {ch:?} (.notdef)"
            );
        }
    }
}
