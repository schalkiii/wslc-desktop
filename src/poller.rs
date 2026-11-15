//! Background worker that talks to `wslc` off the UI thread.
//!
//! The egui UI thread must never block. All wslc calls run on a dedicated
//! thread. The UI sends [`UiRequest`]s and receives [`WorkerEvent`]s over
//! channels; the worker polls the snapshot on a fixed cadence and pumps any
//! active `logs --follow` stream.
//!
//! Long-running requests (pulls, foreground runs, lifecycle actions, execs)
//! go to a single serialized "slow lane" thread instead of the polling thread:
//! a 600 s image pull must not freeze the 2 s snapshot refresh. Only one slow
//! request runs at a time, maintaining the previous action-ordering semantics.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crate::wslc::{
    Container, Image, LogOptions, LogStream, Network, RunSpec, Stat, Volume, WslcClient,
};

/// Snapshot poll cadence (containers + images + volumes + networks + stats).
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Default number of log lines to tail.
pub const LOG_TAIL: u32 = 500;

/// Requests sent from the UI thread to the worker.
pub enum UiRequest {
    /// Force an immediate snapshot refresh.
    RefreshNow,
    /// Enable or disable periodic auto-refresh.
    SetAutoRefresh(bool),
    /// Window visibility and login-autostart activity; controls background
    /// throttling so a hidden window stops spawning wslc processes.
    SetPollingContext {
        window_visible: bool,
        autostart_active: bool,
    },
    /// A container/image/volume/network lifecycle action.
    Action(Action),
    /// One-shot log tail for a container target.
    Logs { target: String, timestamps: bool },
    /// Begin a live `logs --follow` stream for a target.
    StartLogFollow { target: String, timestamps: bool },
    /// Stop any active log stream.
    StopLogFollow,
    /// Fetch `wslc inspect` text for a target.
    Inspect { target: String },
    /// Execute a command inside a running container.
    Exec {
        target: String,
        command: String,
        workdir: String,
        user: String,
    },
    /// Run a new container from a full spec (boxed to keep the enum small).
    Run(Box<RunSpec>),
    /// Run a raw, saved `wslc …` command line (from the command library).
    RunRawCommand { label: String, command: String },
    /// Pull an image by reference.
    Pull { reference: String },
    /// Create a named volume.
    CreateVolume { name: String, driver: String },
    /// Create a network.
    CreateNetwork { name: String },
    /// Ask the worker to exit.
    Shutdown,
}

/// A lifecycle action with a human label for feedback.
pub enum Action {
    Start(String),
    Stop(String),
    Restart(String),
    Kill(String),
    RemoveContainer(String),
    PruneContainers,
    RemoveImage(String),
    PruneImages,
    RemoveVolume(String),
    PruneVolumes,
    RemoveNetwork(String),
    PruneNetworks,
}

/// Events sent from the worker back to the UI thread.
pub enum WorkerEvent {
    /// wslc version string (or an error message).
    Version(Result<String, String>),
    /// A fresh snapshot of all resources.
    Snapshot {
        containers: Vec<Container>,
        images: Vec<Image>,
        volumes: Vec<Volume>,
        networks: Vec<Network>,
    },
    /// A fresh stats sample.
    Stats(Vec<Stat>),
    /// One-shot logs for a given target.
    Logs {
        target: String,
        text: Result<String, String>,
    },
    /// A follow stream (re)started; UI should clear its buffer for `target`.
    LogReset { target: String },
    /// One appended line from an active follow stream.
    LogLine { target: String, line: String },
    /// Inspect JSON for a given target.
    Inspect {
        target: String,
        text: Result<String, String>,
    },
    /// Output of an `exec` invocation.
    Exec {
        target: String,
        text: Result<String, String>,
    },
    /// An action finished; carries a label and result for a toast.
    ActionDone {
        label: String,
        result: Result<String, String>,
    },
    /// A snapshot poll failed (e.g. wslc missing).
    SnapshotError(String),
}

/// Handle held by the UI thread.
pub struct WorkerHandle {
    pub tx: Sender<UiRequest>,
    pub rx: Receiver<WorkerEvent>,
}

