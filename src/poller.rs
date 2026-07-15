//! Background worker that talks to `wslc` off the UI thread.
//!
//! The egui UI thread must never block. All wslc calls run here on a dedicated
//! thread. The UI sends [`UiRequest`]s and receives [`WorkerEvent`]s over
//! channels; the worker also polls the snapshot on a fixed cadence, pumps any
//! active `logs --follow` stream, and requests an egui repaint on fresh data.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
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

/// Mutable worker state that lives across the loop.
struct Worker {
    client: WslcClient,
    tx: Sender<WorkerEvent>,
    auto_refresh: bool,
    /// Active log-follow stream and the target it belongs to.
    log_stream: Option<(String, LogStream)>,
}

/// Spawn the worker thread. `ctx` is used to wake the UI when data arrives.
pub fn spawn(ctx: egui::Context) -> WorkerHandle {
    let (ui_tx, worker_rx) = std::sync::mpsc::channel::<UiRequest>();
    let (worker_tx, ui_rx) = std::sync::mpsc::channel::<WorkerEvent>();

    std::thread::spawn(move || {
        let mut worker = Worker {
            client: WslcClient::new(),
            tx: worker_tx,
            auto_refresh: true,
            log_stream: None,
        };

        // Report version once at startup, then the first snapshot.
        let version = worker.client.version().map_err(|e| e.to_string());
        let _ = worker.tx.send(WorkerEvent::Version(version));
        worker.poll_snapshot();
        worker.poll_stats();
        ctx.request_repaint();

        let mut last_poll = Instant::now();

        loop {
            // Drain any pending UI requests without blocking.
            loop {
                match worker_rx.try_recv() {
                    Ok(UiRequest::Shutdown) | Err(TryRecvError::Disconnected) => {
                        worker.stop_log_stream();
                        return;
                    }
                    Ok(request) => {
                        worker.handle_request(request);
                        ctx.request_repaint();
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }

            // Pump the active log stream, if any.
            if worker.pump_log_stream() {
                ctx.request_repaint();
            }

            // Periodic snapshot + stats poll.
            if worker.auto_refresh && last_poll.elapsed() >= POLL_INTERVAL {
                worker.poll_snapshot();
                worker.poll_stats();
                last_poll = Instant::now();
                ctx.request_repaint();
            }

            std::thread::sleep(Duration::from_millis(80));
        }
    });

    WorkerHandle { tx: ui_tx, rx: ui_rx }
}

impl Worker {
    fn poll_snapshot(&self) {
        match (
            self.client.list_containers(),
            self.client.list_images(),
            self.client.list_volumes(),
            self.client.list_networks(),
        ) {
            (Ok(containers), Ok(images), Ok(volumes), networks) => {
                let _ = self.tx.send(WorkerEvent::Snapshot {
                    containers,
                    images,
                    volumes,
                    // Networks are best-effort; an error yields an empty list.
                    networks: networks.unwrap_or_default(),
                });
            }
            (Err(e), _, _, _) | (_, Err(e), _, _) | (_, _, Err(e), _) => {
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

    fn handle_request(&mut self, request: UiRequest) {
        match request {
            UiRequest::RefreshNow => {
                self.poll_snapshot();
                self.poll_stats();
            }
            UiRequest::SetAutoRefresh(on) => self.auto_refresh = on,
            UiRequest::Logs { target, timestamps } => {
                let opts = LogOptions {
                    tail: LOG_TAIL,
                    timestamps,
                };
                let text = self.client.logs(&target, &opts).map_err(|e| e.to_string());
                let _ = self.tx.send(WorkerEvent::Logs { target, text });
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
            UiRequest::Inspect { target } => {
                let text = self
                    .client
                    .inspect(&target)
                    .map(|raw| pretty_json(&raw))
                    .map_err(|e| e.to_string());
                let _ = self.tx.send(WorkerEvent::Inspect { target, text });
            }
            UiRequest::Exec {
                target,
                command,
                workdir,
                user,
            } => {
                let text = self
                    .client
                    .exec(&target, &command, &workdir, &user)
                    .map_err(|e| e.to_string());
                let _ = self.tx.send(WorkerEvent::Exec { target, text });
            }
            UiRequest::Run(spec) => {
                let label = format!("run {}", spec.image);
                let result = self.client.run_container(&spec).map_err(|e| e.to_string());
                let _ = self.tx.send(WorkerEvent::ActionDone { label, result });
                self.poll_snapshot();
            }
            UiRequest::RunRawCommand { label, command } => {
                let result = self.client.run_raw(&command).map_err(|e| e.to_string());
                let _ = self.tx.send(WorkerEvent::ActionDone { label, result });
                self.poll_snapshot();
            }
            UiRequest::Pull { reference } => {
                let label = format!("pull {reference}");
                let result = self.client.pull_image(&reference).map_err(|e| e.to_string());
                let _ = self.tx.send(WorkerEvent::ActionDone { label, result });
                self.poll_snapshot();
            }
            UiRequest::CreateVolume { name, driver } => {
                let label = format!("volume create {name}");
                let result = self
                    .client
                    .create_volume(&name, &driver)
                    .map_err(|e| e.to_string());
                let _ = self.tx.send(WorkerEvent::ActionDone { label, result });
                self.poll_snapshot();
            }
            UiRequest::CreateNetwork { name } => {
                let label = format!("network create {name}");
                let result = self.client.create_network(&name).map_err(|e| e.to_string());
                let _ = self.tx.send(WorkerEvent::ActionDone { label, result });
                self.poll_snapshot();
            }
            UiRequest::Action(action) => {
                let (label, result) = self.run_action(action);
                let _ = self.tx.send(WorkerEvent::ActionDone { label, result });
                self.poll_snapshot();
                self.poll_stats();
            }
            UiRequest::Shutdown => {}
        }
    }

    fn run_action(&self, action: Action) -> (String, Result<String, String>) {
        let unit = |r: anyhow::Result<()>| r.map(|_| String::new()).map_err(|e| e.to_string());
        match action {
            Action::Start(t) => (format!("start {t}"), unit(self.client.start_container(&t))),
            Action::Stop(t) => (format!("stop {t}"), unit(self.client.stop_container(&t))),
            Action::Restart(t) => (format!("restart {t}"), unit(self.client.restart_container(&t))),
            Action::Kill(t) => (format!("kill {t}"), unit(self.client.kill_container(&t))),
            Action::RemoveContainer(t) => {
                (format!("remove {t}"), unit(self.client.remove_container(&t, true)))
            }
            Action::PruneContainers => (
                "prune containers".to_string(),
                self.client.prune_containers().map_err(|e| e.to_string()),
            ),
            Action::RemoveImage(t) => (format!("rmi {t}"), unit(self.client.remove_image(&t, true))),
            Action::PruneImages => (
                "prune images".to_string(),
                self.client.prune_images().map_err(|e| e.to_string()),
            ),
            Action::RemoveVolume(t) => {
                (format!("volume remove {t}"), unit(self.client.remove_volume(&t)))
            }
            Action::PruneVolumes => (
                "prune volumes".to_string(),
                self.client.prune_volumes().map_err(|e| e.to_string()),
            ),
            Action::RemoveNetwork(t) => {
                (format!("network remove {t}"), unit(self.client.remove_network(&t)))
            }
            Action::PruneNetworks => (
                "prune networks".to_string(),
                self.client.prune_networks().map_err(|e| e.to_string()),
            ),
        }
    }
}

/// Pretty-print raw JSON; if it doesn't parse, return it unchanged.
fn pretty_json(raw: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.to_string()),
        Err(_) => raw.to_string(),
    }
}
