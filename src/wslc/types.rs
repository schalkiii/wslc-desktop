//! Data structures deserialized from `wslc --format json` output.
//!
//! All field names use `PascalCase` to match wslc's JSON exactly (verified
//! against wslc 2.9.3.0 on real containers). Every field is tolerant to
//! omission via `#[serde(default)]` so schema drift in the preview build does
//! not break parsing.

use serde::Deserialize;

/// Lifecycle state of a container, as reported by `wslc list`'s numeric `State`.
///
/// Mapping confirmed empirically on wslc 2.9.3.0 (2 = Running, 3 = Exited) and
/// cross-checked with the lazywslc / lazywslcontainer sources for the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    Created,
    Running,
    Exited,
    Paused,
    Unknown(u8),
}

impl ContainerState {
    pub fn from_code(code: u8) -> Self {
        match code {
            1 => ContainerState::Created,
            2 => ContainerState::Running,
            3 => ContainerState::Exited,
            4 => ContainerState::Paused,
            other => ContainerState::Unknown(other),
        }
    }

    pub fn label(self) -> String {
        match self {
            ContainerState::Created => "created".to_string(),
            ContainerState::Running => "running".to_string(),
            ContainerState::Exited => "exited".to_string(),
            ContainerState::Paused => "paused".to_string(),
            ContainerState::Unknown(code) => format!("unknown({code})"),
        }
    }

    pub fn is_running(self) -> bool {
        matches!(self, ContainerState::Running)
    }
}

/// A published port mapping from `Ports[]` in `wslc list`.
#[derive(Debug, Clone, Deserialize)]
pub struct PortMapping {
    #[serde(default, rename = "BindingAddress")]
    pub binding_address: String,
    #[serde(default, rename = "ContainerPort")]
    pub container_port: u16,
    #[serde(default, rename = "HostPort")]
    pub host_port: u16,
    /// IP protocol number: 6 = tcp, 17 = udp.
    #[serde(default, rename = "Protocol")]
    pub protocol: u16,
}

impl PortMapping {
    pub fn protocol_label(&self) -> &'static str {
        match self.protocol {
            17 => "udp",
            _ => "tcp",
        }
    }

    /// e.g. `127.0.0.1:8003->8080/tcp`
    pub fn display(&self) -> String {
        let addr = if self.binding_address.is_empty() {
            "0.0.0.0"
        } else {
            &self.binding_address
        };
        format!(
            "{addr}:{}->{}/{}",
            self.host_port,
            self.container_port,
            self.protocol_label()
        )
    }
}

/// A container row from `wslc list --all --format json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Container {
    #[serde(default, rename = "Id")]
    pub id: String,
    #[serde(default, rename = "Name")]
    pub name: String,
    #[serde(default, rename = "Image")]
    pub image: String,
    #[serde(default, rename = "State")]
    pub state_code: u8,
    #[serde(default, rename = "CreatedAt")]
    pub created_at: i64,
    #[serde(default, rename = "StateChangedAt")]
    pub state_changed_at: i64,
    #[serde(default, rename = "Ports")]
    pub ports: Vec<PortMapping>,
}

impl Container {
    pub fn state(&self) -> ContainerState {
        ContainerState::from_code(self.state_code)
    }

    /// Prefer the human name; fall back to a short id.
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            self.short_id()
        } else {
            self.name.clone()
        }
    }

    /// A wslc id or name usable as a command target.
    pub fn target(&self) -> String {
        if self.name.is_empty() {
            self.id.clone()
        } else {
            self.name.clone()
        }
    }

    pub fn short_id(&self) -> String {
        self.id.chars().take(12).collect()
    }

    /// Whether this container was created from `image`, matching registry- and
    /// namespace-tolerantly so `docker.io/library/nginx:latest` ≡ `nginx:latest`.
    pub fn uses_image(&self, image: &Image) -> bool {
        normalize_image_ref(&self.image) == normalize_image_ref(&image.reference())
    }
}

/// Normalize an image reference for matching: strip a scheme and registry host,
/// drop Docker's implicit `library/` namespace, and default a missing tag to
/// `latest`, so `docker.io/library/nginx:latest`, `nginx:latest` and `nginx`
/// all collapse to the same bare `repo:tag`.
fn normalize_image_ref(reference: &str) -> String {
    let mut s = reference.trim().to_string();
    for scheme in ["https://", "http://"] {
        if let Some(rest) = s.strip_prefix(scheme) {
            s = rest.to_string();
        }
    }
    if let Some(slash) = s.find('/') {
        let head = &s[..slash];
        let is_host = head.contains('.') || head.contains(':') || head == "localhost";
        if is_host {
            s = s[slash + 1..].to_string();
        }
    }
    if let Some(rest) = s.strip_prefix("library/") {
        s = rest.to_string();
    }
    if !s.contains(':') {
        s = format!("{s}:latest");
    }
    s
}

