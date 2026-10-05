//! Data structures deserialized from `wslc --format json` output.
//!
//! All field names use `PascalCase` to match wslc's JSON exactly (verified
//! against wslc 2.9.3.0 on real containers). Every field is tolerant to
//! omission via `#[serde(default)]` so schema drift in the preview build does
//! not break parsing.

use serde::Deserialize;

/// 反序列化 `State`：≤2.x 是数字码，≥3.x 是 `"exited"` 这类状态词字符串。
/// 统一归一化成旧版数字码，交由 [`ContainerState::from_code`] 解释。
fn de_state_code<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl serde::de::Visitor<'_> for Visitor {
        type Value = u8;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a numeric state code or a state name")
        }

        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<u8, E> {
            Ok(v.min(u8::MAX as u64) as u8)
        }

        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<u8, E> {
            Ok(v.clamp(0, i64::from(u8::MAX)) as u8)
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<u8, E> {
            let lowered = v.trim().to_ascii_lowercase();
            // 状态词优先，其次容忍数字以字符串形式出现（"2"），未知值归 0。
            Ok(match lowered.as_str() {
                "created" => 1,
                "running" => 2,
                "exited" => 3,
                "paused" => 4,
                other => other.parse().unwrap_or(0),
            })
        }
    }

    deserializer.deserialize_any(Visitor)
}

/// 反序列化时间：≤2.x 是 unix 秒数，≥3.x 是 `"2026-07-15 21:03:40 +0800 GMT+8"`
/// 这类字符串（network 的 CreatedAt 还带小数秒：`… 15:44:29.886 +0000 UTC`）。
fn de_unix_time<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl serde::de::Visitor<'_> for Visitor {
        type Value = i64;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a unix timestamp or a wslc time string")
        }

        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<i64, E> {
            Ok(v.min(i64::MAX as u64) as i64)
        }

        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<i64, E> {
            Ok(v)
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<i64, E> {
            Ok(parse_wslc_time(v))
        }
    }

    deserializer.deserialize_any(Visitor)
}

/// 把 ≥3.x 的时间字符串解析成 unix 秒：丢弃尾部 `GMT+8`/`UTC` 标签，只取
/// 「日期 时间 时区」三段。解析失败回退到纯数字（≤2.x 值可能以字符串出现），
/// 仍失败返回 0（UI 显示为 `-`）。
fn parse_wslc_time(text: &str) -> i64 {
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.len() >= 3 {
        let joined = format!("{} {} {}", parts[0], parts[1], parts[2]);
        // `%.f` 匹配可选的小数秒，兼容无小数与纳秒精度两种形态。
        if let Ok(dt) = chrono::DateTime::parse_from_str(&joined, "%Y-%m-%d %H:%M:%S%.f %z") {
            return dt.timestamp();
        }
    }
    text.trim().parse::<i64>().unwrap_or(0)
}

/// 反序列化镜像大小：≤2.x 是字节数，≥3.x 是 `"210MB"`/`"N/A"` 这类字符串。
fn de_byte_size<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl serde::de::Visitor<'_> for Visitor {
        type Value = u64;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a byte count or a human size string")
        }

        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<u64, E> {
            Ok(v)
        }

        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<u64, E> {
            Ok(v.max(0) as u64)
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<u64, E> {
            Ok(parse_size_to_bytes(v) as u64)
        }
    }

    deserializer.deserialize_any(Visitor)
}

/// 反序列化 `Ports`：≤2.x 是映射对象数组，≥3.x 是 docker 风格文本
/// （如 `"127.0.0.1:8082->8088/tcp"`，3.0.1.0 运行容器实测）。
fn de_ports<'de, D>(deserializer: D) -> Result<Vec<PortMapping>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = Vec<PortMapping>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a port mapping array or docker-style port text")
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Vec<PortMapping>, A::Error> {
            let mut ports = Vec::new();
            while let Some(port) = seq.next_element::<PortMapping>()? {
                ports.push(port);
            }
            Ok(ports)
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Vec<PortMapping>, E> {
            Ok(parse_ports_text(v))
        }
    }

    deserializer.deserialize_any(Visitor)
}

/// 解析 docker 风格端口文本，多条映射以逗号分隔；解析不出的条目直接跳过，
/// 避免个别格式漂移拖垮整行数据。
fn parse_ports_text(text: &str) -> Vec<PortMapping> {
    text.split(',').filter_map(parse_port_entry).collect()
}

