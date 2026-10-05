//! Low-level `wslc.exe` process runner.
//!
//! Framework-agnostic: depends only on `std` + `serde`. Every invocation runs
//! with a timeout and, on Windows, the `CREATE_NO_WINDOW` flag so the tool
//! never flashes a console window (the same approach lazywslc uses).

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

/// The wslc binary name; resolved from PATH.
const WSLC_BINARY: &str = "wslc.exe";

/// Default per-command timeout. Kept generous because some wslc calls spin up a
/// Hyper-V VM on first use.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// A longer timeout for commands that legitimately take a while, such as
/// `wslc pull` (image download) or a foreground `wslc run`.
const SLOW_TIMEOUT: Duration = Duration::from_secs(600);

/// A live `wslc logs --follow` stream: incoming lines plus the child handle so
/// the caller can stop it.
pub struct LogStream {
    child: Child,
    pub rx: Receiver<String>,
}

impl LogStream {
    /// Kill the underlying `wslc logs -f` process and reap it.
    pub fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Windows process creation flag: do not allocate a console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A thin handle around the wslc binary.
#[derive(Clone)]
pub struct WslcClient {
    binary: String,
    timeout: Duration,
}

impl Default for WslcClient {
    fn default() -> Self {
        Self {
            binary: WSLC_BINARY.to_string(),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl WslcClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run wslc with the given args, returning trimmed stdout on success.
    ///
    /// Errors when the process cannot be spawned, times out, exits non-zero, or
    /// prints only to stderr.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        self.run_with_timeout(args, self.timeout)
    }

    /// Like [`run`](Self::run) but with the long [`SLOW_TIMEOUT`] for pulls/runs.
    pub fn run_slow(&self, args: &[&str]) -> Result<String> {
        self.run_with_timeout(args, SLOW_TIMEOUT)
    }

    fn run_with_timeout(&self, args: &[&str], timeout: Duration) -> Result<String> {
        let (stdout, stderr, success) = self.spawn_bounded(args, timeout)?;
        let cleaned = strip_copyright_header(&stdout);

        if success {
            // Some commands succeed silently; that is fine.
            return Ok(cleaned);
        }

        // Non-zero exit: surface stderr (fall back to stdout) for the UI.
        let message = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !cleaned.trim().is_empty() {
            cleaned.trim().to_string()
        } else {
            "wslc exited with a non-zero status".to_string()
        };
        Err(anyhow!("wslc {}: {message}", args.join(" ")))
    }

    /// Run wslc and deserialize its JSON stdout into a list of `T`.
    ///
    /// 兼容两代输出格式：≤2.x 是一个整体 JSON 数组；≥3.x 是 NDJSON
    /// （每行一个对象，实测 3.0.1.0）。NDJSON 按行独立解析，个别异常行
    /// 只损失该行数据；全部行都不可解析时才整体报错，避免 UI 清空。
    pub fn run_json<T: serde::de::DeserializeOwned>(&self, args: &[&str]) -> Result<Vec<T>> {
        let output = self.run(args)?;
        parse_resource_list(&output, &args.join(" "))
    }

    /// Start a `wslc logs --follow` stream. Returns a [`LogStream`] delivering
    /// lines over a channel; call [`LogStream::stop`] to terminate it.
    pub fn logs_follow(&self, target: &str, tail: u32, timestamps: bool) -> Result<LogStream> {
        let tail_str = tail.to_string();
        let mut command = Command::new(&self.binary);
        command.arg("logs").arg("-f");
        if timestamps {
            command.arg("-t");
        }
        command.arg("-n").arg(&tail_str).arg(target);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn `{WSLC_BINARY} logs -f`"))?;

        let (tx, rx) = mpsc::channel::<String>();
        // Merge stdout + stderr line streams into the same channel.
        if let Some(out) = child.stdout.take() {
            let tx = tx.clone();
            thread::spawn(move || {
                for line in BufReader::new(out).lines().map_while(|l| l.ok()) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        if let Some(err) = child.stderr.take() {
            thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(|l| l.ok()) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }

        Ok(LogStream { child, rx })
    }

    /// Spawn wslc, capture stdout/stderr, and enforce the timeout by killing the
    /// child if it overruns. Returns (stdout, stderr, success).
    fn spawn_bounded(&self, args: &[&str], timeout: Duration) -> Result<(String, String, bool)> {
        let mut command = Command::new(&self.binary);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command.spawn().with_context(|| {
            format!("failed to spawn `{WSLC_BINARY}`; is WSL updated (wsl --update)?")
        })?;

        // Read pipes on separate threads to avoid deadlock on large output.
        let mut stdout_pipe = child.stdout.take().context("no stdout pipe")?;
        let mut stderr_pipe = child.stderr.take().context("no stderr pipe")?;

        let (tx_out, rx_out) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = String::new();
            let _ = stdout_pipe.read_to_string(&mut buf);
            let _ = tx_out.send(buf);
        });
        let (tx_err, rx_err) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = String::new();
            let _ = stderr_pipe.read_to_string(&mut buf);
            let _ = tx_err.send(buf);
        });

        // Poll for completion up to the timeout.
        let deadline = std::time::Instant::now() + timeout;
        let status = loop {
            match child.try_wait().context("failed while waiting on wslc")? {
                Some(status) => break status,
                None => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(anyhow!(
                            "wslc {} timed out after {}s",
                            args.join(" "),
                            timeout.as_secs()
                        ));
                    }
                    thread::sleep(Duration::from_millis(25));
                }
            }
        };