/// An image row from `wslc images --format json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Image {
    #[serde(default, rename = "Id")]
    pub id: String,
    #[serde(default, rename = "Repository")]
    pub repository: String,
    #[serde(default, rename = "Tag")]
    pub tag: String,
    #[serde(default, rename = "Size")]
    pub size: u64,
    #[serde(default, rename = "Created")]
    pub created: i64,
}

impl Image {
    /// e.g. `ghcr.io/schalkiii/reseedhound:latest`
    pub fn reference(&self) -> String {
        if self.repository.is_empty() {
            return self.short_id();
        }
        let tag = if self.tag.is_empty() { "latest" } else { &self.tag };
        format!("{}:{}", self.repository, tag)
    }

    pub fn short_id(&self) -> String {
        // Drop a leading `sha256:` if present, then take 12 hex chars.
        let hex = self.id.strip_prefix("sha256:").unwrap_or(&self.id);
        hex.chars().take(12).collect()
    }

    pub fn size_display(&self) -> String {
        human_size(self.size)
    }
}

/// A volume row from `wslc volume list --format json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Volume {
    #[serde(default, rename = "Name")]
    pub name: String,
    #[serde(default, rename = "Driver")]
    pub driver: String,
}

/// A network row from `wslc network list --format json`.
///
/// The preview build currently reports an empty list; fields are modelled
/// defensively (all optional) with common aliases so parsing survives whatever
/// casing wslc settles on.
#[derive(Debug, Clone, Deserialize)]
pub struct Network {
    #[serde(default, alias = "Name", alias = "name")]
    pub name: String,
    #[serde(default, alias = "Id", alias = "ID", alias = "id")]
    pub id: String,
    #[serde(default, alias = "Driver", alias = "driver")]
    pub driver: String,
    #[serde(default, alias = "Scope", alias = "scope")]
    pub scope: String,
}

impl Network {
    /// Prefer the human name; fall back to a short id.
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            self.short_id()
        } else {
            self.name.clone()
        }
    }

    pub fn short_id(&self) -> String {
        let hex = self.id.strip_prefix("sha256:").unwrap_or(&self.id);
        hex.chars().take(12).collect()
    }
}

/// A live-stats row from `wslc stats --format json`. Numeric fields arrive as
/// human strings (e.g. `"0.25%"`, `"63.16 MiB / 15.48 GiB"`) and are parsed
/// lazily via the helper accessors.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Stat {
    #[serde(default, rename = "ID")]
    pub id: String,
    #[allow(dead_code)] // Retained for debug / inspection; keyed by ID in UI.
    #[serde(default, rename = "Name")]
    pub name: String,
    #[serde(default, rename = "CPUPerc")]
    pub cpu_perc: String,
    #[serde(default, rename = "MemUsage")]
    pub mem_usage: String,
    #[serde(default, rename = "MemPerc")]
    pub mem_perc: String,
    #[serde(default, rename = "NetIO")]
    pub net_io: String,
    #[serde(default, rename = "BlockIO")]
    pub block_io: String,
    #[serde(default, rename = "PIDs")]
    pub pids: u32,
}

impl Stat {
    /// Parse `"0.25%"` -> 0.25.
    pub fn cpu_percent(&self) -> f64 {
        self.cpu_perc.trim().trim_end_matches('%').parse().unwrap_or(0.0)
    }

    /// Parse the used side of `"63.16 MiB / 15.48 GiB"` -> bytes.
    pub fn mem_used_bytes(&self) -> f64 {
        let used = self.mem_usage.split('/').next().unwrap_or("").trim();
        parse_size_to_bytes(used)
    }

    /// Parse `"63.16%"` -> 63.16 (used for header sorting).
    pub fn mem_percent(&self) -> f64 {
        self.mem_perc
            .trim()
            .trim_end_matches('%')
            .parse()
            .unwrap_or(0.0)
    }

    /// Parse the received side of `"8.5MB / 2.1MB"` -> bytes (for sorting).
    pub fn net_rx_bytes(&self) -> f64 {
        let rx = self.net_io.split('/').next().unwrap_or("").trim();
        parse_size_to_bytes(rx)
    }