/// Whether a request may block the polling thread for a long time.
///
/// `pull` / `run` use the 600 s timeout and lifecycle actions may boot the
/// WSL VM, so all of them are dispatched to [`SlowLane`]. Control requests
/// (refresh, auto-refresh, log-follow lifecycle, shutdown) stay synchronous.
fn is_slow_request(request: &UiRequest) -> bool {
    matches!(
        request,
        UiRequest::Action(_)
            | UiRequest::Logs { .. }
            | UiRequest::Inspect { .. }
            | UiRequest::Exec { .. }
            | UiRequest::Run(_)
            | UiRequest::RunRawCommand { .. }
            | UiRequest::Pull { .. }
            | UiRequest::CreateVolume { .. }
            | UiRequest::CreateNetwork { .. }
    )
}

/// Whether periodic snapshot/stats polling should run.
///
/// A hidden window has no observer, so polling it is pure overhead. The
/// login-autostart sequence is the exception: it needs fresh snapshots to
/// retire containers and decide when the app may exit.
fn should_poll(window_visible: bool, autostart_active: bool) -> bool {
    window_visible || autostart_active
}

/// FIFO queue for slow requests. `take_next` refuses to hand out work while
/// the lane is busy, which keeps actions serialized.
#[derive(Default)]
struct SlowQueue {
    pending: VecDeque<UiRequest>,
}

impl SlowQueue {
    fn push(&mut self, request: UiRequest) {
        self.pending.push_back(request);
    }

    fn take_next(&mut self, busy: bool) -> Option<UiRequest> {
        if busy {
            return None;
        }
        self.pending.pop_front()
    }
}

/// One-at-a-time lane for long-running wslc calls.
struct SlowLane {
    client: WslcClient,
    tx: Sender<WorkerEvent>,
    ctx: egui::Context,
    done_tx: Sender<()>,
    done_rx: Receiver<()>,
    busy: bool,
    queue: SlowQueue,
}

impl SlowLane {
    fn new(client: WslcClient, tx: Sender<WorkerEvent>, ctx: egui::Context) -> Self {
        let (done_tx, done_rx) = mpsc::channel();
        Self {
            client,
            tx,
            ctx,
            done_tx,
            done_rx,
            busy: false,
            queue: SlowQueue::default(),
        }
    }

    /// Queue a request and start it when the lane is idle.
    fn submit(&mut self, request: UiRequest) {
        self.queue.push(request);
        self.start_next();
    }

    fn start_next(&mut self) {
        let Some(request) = self.queue.take_next(self.busy) else {
            return;
        };
        self.busy = true;
        let client = self.client.clone();
        let tx = self.tx.clone();
        let done_tx = self.done_tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            run_slow_request(&client, &tx, request);
            // The polling thread refreshes the snapshot on this signal; the
            // UI is repainted both here and there so a toast cannot be missed.
            let _ = done_tx.send(());
            ctx.request_repaint();
        });
    }

    /// Drain completion signals; returns true when at least one request
    /// finished (the caller then refreshes the snapshot on the polling thread).
    fn drain_done(&mut self) -> bool {
        let mut any = false;
        while self.done_rx.try_recv().is_ok() {
            self.busy = false;
            any = true;
        }
        if any {
            self.start_next();
        }
        any
    }
}