        let stdout = rx_out.recv().unwrap_or_default();
        let stderr = rx_err.recv().unwrap_or_default();
        Ok((stdout, stderr, status.success()))
    }
}

/// Remove any leading copyright / privacy notice lines that wslc may print
/// before real output. We drop leading blank lines and lines that look like a
/// banner until we hit the first line of substance (JSON `[`/`{` or text).
fn strip_copyright_header(raw: &str) -> String {
    let mut skipping = true;
    let mut kept: Vec<&str> = Vec::new();

    for line in raw.lines() {
        if skipping {
            let trimmed = line.trim();
            let is_banner = trimmed.is_empty()
                || trimmed.starts_with("Copyright")
                || trimmed.starts_with("(c)")
                || trimmed.starts_with("(C)")
                || trimmed.to_ascii_lowercase().contains("for privacy")
                || trimmed.to_ascii_lowercase().contains("microsoft");
            // JSON or a data line ends the banner.
            if trimmed.starts_with('[') || trimmed.starts_with('{') || !is_banner {
                skipping = false;
                kept.push(line);
                continue;
            }
            // still in banner: skip
        } else {
            kept.push(line);
        }
    }
    kept.join("\n")
}

/// Parse `--format json` output into `Vec<T>`，两种形态都接受：
/// wslc ≤2.x 的整体 JSON 数组，以及 ≥3.x 的 NDJSON（每行一个对象，
/// 行间可能有空行）。优先按数组解析；失败则逐行解析并跳过不可读行，
/// 只有在所有行都失败时才返回错误（此时更可能是 schema 漂移而非个别脏行）。
fn parse_resource_list<T: serde::de::DeserializeOwned>(raw: &str, command: &str) -> Result<Vec<T>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if let Ok(items) = serde_json::from_str::<Vec<T>>(trimmed) {
        return Ok(items);
    }
    let mut items = Vec::new();
    let mut unreadable = 0usize;
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<T>(line) {
            Ok(item) => items.push(item),
            Err(_) => unreadable += 1,
        }
    }
    if items.is_empty() {
        return Err(anyhow!(
            "failed to parse wslc JSON for `{command}` ({unreadable} unreadable line(s); unsupported schema?)"
        ));
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::{parse_resource_list, strip_copyright_header};

    /// ≥3.x 的 NDJSON 多行对象（含对象间空行、CRLF 行尾）逐行解析。
    #[test]
    fn parse_resource_list_accepts_ndjson() {
        let raw = "{\"ID\":\"a\",\"Names\":\"one\"}\r\n\r\n{\"ID\":\"b\",\"Names\":\"two\"}\r\n";
        let items = parse_resource_list::<serde_json::Value>(raw, "list").unwrap();
        assert_eq!(items.len(), 2);
    }

    /// ≤2.x 的整体 JSON 数组仍兼容。
    #[test]
    fn parse_resource_list_accepts_array() {
        let raw = r#"[{"Id":"a"},{"Id":"b"}]"#;
        let items = parse_resource_list::<serde_json::Value>(raw, "list").unwrap();
        assert_eq!(items.len(), 2);
    }

    /// 空输出 → 空列表；全部行不可解析 → 报错而非静默清空。
    #[test]
    fn parse_resource_list_edge_cases() {
        assert!(parse_resource_list::<serde_json::Value>("", "x")
            .unwrap()
            .is_empty());
        assert!(parse_resource_list::<serde_json::Value>("   \r\n", "x")
            .unwrap()
            .is_empty());
        assert!(parse_resource_list::<serde_json::Value>("not json", "x").is_err());
    }

    /// 版权/隐私 banner 各形态都被剥掉，首个实质行之后内容原样保留。
    #[test]
    fn strip_copyright_header_removes_leading_banner() {
        let raw = "Copyright (c) Microsoft Corporation.\r\n\r\nSee ... for privacy information.\r\nwslc 3.0.1.0\r\nnext line\r\n";
        assert_eq!(strip_copyright_header(raw), "wslc 3.0.1.0\nnext line");
    }

    /// JSON 数据行即使包含 "microsoft"（镜像名等）也不能被误当 banner。
    #[test]
    fn strip_copyright_header_keeps_json_containing_microsoft() {
        let raw = r#"{"ID":"a","Image":"mcr.microsoft.com/dotnet/runtime:latest"}"#;
        assert_eq!(strip_copyright_header(raw), raw);
    }

    /// 无 banner 的普通文本（如日志）原样保留，包括空行。
    #[test]
    fn strip_copyright_header_keeps_plain_text() {
        let raw = "Server start on http://localhost:8088/\r\n\r\nsecond\r\n";
        assert_eq!(
            strip_copyright_header(raw),
            "Server start on http://localhost:8088/\n\nsecond"
        );
    }

    /// 空 / 纯空白输入 → 空字符串。
    #[test]
    fn strip_copyright_header_empty_input() {
        assert_eq!(strip_copyright_header(""), "");
        assert_eq!(strip_copyright_header("  \r\n\r\n "), "");
    }
}