/// 解析单条 `[addr:]host->container[/proto]` 映射；宿主侧兼容 `[::1]:8003`
/// （IPv6 带括号）、`127.0.0.1:8003` 与裸端口三种写法。
fn parse_port_entry(entry: &str) -> Option<PortMapping> {
    let entry = entry.trim();
    let (host_side, container_side) = entry.split_once("->")?;
    let (container_port_str, proto) = container_side.split_once('/')?;
    let protocol = match proto.trim() {
        "udp" => 17,
        _ => 6,
    };
    let host_side = host_side.trim();
    let (binding_address, host_port_str) = if let Some(rest) = host_side.strip_prefix('[') {
        let (addr, rest) = rest.split_once(']')?;
        (addr.to_string(), rest.trim_start_matches(':'))
    } else if let Some((addr, port)) = host_side.rsplit_once(':') {
        (addr.to_string(), port)
    } else {
        (String::new(), host_side)
    };
    Some(PortMapping {
        binding_address,
        container_port: container_port_str.trim().parse().ok()?,
        host_port: host_port_str.trim().parse().ok()?,
        protocol,
    })
}

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
///
/// 同时兼容两代输出 schema（3.0.1.0 实测）：≤2.x 输出 JSON 数组，`Id`/`Name`
/// 键名、数字 `State` 码、unix 时间戳、`Ports` 对象数组；≥3.x 输出 NDJSON，
/// 键改名 `ID`/`Names`，`State` 变字符串（`"exited"`），时间变人类可读字符串，
/// `Ports` 变 docker 风格文本。字段类型不匹配时 serde 会整体报错，因此这里的
/// 自定义反序列化器必须同时接受两种形态。
#[derive(Debug, Clone, Deserialize)]
pub struct Container {
    #[serde(default, rename = "Id", alias = "ID")]
    pub id: String,
    #[serde(default, rename = "Name", alias = "Names")]
    pub name: String,
    #[serde(default, rename = "Image")]
    pub image: String,
    #[serde(default, rename = "State", deserialize_with = "de_state_code")]
    pub state_code: u8,
    #[serde(default, rename = "CreatedAt", deserialize_with = "de_unix_time")]
    pub created_at: i64,
    #[serde(default, rename = "StateChangedAt", deserialize_with = "de_unix_time")]
    pub state_changed_at: i64,
    #[serde(default, rename = "Ports", deserialize_with = "de_ports")]
    pub ports: Vec<PortMapping>,
    /// ≥3.x 的人类可读状态行（如 `Exited (255) 21 minutes ago`、`Up 3 seconds`），
    /// 比 `state_changed_at` 信息更准；≤2.x 无此字段，保持为空并由 UI 回退显示相对时间。
    #[serde(default, rename = "Status")]
    pub status: String,
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
///
/// 兼容两代 schema：≤2.x 数字 `Created`/`Size`；≥3.x 键名 `ID`/`CreatedAt`，
/// `Size` 变 `"210MB"` 这类人类字符串。
#[derive(Debug, Clone, Deserialize)]
pub struct Image {
    #[serde(default, rename = "Id", alias = "ID")]
    pub id: String,
    #[serde(default, rename = "Repository")]
    pub repository: String,
    #[serde(default, rename = "Tag")]
    pub tag: String,
    #[serde(default, rename = "Size", deserialize_with = "de_byte_size")]
    pub size: u64,
    #[serde(
        default,
        rename = "Created",
        alias = "CreatedAt",
        deserialize_with = "de_unix_time"
    )]
    pub created: i64,
}

impl Image {
    /// e.g. `ghcr.io/schalkiii/reseedhound:latest`
    pub fn reference(&self) -> String {
        if self.repository.is_empty() {
            return self.short_id();
        }
        let tag = if self.tag.is_empty() {
            "latest"
        } else {
            &self.tag
        };
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
        self.cpu_perc
            .trim()
            .trim_end_matches('%')
            .parse()
            .unwrap_or(0.0)
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
            status: String::new(),
        };

        // All of these denote the same image as `nginx:latest`.
        for container_image in [
            "nginx",
            "nginx:latest",
            "docker.io/library/nginx:latest",
            "https://registry-1.docker.io/library/nginx:latest",
        ] {
            assert!(
                mk(container_image).uses_image(&image),
                "expected {container_image:?} to match"
            );
        }