/// Execute one slow request, forwarding its result to the UI. Runs on the
/// slow-lane thread; it must not touch shared worker state.
fn run_slow_request(client: &WslcClient, tx: &Sender<WorkerEvent>, request: UiRequest) {
    match request {
        UiRequest::Logs { target, timestamps } => {
            let opts = LogOptions {
                tail: LOG_TAIL,
                timestamps,
            };
            let text = client.logs(&target, &opts).map_err(|e| e.to_string());
            let _ = tx.send(WorkerEvent::Logs { target, text });
        }
        UiRequest::Inspect { target } => {
            let text = client
                .inspect(&target)
                .map(|raw| pretty_json(&raw))
                .map_err(|e| e.to_string());
            let _ = tx.send(WorkerEvent::Inspect { target, text });
        }
        UiRequest::Exec {
            target,
            command,
            workdir,
            user,
        } => {
            let text = client
                .exec(&target, &command, &workdir, &user)
                .map_err(|e| e.to_string());
            let _ = tx.send(WorkerEvent::Exec { target, text });
        }
        UiRequest::Run(spec) => {
            let label = format!("run {}", spec.image);
            let result = client.run_container(&spec).map_err(|e| e.to_string());
            let _ = tx.send(WorkerEvent::ActionDone { label, result });
        }
        UiRequest::RunRawCommand { label, command } => {
            let result = client.run_raw(&command).map_err(|e| e.to_string());
            let _ = tx.send(WorkerEvent::ActionDone { label, result });
        }
        UiRequest::Pull { reference } => {
            let label = format!("pull {reference}");
            let result = client.pull_image(&reference).map_err(|e| e.to_string());
            let _ = tx.send(WorkerEvent::ActionDone { label, result });
        }
        UiRequest::CreateVolume { name, driver } => {
            let label = format!("volume create {name}");
            let result = client
                .create_volume(&name, &driver)
                .map_err(|e| e.to_string());
            let _ = tx.send(WorkerEvent::ActionDone { label, result });
        }
        UiRequest::CreateNetwork { name } => {
            let label = format!("network create {name}");
            let result = client.create_network(&name).map_err(|e| e.to_string());
            let _ = tx.send(WorkerEvent::ActionDone { label, result });
        }
        UiRequest::Action(action) => {
            let (label, result) = run_action(client, action);
            let _ = tx.send(WorkerEvent::ActionDone { label, result });
        }
        // Control requests are handled synchronously by `Worker` and never
        // reach the slow lane (see `is_slow_request`).
        UiRequest::RefreshNow
        | UiRequest::SetAutoRefresh(_)
        | UiRequest::SetPollingContext { .. }
        | UiRequest::StartLogFollow { .. }
        | UiRequest::StopLogFollow
        | UiRequest::Shutdown => {}
    }
}

/// Mutable worker state that lives across the loop.
struct Worker {
    client: WslcClient,
    tx: Sender<WorkerEvent>,
    auto_refresh: bool,
    /// Whether the window is currently visible to the user.
    window_visible: bool,
    /// Whether the login-autostart sequence still needs snapshots.
    autostart_active: bool,
    /// Timestamp of the last periodic poll.
    last_poll: Instant,
    /// Active log-follow stream and the target it belongs to.
    log_stream: Option<(String, LogStream)>,
    /// Long-running requests run off-thread so periodic polls keep flowing.
    slow: SlowLane,
}