    /// Parse the read side of `"12.3MB / 4.5MB"` -> bytes (for sorting).
    pub fn block_rx_bytes(&self) -> f64 {
        let rx = self.block_io.split('/').next().unwrap_or("").trim();
        parse_size_to_bytes(rx)
    }
}

/// Format a byte count as a human-readable binary size.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

/// Parse strings like `"63.16 MiB"`, `"8.50MB"` (no space), or `"0 B"` into bytes.
///
/// Tolerates the number and unit being concatenated (which is how `wslc stats`
/// emits `NetIO` / `BlockIO`, e.g. `"8.50MB / 2.10MB"`) by splitting on the
/// first non-numeric character rather than on whitespace.
fn parse_size_to_bytes(text: &str) -> f64 {
    let trimmed = text.trim();
    let split = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '+' && c != '-')
        .unwrap_or(trimmed.len());
    let (num_str, unit_str) = trimmed.split_at(split);
    let number: f64 = num_str.trim().parse().unwrap_or(0.0);
    let unit = unit_str.trim().to_ascii_uppercase();
    let factor = match unit.as_str() {
        "B" => 1.0,
        "KIB" | "KB" => 1024.0,
        "MIB" | "MB" => 1024.0 * 1024.0,
        "GIB" | "GB" => 1024.0 * 1024.0 * 1024.0,
        "TIB" | "TB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    number * factor
}

/// Render a unix timestamp as a compact "time ago" string.
pub fn relative_time(unix_secs: i64) -> String {
    if unix_secs <= 0 {
        return "-".to_string();
    }
    let now = chrono::Utc::now().timestamp();
    let delta = now - unix_secs;
    if delta < 0 {
        return "just now".to_string();
    }
    let (value, unit) = if delta < 60 {
        (delta, "s")
    } else if delta < 3600 {
        (delta / 60, "m")
    } else if delta < 86_400 {
        (delta / 3600, "h")
    } else {
        (delta / 86_400, "d")
    };
    format!("{value}{unit} ago")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numeric accessors back the clickable column sorting, so they must
    /// parse the display strings `wslc stats` actually emits.
    #[test]
    fn stat_numeric_accessors() {
        let stat = Stat {
            id: "abc".into(),
            name: "demo".into(),
            cpu_perc: "12.50%".into(),
            mem_usage: "63.16 MiB / 15.48 GiB".into(),
            mem_perc: "34.20%".into(),
            net_io: "8.50MB / 2.10MB".into(),
            block_io: "12.30MB / 4.50MB".into(),
            pids: 7,
        };
        assert_eq!(stat.cpu_percent(), 12.5);
        assert_eq!(stat.mem_percent(), 34.2);
        // 63.16 MiB -> 63.16 * 1024 * 1024 bytes
        assert!((stat.mem_used_bytes() - 63.16 * 1024.0 * 1024.0).abs() < 1.0);
        // 8.50MB -> 8.50 * 1024 * 1024 bytes (MB treated as binary here)
        assert!((stat.net_rx_bytes() - 8.50 * 1024.0 * 1024.0).abs() < 1.0);
        assert!((stat.block_rx_bytes() - 12.30 * 1024.0 * 1024.0).abs() < 1.0);
    }

    #[test]
    fn stat_missing_fields_default_to_zero() {
        let stat = Stat::default();
        assert_eq!(stat.cpu_percent(), 0.0);
        assert_eq!(stat.mem_used_bytes(), 0.0);
        assert_eq!(stat.net_rx_bytes(), 0.0);
    }

    /// `uses_image` must treat registry- and namespace-variant references as the
    /// same image, so the "In Use" column is accurate across wslc output styles.
    #[test]
    fn container_uses_image_is_registry_tolerant() {
        let image = Image {
            id: "sha256:deadbeef".into(),
            repository: "nginx".into(),
            tag: "latest".into(),
            size: 0,
            created: 0,
        };

        let mk = |image: &str| Container {
            id: "id".into(),
            name: "c".into(),
            image: image.into(),
            state_code: 2,
            created_at: 0,
            state_changed_at: 0,
            ports: vec![],
        };

        // All of these denote the same image as `nginx:latest`.
        for container_image in [
            "nginx",
            "nginx:latest",
            "docker.io/library/nginx:latest",
            "https://registry-1.docker.io/library/nginx:latest",
        ] {
            assert!(mk(container_image).uses_image(&image), "expected {container_image:?} to match");
        }

        // A different image must not match.
        assert!(!mk("redis:latest").uses_image(&image));
        assert!(!mk("nginx:1.25").uses_image(&image));
    }
}
