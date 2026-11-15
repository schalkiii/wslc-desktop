//! Windows login integration: the per-user Run key and the launch flags it
//! passes back to the app.
//!
//! "Start with Windows" is implemented as a value under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` — per-user, so no
//! elevation is ever required. The stored command line is
//!
//! ```text
//! "C:\path\to\wslc-desktop.exe" --autostart [--silent]
//! ```
//!
//! `--autostart` tells the app it was started at login (so the containers
//! checked for autostart are started too); `--silent` keeps the window hidden
//! at login, leaving only the tray icon.

use std::path::Path;

/// Registry value name under the per-user Run key.
const RUN_VALUE: &str = "wslc-desktop";

/// The per-user Run key (HKCU; no admin rights required).
#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Flags parsed from the process command line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaunchFlags {
    /// `--autostart`: launched by the Run key at login — start the containers
    /// checked for autostart.
    pub autostart: bool,
    /// `--silent`: start with the window hidden (tray only).
    pub silent: bool,
}

/// Parse `--autostart` / `--silent` out of the process arguments.
pub fn parse_launch_flags(args: impl Iterator<Item = String>) -> LaunchFlags {
    let mut flags = LaunchFlags::default();
    for arg in args {
        match arg.as_str() {
            "--autostart" => flags.autostart = true,
            "--silent" => flags.silent = true,
            _ => {}
        }
    }
    flags
}

/// The exact command line written to the Run key.
///
/// The exe path is quoted because install paths may contain spaces.
pub fn run_key_command(exe: &Path, silent: bool) -> String {
    let mut command = format!("\"{}\" --autostart", exe.display());
    if silent {
        command.push_str(" --silent");
    }
    command
}

/// Create, update, or remove the Run-key entry.
#[cfg(windows)]
pub fn set_enabled(enabled: bool, silent: bool) -> anyhow::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if enabled {
        let exe = std::env::current_exe()?;
        let (key, _) = hkcu.create_subkey(RUN_KEY)?;
        key.set_value(RUN_VALUE, &run_key_command(&exe, silent))?;
    } else if let Ok(key) = hkcu.open_subkey_with_flags(RUN_KEY, winreg::enums::KEY_SET_VALUE) {
        // A missing value (NotFound) means “already disabled”: ignore it.
        let _ = key.delete_value(RUN_VALUE);
    }
    Ok(())
}

/// The command line currently registered in the Run key, if any.
#[cfg(windows)]
pub fn registered_command() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|key| key.get_value::<String, _>(RUN_VALUE))
        .ok()
}

// Non-Windows stubs keep the crate buildable on other hosts (`cargo check`).
#[cfg(not(windows))]
pub fn set_enabled(_enabled: bool, _silent: bool) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn registered_command() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::{parse_launch_flags, run_key_command};
    use std::path::Path;

    /// 只识别 `--autostart` / `--silent`；其他参数一律忽略。
    #[test]
    fn parse_flags_recognizes_only_known_switches() {
        let flags = parse_launch_flags(
            ["--autostart", "--silent", "--other"]
                .iter()
                .map(|s| s.to_string()),
        );
        assert!(flags.autostart);
        assert!(flags.silent);

        let none = parse_launch_flags(["--unknown"].iter().map(|s| s.to_string()));
        assert_eq!(none, super::LaunchFlags::default());

        let only_silent = parse_launch_flags(["--silent"].iter().map(|s| s.to_string()));
        assert!(!only_silent.autostart);
        assert!(only_silent.silent);
    }

    /// Run 键命令：exe 路径带引号；`--silent` 决定是否追加隐藏启动参数。
    #[test]
    fn run_key_command_quotes_exe_and_appends_silent() {
        let exe = Path::new(r"C:\Program Files\wslc desktop\wslc-desktop.exe");
        assert_eq!(
            run_key_command(exe, false),
            r#""C:\Program Files\wslc desktop\wslc-desktop.exe" --autostart"#
        );
        assert_eq!(
            run_key_command(exe, true),
            r#""C:\Program Files\wslc desktop\wslc-desktop.exe" --autostart --silent"#
        );
    }
}