/// Spawn the worker thread. `ctx` is used to wake the UI when data arrives.
pub fn spawn(ctx: egui::Context) -> WorkerHandle {
    let (ui_tx, worker_rx) = std::sync::mpsc::channel::<UiRequest>();
    let (worker_tx, ui_rx) = std::sync::mpsc::channel::<WorkerEvent>();

    std::thread::spawn(move || {
        let client = WslcClient::new();
        let mut worker = Worker {
            client: client.clone(),
            tx: worker_tx.clone(),
            auto_refresh: true,
            window_visible: true,
            autostart_active: false,
            last_poll: Instant::now(),
            log_stream: None,
            slow: SlowLane::new(client, worker_tx, ctx.clone()),
        };

        // Report version once at startup, then the first snapshot.
        let version = worker.client.version().map_err(|e| e.to_string());
        let _ = worker.tx.send(WorkerEvent::Version(version));
        worker.poll_snapshot();
        worker.poll_stats();
        ctx.request_repaint();

        loop {
            // Drain any pending UI requests without blocking.
            loop {
                match worker_rx.try_recv() {
                    Ok(UiRequest::Shutdown) | Err(TryRecvError::Disconnected) => {
                        worker.stop_log_stream();
                        return;
                    }
                    Ok(request) => {
                        if is_slow_request(&request) {
                            worker.slow.submit(request);
                        } else {
                            worker.handle_request(request);
                        }
                        ctx.request_repaint();
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }

            // A finished slow request invalidates the snapshot. Refresh on
            // this thread so the slow lane never touches worker state.
            if worker.slow.drain_done() {
                worker.poll_snapshot();
                worker.poll_stats();
                worker.last_poll = Instant::now();
                ctx.request_repaint();
            }

            // Pump the active log stream, if any.
            if worker.pump_log_stream() {
                ctx.request_repaint();
            }

            // Periodic snapshot + stats poll. While the window is hidden and
            // no login-autostart sequence is running, skip wslc entirely.
            // `update()` still requests a repaint every 500 ms, so tray events
            // (and the show-window path below) remain responsive.
            if worker.auto_refresh
                && should_poll(worker.window_visible, worker.autostart_active)
                && worker.last_poll.elapsed() >= POLL_INTERVAL
            {
                worker.poll_snapshot();
                worker.poll_stats();
                worker.last_poll = Instant::now();
                ctx.request_repaint();
            }

            std::thread::sleep(Duration::from_millis(80));
        }
    });

    WorkerHandle {
        tx: ui_tx,
        rx: ui_rx,
    }
}

impl Worker {
    fn poll_snapshot(&self) {
        match (
            self.client.list_containers(),
            self.client.list_images(),
            self.client.list_volumes(),
            self.client.list_networks(),
        ) {
            (Ok(containers), Ok(images), volumes, networks) => {
                let _ = self.tx.send(WorkerEvent::Snapshot {
                    containers,
                    images,
                    // 卷/网络尽力而为：单个资源的 schema 漂移只表现为空列表，
                    // 不拖垮容器/镜像这两个核心视图（否则一个 Err 会全灭）。
                    volumes: volumes.unwrap_or_default(),
                    networks: networks.unwrap_or_default(),
                });
            }
            (Err(e), _, _, _) | (_, Err(e), _, _) => {
                let _ = self.tx.send(WorkerEvent::SnapshotError(e.to_string()));
            }
        }
    }

    fn poll_stats(&self) {
        if let Ok(stats) = self.client.stats() {
            let _ = self.tx.send(WorkerEvent::Stats(stats));
        }
    }

    /// Forward any lines available from the active log stream. Returns true if
    /// at least one line was forwarded.
    fn pump_log_stream(&mut self) -> bool {
        let mut any = false;
        if let Some((target, stream)) = &self.log_stream {
            // Drain everything currently buffered.
            loop {
                match stream.rx.try_recv() {
                    Ok(line) => {
                        let _ = self.tx.send(WorkerEvent::LogLine {
                            target: target.clone(),
                            line,
                        });
                        any = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
        }
        any
    }

    fn stop_log_stream(&mut self) {
        if let Some((_, stream)) = self.log_stream.take() {
            stream.stop();
        }
    }

    /// Handle control requests synchronously. Slow requests are dispatched to
    /// [`SlowLane`] by `spawn()` and never reach this method.
    fn handle_request(&mut self, request: UiRequest) {
        match request {
            UiRequest::RefreshNow => {
                self.poll_snapshot();
                self.poll_stats();
            }
            UiRequest::SetAutoRefresh(on) => self.auto_refresh = on,
            UiRequest::SetPollingContext {
                window_visible,
                autostart_active,
            } => {
                let became_visible = window_visible && !self.window_visible;
                self.window_visible = window_visible;
                self.autostart_active = autostart_active;
                if became_visible {
                    // Data went stale while hidden; show fresh state at once.
                    self.poll_snapshot();
                    self.poll_stats();
                    self.last_poll = Instant::now();
                }
            }
            UiRequest::StartLogFollow { target, timestamps } => {
                self.stop_log_stream();
                match self.client.logs_follow(&target, LOG_TAIL, timestamps) {
                    Ok(stream) => {
                        let _ = self.tx.send(WorkerEvent::LogReset {
                            target: target.clone(),
                        });
                        self.log_stream = Some((target, stream));
                    }
                    Err(e) => {
                        let _ = self.tx.send(WorkerEvent::Logs {
                            target,
                            text: Err(e.to_string()),
                        });
                    }
                }
            }
            UiRequest::StopLogFollow => self.stop_log_stream(),
            // Slow requests are owned by `run_slow_request`; `Shutdown` is
            // intercepted by the loop before it can reach this method.
            UiRequest::Action(_)
            | UiRequest::Logs { .. }
            | UiRequest::Inspect { .. }
            | UiRequest::Exec { .. }
            | UiRequest::Run(_)
            | UiRequest::RunRawCommand { .. }
            | UiRequest::Pull { .. }
            | UiRequest::CreateVolume { .. }
            | UiRequest::CreateNetwork { .. }
            | UiRequest::Shutdown => {}
        }
    }
}

/// Map a lifecycle action to its wslc call and a human-readable label.
///
/// Kept as a free function taking `&WslcClient` so the slow lane can run it
/// without borrowing the polling thread's mutable worker state.
fn run_action(client: &WslcClient, action: Action) -> (String, Result<String, String>) {
    let unit = |r: anyhow::Result<()>| r.map(|_| String::new()).map_err(|e| e.to_string());
    match action {
        Action::Start(t) => (format!("start {t}"), unit(client.start_container(&t))),
        Action::Stop(t) => (format!("stop {t}"), unit(client.stop_container(&t))),
        Action::Restart(t) => (format!("restart {t}"), unit(client.restart_container(&t))),
        Action::Kill(t) => (format!("kill {t}"), unit(client.kill_container(&t))),
        Action::RemoveContainer(t) => (
            format!("remove {t}"),
            unit(client.remove_container(&t, true)),
        ),
        Action::PruneContainers => (
            "prune containers".to_string(),
            client.prune_containers().map_err(|e| e.to_string()),
        ),
        Action::RemoveImage(t) => (format!("rmi {t}"), unit(client.remove_image(&t, true))),
        Action::PruneImages => (
            "prune images".to_string(),
            client.prune_images().map_err(|e| e.to_string()),
        ),
        Action::RemoveVolume(t) => (format!("volume remove {t}"), unit(client.remove_volume(&t))),
        Action::PruneVolumes => (
            "prune volumes".to_string(),
            client.prune_volumes().map_err(|e| e.to_string()),
        ),
        Action::RemoveNetwork(t) => (
            format!("network remove {t}"),
            unit(client.remove_network(&t)),
        ),
        Action::PruneNetworks => (
            "prune networks".to_string(),
            client.prune_networks().map_err(|e| e.to_string()),
        ),
    }
}

/// Pretty-print raw JSON; if it doesn't parse, return it unchanged.
fn pretty_json(raw: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.to_string()),
        Err(_) => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control_requests() -> Vec<UiRequest> {
        vec![
            UiRequest::RefreshNow,
            UiRequest::SetAutoRefresh(true),
            UiRequest::SetPollingContext {
                window_visible: true,
                autostart_active: false,
            },
            UiRequest::StartLogFollow {
                target: "web".into(),
                timestamps: false,
            },
            UiRequest::StopLogFollow,
            UiRequest::Shutdown,
        ]
    }

    fn slow_requests() -> Vec<UiRequest> {
        vec![
            UiRequest::Action(Action::Start("web".into())),
            UiRequest::Logs {
                target: "web".into(),
                timestamps: false,
            },
            UiRequest::Inspect {
                target: "web".into(),
            },
            UiRequest::Exec {
                target: "web".into(),
                command: "ls".into(),
                workdir: String::new(),
                user: String::new(),
            },
            UiRequest::Run(Box::new(RunSpec {
                image: "alpine".into(),
                name: String::new(),
                ports: Vec::new(),
                env: Vec::new(),
                volumes: Vec::new(),
                network: String::new(),
                workdir: String::new(),
                user: String::new(),
                hostname: String::new(),
                memory: String::new(),
                cpus: String::new(),
                entrypoint: String::new(),
                command: String::new(),
                detach: true,
                auto_remove: false,
                publish_all: false,
            })),
            UiRequest::RunRawCommand {
                label: "ps".into(),
                command: "wslc ps".into(),
            },
            UiRequest::Pull {
                reference: "alpine".into(),
            },
            UiRequest::CreateVolume {
                name: "data".into(),
                driver: String::new(),
            },
            UiRequest::CreateNetwork { name: "net".into() },
        ]
    }

    #[test]
    fn slow_requests_are_classified_correctly() {
        for request in control_requests() {
            assert!(!is_slow_request(&request));
        }
        for request in slow_requests() {
            assert!(is_slow_request(&request));
        }
    }

    #[test]
    fn slow_queue_is_fifo_and_single_flight() {
        let mut queue = SlowQueue::default();
        assert!(queue.take_next(false).is_none());

        queue.push(UiRequest::Pull {
            reference: "a".into(),
        });
        queue.push(UiRequest::Pull {
            reference: "b".into(),
        });

        assert!(
            queue.take_next(true).is_none(),
            "a busy lane must not start another request"
        );
        assert!(matches!(
            queue.take_next(false),
            Some(UiRequest::Pull { reference }) if reference == "a"
        ));
        assert!(matches!(
            queue.take_next(false),
            Some(UiRequest::Pull { reference }) if reference == "b"
        ));
        assert!(queue.take_next(false).is_none());
    }

    #[test]
    fn polling_is_paused_only_for_hidden_inactive_windows() {
        assert!(should_poll(true, false));
        assert!(should_poll(true, true));
        assert!(
            should_poll(false, true),
            "login autostart still needs snapshots while hidden"
        );
        assert!(!should_poll(false, false));
    }
}
