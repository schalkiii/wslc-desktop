//! High-level wslc command wrappers built on [`WslcClient`].
//!
//! Read commands return parsed structs; lifecycle commands return `()` (or the
//! raw text for logs/inspect). Every list command uses `--format json`, matching
//! the schemas verified against wslc 2.9.3.0. The command surface here is
//! derived from `wslc --help` on that build — notably wslc has **no** `restart`,
//! `pause`, or `rename` command, so restart is emulated as stop + start.

use anyhow::{anyhow, Result};

use super::client::WslcClient;
use super::types::{Container, Image, Network, Stat, Volume};

/// Options for logging, mirroring `wslc logs` flags.
#[derive(Debug, Clone, Default)]
pub struct LogOptions {
    pub tail: u32,
    pub timestamps: bool,
}

/// A full container-run specification, mirroring the useful `wslc run` flags.
#[derive(Debug, Clone, Default)]
pub struct RunSpec {
    pub image: String,
    pub name: String,
    /// `HOST:CONTAINER[/proto]` publish mappings.
    pub ports: Vec<String>,
    /// `KEY=VALUE` environment pairs.
    pub env: Vec<String>,
    /// `HOST_PATH:CONTAINER_PATH[:ro]` or `named:CONTAINER_PATH` bind/volume mounts.
    pub volumes: Vec<String>,
    /// Network to attach to (empty = default).
    pub network: String,
    pub workdir: String,
    pub user: String,
    pub hostname: String,
    pub memory: String,
    pub cpus: String,
    pub entrypoint: String,
    /// Command + args to run, whitespace-split.
    pub command: String,
    pub detach: bool,
    pub auto_remove: bool,
    pub publish_all: bool,
}

impl RunSpec {
    /// Build the argument vector for `wslc run …`.
    fn to_args(&self) -> Vec<String> {
        let mut args: Vec<String> = vec!["run".to_string()];
        if self.detach {
            args.push("-d".to_string());
        }
        if self.auto_remove {
            args.push("--rm".to_string());
        }
        if self.publish_all {
            args.push("-P".to_string());
        }
        push_kv(&mut args, "--name", &self.name);
        push_kv(&mut args, "--network", &self.network);
        push_kv(&mut args, "-w", &self.workdir);
        push_kv(&mut args, "-u", &self.user);
        push_kv(&mut args, "-h", &self.hostname);
        push_kv(&mut args, "-m", &self.memory);
        push_kv(&mut args, "--cpus", &self.cpus);
        push_kv(&mut args, "--entrypoint", &self.entrypoint);
        push_repeated(&mut args, "-p", &self.ports);
        push_repeated(&mut args, "-e", &self.env);
        push_repeated(&mut args, "-v", &self.volumes);

        args.push(self.image.trim().to_string());
        // 复用 shell 式分词而不是 whitespace-split：向导的 command 字段同样
        // 会出现 `bash -c "echo hi && ls"` 这类带引号的命令，按空格硬拆会把
        // 整条 payload 撕碎成错误参数。
        for token in tokenize(&self.command) {
            args.push(token);
        }
        args
    }