        // A different image must not match.
        assert!(!mk("redis:latest").uses_image(&image));
        assert!(!mk("nginx:1.25").uses_image(&image));
    }

    /// wslc ≥3.x（3.0.1.0 运行容器实测）：NDJSON 单行、键改名、字符串状态、
    /// 字符串时间、docker 风格端口文本，都必须解析成功。
    #[test]
    fn container_row_from_wslc3_running() {
        let raw = r#"{"Command":"\"docker-entrypoint.s…\"","CreatedAt":"2026-07-15 21:03:40 +0800 GMT+8","HealthStatus":"","ID":"fdda489428e9","Image":"docker.1panel.live/easychen/cookiecloud:latest","Labels":"meta","LocalVolumes":"0","Mounts":"","Names":"cookiecloud","Networks":"bridge","Platform":{"architecture":"amd64","os":"linux"},"Ports":"127.0.0.1:8082->8088/tcp","RunningFor":"2 months ago","Size":"0B","State":"running","Status":"Up 3 seconds"}"#;
        let c: Container = serde_json::from_str(raw).unwrap();
        assert_eq!(c.id, "fdda489428e9");
        assert_eq!(c.name, "cookiecloud");
        assert_eq!(c.state_code, 2);
        assert_eq!(c.status, "Up 3 seconds");
        assert!(c.created_at > 0);
        assert_eq!(c.ports.len(), 1);
        assert_eq!(c.ports[0].display(), "127.0.0.1:8082->8088/tcp");
        assert_eq!(c.ports[0].host_port, 8082);
    }

    /// ≥3.x 的 exited 行：端口为空串、无 StateChangedAt 字段（回退默认值）。
    #[test]
    fn container_row_from_wslc3_exited() {
        let raw = r#"{"ID":"7c138ce089fc","Names":"pt-invite-watcher","Image":"x/y:latest","State":"exited","CreatedAt":"2026-07-15 21:10:43 +0800 GMT+8","Ports":"","Status":"Exited (255) 21 minutes ago"}"#;
        let c: Container = serde_json::from_str(raw).unwrap();
        assert_eq!(c.state_code, 3);
        assert!(c.ports.is_empty());
        assert!(c.status.starts_with("Exited"));
        assert_eq!(c.state_changed_at, 0);
    }

    /// wslc ≤2.x：JSON 数组、数字 State、unix 时间戳、Ports 对象数组仍兼容。
    #[test]
    fn container_row_from_wslc2_array() {
        let raw = r#"[{"Id":"abc123def456","Name":"demo","Image":"nginx:latest","State":2,"CreatedAt":1719999999,"StateChangedAt":1720000000,"Ports":[{"BindingAddress":"127.0.0.1","ContainerPort":80,"HostPort":8080,"Protocol":6}]}]"#;
        let list: Vec<Container> = serde_json::from_str(raw).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].state_code, 2);
        assert_eq!(list[0].created_at, 1_719_999_999);
        assert_eq!(list[0].state_changed_at, 1_720_000_000);
        assert_eq!(list[0].ports[0].host_port, 8080);
        assert!(list[0].status.is_empty());
    }

    /// ≥3.x 镜像行：ID 短码、Size 人类字符串（"210MB"）、CreatedAt 字符串。
    #[test]
    fn image_row_from_wslc3() {
        let raw = r#"{"Containers":"1","CreatedAt":"2026-07-12 03:47:20 +0800 GMT+8","CreatedSince":"2 months ago","Digest":"<none>","ID":"3b84dca42aa4","Repository":"docker.1panel.live/x/y","SharedSize":"N/A","Size":"210MB","Tag":"latest","UniqueSize":"N/A"}"#;
        let i: Image = serde_json::from_str(raw).unwrap();
        assert_eq!(i.id, "3b84dca42aa4");
        assert_eq!(i.size, 210 * 1024 * 1024);
        assert!(i.created > 0);
        assert_eq!(i.reference(), "docker.1panel.live/x/y:latest");
    }

    /// ≤2.x 数字 Size / Created 仍兼容。
    #[test]
    fn image_row_from_wslc2() {
        let raw = r#"{"Id":"sha256:deadbeef","Repository":"nginx","Tag":"1.25","Size":187000000,"Created":1719999999}"#;
        let i: Image = serde_json::from_str(raw).unwrap();
        assert_eq!(i.short_id(), "deadbeef");
        assert_eq!(i.size, 187_000_000);
        assert_eq!(i.created, 1_719_999_999);
    }

    /// docker 风格端口文本：多映射、udp、IPv6、垃圾输入。
    #[test]
    fn parse_ports_text_variants() {
        let ports = parse_ports_text("127.0.0.1:8082->8088/tcp, 0.0.0.0:53->53/udp");
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].display(), "127.0.0.1:8082->8088/tcp");
        assert_eq!(ports[1].protocol_label(), "udp");
        assert_eq!(ports[1].host_port, 53);

        let ipv6 = parse_ports_text("[::1]:8003->8080/tcp");
        assert_eq!(ipv6.len(), 1);
        assert_eq!(ipv6[0].binding_address, "::1");
        assert_eq!(ipv6[0].host_port, 8003);

        assert!(parse_ports_text("").is_empty());
        assert!(parse_ports_text("garbage").is_empty());
    }

    /// 时间字符串：带 GMT 标签、带纳秒小数、纯数字、垃圾输入。
    #[test]
    fn parse_wslc_time_variants() {
        // 2026-07-15 21:03:40 +0800 == 13:03:40 UTC，落在合理区间。
        let ts = parse_wslc_time("2026-07-15 21:03:40 +0800 GMT+8");
        assert!((1_700_000_000..1_900_000_000).contains(&ts));
        assert!(parse_wslc_time("2026-10-05 15:44:29.886881206 +0000 UTC") > 0);
        assert_eq!(parse_wslc_time("1719999999"), 1_719_999_999);
        assert_eq!(parse_wslc_time("n/a"), 0);
    }

    /// ≥3.x 网络行（3.0.1.0 实测）：ID/Name/Driver/Scope 均靠 alias 命中。
    #[test]
    fn network_row_from_wslc3() {
        let raw = r#"{"CreatedAt":"2026-10-05 15:44:29.886881206 +0000 UTC","Driver":"bridge","ID":"c1758720de06","IPv4":"true","IPv6":"false","Internal":"false","Labels":"","Name":"bridge","Scope":"local"}"#;
        let n: Network = serde_json::from_str(raw).unwrap();
        assert_eq!(n.display_name(), "bridge");
        assert_eq!(n.short_id(), "c1758720de06");
        assert_eq!(n.driver, "bridge");
        assert_eq!(n.scope, "local");
    }

    /// ≤2.x 卷行：未知字段忽略、Driver/Name 命中。
    #[test]
    fn volume_row_parses() {
        let raw = r#"{"Driver":"guest","Name":"pansou-data","Extra":"ignored"}"#;
        let v: Volume = serde_json::from_str(raw).unwrap();
        assert_eq!(v.name, "pansou-data");
        assert_eq!(v.driver, "guest");
    }

    /// 状态码 → 标签的全枚举覆盖（UI 直接展示这些标签）。
    #[test]
    fn container_state_labels_and_codes() {
        assert_eq!(ContainerState::from_code(1).label(), "created");
        assert_eq!(ContainerState::from_code(2).label(), "running");
        assert_eq!(ContainerState::from_code(3).label(), "exited");
        assert_eq!(ContainerState::from_code(4).label(), "paused");
        assert_eq!(ContainerState::from_code(99).label(), "unknown(99)");
        assert!(ContainerState::from_code(2).is_running());
        assert!(!ContainerState::from_code(3).is_running());
    }

    /// 镜像 ID 兼容 sha256: 前缀与裸短码两种形态。
    #[test]
    fn image_short_id_variants() {
        let mut image = Image {
            id: "sha256:0123456789abcdef".into(),
            repository: String::new(),
            tag: String::new(),
            size: 0,
            created: 0,
        };
        assert_eq!(image.short_id(), "0123456789ab");
        // repository 为空时 reference 回退短 ID。
        assert_eq!(image.reference(), "0123456789ab");
        image.id = "3b84dca42aa4".into();
        assert_eq!(image.short_id(), "3b84dca42aa4");
    }

    /// human_size 二进制单位进位与边界（0B、恰好在单位边界）。
    #[test]
    fn human_size_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1.00 KiB");
        assert_eq!(human_size(210 * 1024 * 1024), "210.00 MiB");
        assert_eq!(human_size(15 * 1024 * 1024 * 1024 + 1), "15.00 GiB");
    }

    /// relative_time 的各时间档位与边界输入（0/负值显示 -）。
    #[test]
    fn relative_time_tiers() {
        assert_eq!(relative_time(0), "-");
        let now = chrono::Utc::now().timestamp();
        assert_eq!(relative_time(now + 10), "just now");
        assert_eq!(relative_time(now - 30), "30s ago");
        assert_eq!(relative_time(now - 120), "2m ago");
        assert_eq!(relative_time(now - 7200), "2h ago");
        assert_eq!(relative_time(now - 3 * 86_400), "3d ago");
    }

    /// normalize_image_ref：registry/命名空间/协议/缺省 tag 的归一化边界。
    #[test]
    fn normalize_image_ref_variants() {
        fn norm(s: &str) -> String {
            // 通过 uses_image 间接驱动难以覆盖全部输入，直接测私有函数。
            super::normalize_image_ref(s)
        }
        assert_eq!(norm("nginx"), "nginx:latest");
        assert_eq!(norm("nginx:1.25"), "nginx:1.25");
        assert_eq!(norm("docker.io/library/nginx"), "nginx:latest");
        assert_eq!(norm("localhost:5000/myimg"), "myimg:latest");
        assert_eq!(norm("localhost/myimg"), "myimg:latest");
        assert_eq!(
            norm("https://registry-1.docker.io/library/nginx:latest"),
            "nginx:latest"
        );
        // 带端口歧义：repo:tag（无斜杠）不当作 registry host。
        assert_eq!(norm("nginx:8080"), "nginx:8080");
        // 命名空间镜像保留 path（仅剥 host）。
        assert_eq!(norm("ghcr.io/owner/repo:tag"), "owner/repo:tag");
    }
}