    /// A human-readable summary of the equivalent CLI, for the confirm/preview.
    pub fn preview(&self) -> String {
        let joined = self
            .to_args()
            .iter()
            // 含空格的参数在 preview 中加引号，保证展示行复制后可直接执行。
            .map(|token| {
                if token.is_empty() {
                    "\"\"".to_string()
                } else if token.chars().any(|c| c.is_whitespace()) {
                    format!("\"{token}\"")
                } else {
                    token.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        format!("wslc {joined}")
    }
}

fn push_kv(args: &mut Vec<String>, flag: &str, value: &str) {
    let v = value.trim();
    if !v.is_empty() {
        args.push(flag.to_string());
        args.push(v.to_string());
    }
}

fn push_repeated(args: &mut Vec<String>, flag: &str, values: &[String]) {
    for value in values {
        let v = value.trim();
        if !v.is_empty() {
            args.push(flag.to_string());
            args.push(v.to_string());
        }
    }
}

impl WslcClient {
    /// `wslc version`
    pub fn version(&self) -> Result<String> {
        self.run(&["version"])
    }

    /// `wslc list --all --format json`
    pub fn list_containers(&self) -> Result<Vec<Container>> {
        self.run_json(&["list", "--all", "--format", "json"])
    }

    /// `wslc images --format json`
    pub fn list_images(&self) -> Result<Vec<Image>> {
        self.run_json(&["images", "--format", "json"])
    }

    /// `wslc volume list --format json`
    pub fn list_volumes(&self) -> Result<Vec<Volume>> {
        self.run_json(&["volume", "list", "--format", "json"])
    }

    /// `wslc network list --format json`
    pub fn list_networks(&self) -> Result<Vec<Network>> {
        self.run_json(&["network", "list", "--format", "json"])
    }

    /// `wslc stats --format json` (a single snapshot; no streaming)
    pub fn stats(&self) -> Result<Vec<Stat>> {
        self.run_json(&["stats", "--format", "json"])
    }

    /// `wslc logs <target> [-t] -n <tail>` — one-shot tail.
    pub fn logs(&self, target: &str, opts: &LogOptions) -> Result<String> {
        let tail_str = opts.tail.to_string();
        let mut args = vec!["logs"];
        if opts.timestamps {
            args.push("-t");
        }
        args.push("-n");
        args.push(&tail_str);
        args.push(target);
        self.run(&args)
    }

    /// `wslc inspect <target>` (raw JSON text)
    pub fn inspect(&self, target: &str) -> Result<String> {
        self.run(&["inspect", target])
    }

    /// `wslc exec [-w dir] [-u user] <target> <argv...>` — non-interactive.
    pub fn exec(&self, target: &str, command: &str, workdir: &str, user: &str) -> Result<String> {
        let mut args: Vec<String> = vec!["exec".to_string()];
        if !workdir.trim().is_empty() {
            args.push("-w".to_string());
            args.push(workdir.trim().to_string());
        }
        if !user.trim().is_empty() {
            args.push("-u".to_string());
            args.push(user.trim().to_string());
        }
        args.push(target.to_string());
        // Run through a shell so the user can type a full command line.
        args.push("sh".to_string());
        args.push("-c".to_string());
        args.push(command.to_string());
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&borrowed)
    }

    // ---- container lifecycle ------------------------------------------------

    pub fn start_container(&self, target: &str) -> Result<()> {
        self.run(&["start", target]).map(|_| ())
    }

    pub fn stop_container(&self, target: &str) -> Result<()> {
        self.run(&["stop", target]).map(|_| ())
    }

    /// wslc has no `restart`; emulate as stop then start.
    pub fn restart_container(&self, target: &str) -> Result<()> {
        // Ignore a stop error (container may already be stopped), then start.
        let _ = self.run(&["stop", target]);
        self.run(&["start", target]).map(|_| ())
    }

    pub fn kill_container(&self, target: &str) -> Result<()> {
        self.run(&["kill", target]).map(|_| ())
    }

    pub fn remove_container(&self, target: &str, force: bool) -> Result<()> {
        if force {
            self.run(&["remove", "--force", target]).map(|_| ())
        } else {
            self.run(&["remove", target]).map(|_| ())
        }
    }

    pub fn prune_containers(&self) -> Result<String> {
        // wslc `container prune` accepts no `--force` flag (it has no options
        // other than `--help`); passing one errors with "无法识别当前命令的
        // 参数名称". The GUI already confirms the action before calling this.
        self.run(&["container", "prune"])
    }

    // ---- images -------------------------------------------------------------

    /// `wslc pull <reference>` — can be slow; the client uses a long timeout.
    pub fn pull_image(&self, reference: &str) -> Result<String> {
        self.run_slow(&["pull", reference])
    }

    pub fn remove_image(&self, reference: &str, force: bool) -> Result<()> {
        if force {
            self.run(&["rmi", "--force", reference]).map(|_| ())
        } else {
            self.run(&["rmi", reference]).map(|_| ())
        }
    }

    pub fn prune_images(&self) -> Result<String> {
        // wslc `image prune` accepts `-a/--all` and `-f/--filter`, but no
        // `--force`; the GUI confirms before calling. Keeping the flag would
        // error ("无法识别当前命令的参数名称").
        self.run(&["image", "prune"])
    }

    // ---- volumes ------------------------------------------------------------

    /// `wslc volume create [-d driver] <name>`
    pub fn create_volume(&self, name: &str, driver: &str) -> Result<String> {
        let mut args = vec!["volume", "create"];
        if !driver.trim().is_empty() {
            args.push("-d");
            args.push(driver.trim());
        }
        args.push(name.trim());
        self.run(&args)
    }

    pub fn remove_volume(&self, name: &str) -> Result<()> {
        self.run(&["volume", "remove", name]).map(|_| ())
    }

    pub fn prune_volumes(&self) -> Result<String> {
        // wslc `volume prune` accepts `-a/--all` and `-f/--filter`, but no
        // `--force`. The user-visible error was "无法识别当前命令的参数名称:
        // --force". The GUI confirms before calling, so dropping the flag is
        // safe and makes the prune actually run.
        self.run(&["volume", "prune"])
    }

    // ---- networks -----------------------------------------------------------

    pub fn create_network(&self, name: &str) -> Result<String> {
        self.run(&["network", "create", name.trim()])
    }

    pub fn remove_network(&self, name: &str) -> Result<()> {
        self.run(&["network", "remove", name]).map(|_| ())
    }

    pub fn prune_networks(&self) -> Result<String> {
        // wslc `network prune` accepts `-f/--filter` but no `--force`; the GUI
        // confirms before calling, so the flag is dropped (it would error).
        self.run(&["network", "prune"])
    }

    // ---- run ----------------------------------------------------------------

    /// Run a container from a full [`RunSpec`].
    pub fn run_container(&self, spec: &RunSpec) -> Result<String> {
        let args = spec.to_args();
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        // Detached runs return quickly; foreground/pull-on-run may be slow.
        if spec.detach {
            self.run(&borrowed)
        } else {
            self.run_slow(&borrowed)
        }
    }

    /// Run a saved raw command line such as `wslc run -d --name x image …`.
    ///
    /// The line is tokenized with shell-style quote handling; an optional leading
    /// `wslc`/`wslc.exe` token is stripped so both `wslc run …` and `run …` work.
    /// Uses the long timeout because a first-time run may pull the image.
    pub fn run_raw(&self, command_line: &str) -> Result<String> {
        let mut tokens = tokenize(command_line);
        if matches!(tokens.first(), Some(t) if t.eq_ignore_ascii_case("wslc") || t.eq_ignore_ascii_case("wslc.exe"))
        {
            tokens.remove(0);
        }
        if tokens.is_empty() {
            return Err(anyhow!("command is empty after removing the `wslc` prefix"));
        }
        let borrowed: Vec<&str> = tokens.iter().map(String::as_str).collect();
        self.run_slow(&borrowed)
    }
}

/// Split a command line into arguments, honoring single and double quotes.
///
/// Quotes group whitespace-containing arguments and are removed from the output
/// (e.g. `-v "C:\a b:/x"` → `["-v", "C:\a b:/x"]`). Nested quotes of the other
/// kind are kept literally, matching how `bash -c "…"` payloads are written.
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut has_token = false;
    let mut in_single = false;
    let mut in_double = false;

    for ch in input.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                has_token = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                has_token = true;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if has_token {
                    tokens.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::tokenize;
    use super::*;

    /// 空输入产生空列表；未闭合引号把剩余内容当作一个 token（宽容处理）。
    #[test]
    fn tokenize_edge_cases() {
        assert!(tokenize("   ").is_empty());
        assert_eq!(
            tokenize(r#"run "unclosed arg"#),
            vec!["run", "unclosed arg"]
        );
    }

    /// 全部可选参数都填时的完整参数序（与 wslc run 的 flag 一一对应）。
    #[test]
    fn run_spec_to_args_full() {
        let spec = RunSpec {
            image: "nginx:latest".into(),
            name: "web".into(),
            ports: vec!["8080:80".into(), "5432:5432".into()],
            env: vec!["TZ=UTC".into()],
            volumes: vec!["C:/a b:/data".into()],
            network: "bridge".into(),
            workdir: "/app".into(),
            user: "1000:1000".into(),
            hostname: "web-host".into(),
            memory: "512M".into(),
            cpus: "1.5".into(),
            entrypoint: "/entry.sh".into(),
            command: "nginx -g \"daemon off;\"".into(),
            detach: true,
            auto_remove: true,
            publish_all: true,
        };
        assert_eq!(
            spec.to_args(),
            vec![
                "run",
                "-d",
                "--rm",
                "-P",
                "--name",
                "web",
                "--network",
                "bridge",
                "-w",
                "/app",
                "-u",
                "1000:1000",
                "-h",
                "web-host",
                "-m",
                "512M",
                "--cpus",
                "1.5",
                "--entrypoint",
                "/entry.sh",
                "-p",
                "8080:80",
                "-p",
                "5432:5432",
                "-e",
                "TZ=UTC",
                "-v",
                "C:/a b:/data",
                "nginx:latest",
                "nginx",
                "-g",
                "daemon off;"
            ]
        );
    }

    /// 空可选字段不得产生空参数或悬空 flag；detached=false 走慢超时路径。
    #[test]
    fn run_spec_to_args_skips_empty_optionals() {
        let spec = RunSpec {
            image: "alpine".into(),
            ..Default::default()
        };
        assert_eq!(spec.to_args(), vec!["run", "alpine"]);
        assert!(!spec.detach);
    }

    /// 向导 command 字段的引号 payload 必须整块传递（此前按空格硬拆会撕碎）。
    #[test]
    fn run_spec_command_keeps_quoted_payload() {
        let spec = RunSpec {
            image: "python:3.11".into(),
            command: r#"bash -c "echo hi && ls""#.into(),
            detach: true,
            ..Default::default()
        };
        let args = spec.to_args();
        assert_eq!(
            &args[args.len() - 4..],
            &["python:3.11", "bash", "-c", "echo hi && ls"][..]
        );
    }

    /// preview 行复制后可直接执行：含空格的参数带引号展示。
    #[test]
    fn run_spec_preview_quotes_whitespace_args() {
        let spec = RunSpec {
            image: "alpine".into(),
            volumes: vec!["C:/a b:/data".into()],
            detach: true,
            ..Default::default()
        };
        assert_eq!(spec.preview(), r#"wslc run -d -v "C:/a b:/data" alpine"#);
    }

    #[test]
    fn tokenize_handles_quoted_paths() {
        let args = tokenize(r#"run -d -v "C:\a b:/data" --name x image"#);
        assert_eq!(
            args,
            vec!["run", "-d", "-v", r"C:\a b:/data", "--name", "x", "image"]
        );
    }

    #[test]
    fn tokenize_keeps_bash_c_payload_together() {
        let args = tokenize(r#"run image bash -c "echo hi && ls""#);
        assert_eq!(args, vec!["run", "image", "bash", "-c", "echo hi && ls"]);
    }
}
