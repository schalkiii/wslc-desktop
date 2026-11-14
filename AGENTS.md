# AGENTS.md — wslc-desktop

Windows 桌面 GUI（Rust + egui/eframe），管理 Microsoft WSL Containers（`wslc.exe`）；单 exe、零运行时依赖。

## 怎么跑

- 开发：`cargo run`　·　Release：`cargo build --release` → `target/release/wslc-desktop.exe`
- 门禁：`cargo test`（50 例）+ `cargo clippy --all-targets`（零警告）
- UI 改动必须看真实窗口验证；`--silent` 启动只驻留托盘（窗口隐藏）。
- **不要**用 `exe --version` 查版本：程序没有 CLI 版本参数，会启动 GUI 阻塞命令行；用 `git describe --tags`。
- 发布：推 `v*` 标签即触发 `.github/workflows/release.yml`（windows-latest）发布 exe 资产。

## 技术栈

Rust 2021 · eframe/egui 0.29（glow + persistence）· egui_plot · serde/serde_json · anyhow · chrono · tray-icon 0.24（Windows 托盘，`default-features=false`）· winreg 0.55（HKCU Run 键）。crates 源走 rsproxy 镜像（`.cargo/config.toml`；CI 里会临时移除）。

## 目录与约定

- `src/wslc/`：GUI 无关后端——`client.rs` 进程执行（超时 + `CREATE_NO_WINDOW`）、`commands.rs` 封装、`types.rs` serde 双 schema（wslc 2.x 数组 / 3.x NDJSON）。
- `src/poller.rs`：后台线程 2s 轮询 + 通道；**UI 线程绝不阻塞**，一切 wslc 调用走 worker。
- `src/app.rs` / `src/ui/`：状态与渲染（`layout.rs` 表格、`detail.rs` 详情、`dialogs.rs` 弹窗）。
- `src/autostart.rs`：HKCU Run 键 + `--autostart` / `--silent` 解析；`src/tray.rs`：托盘图标（256→32 盒式降采样）。
- 字体/图标全内置（`assets/`）：中文走 Noto Sans SC 子集；新增 UI 符号前确认字形在子集里（有单测护栏）。
- 预设命令一律占位化入库；真实凭据只进 gitignored `presets.local.json`。
- 文档同步：功能变更同 commit 更新 README（中英）、`CHANGELOG.md`、`docs/测试计划.md`。

## 关键不变量（都是踩过的坑）

- wslc 的 `container/image/volume/network prune` **没有 `--force`**，传了报「无法识别当前命令的参数名称」。
- UI 缩放必须在 `update()` 首帧应用 `set_pixels_per_point`；在 `new()` 里调用会与未知的原生缩放相乘（150% 屏启动放大到 2.25×）。
- 隐藏窗口（`--silent`）下 eframe 事件循环仍运行；关闭拦截 = `close_requested()` + `CancelClose` + `Visible(false)`，真正退出前先置 `quitting` 标志（否则被「关闭到托盘」吞掉）。
- 改窗口行为后可用无头方式验证：`taskkill.exe /PID <pid>`（不带 `/F`，即 WM_CLOSE）后进程仍存活 ⇒ 关闭到托盘生效。
- Git Bash 调 `taskkill.exe`/`reg.exe` 必须加 `MSYS2_ARG_CONV_EXCL='*'`，否则 `/PID`、`/F` 会被转成路径。

## 当前状态与下一步

- v0.3.0（2026-11-14）：M1–M3 已实现（CRUD/生命周期/向导/日志/统计/命令库/缩放/开机自启 + 托盘 + 逐容器自启 + 自启后退出）。
- 下一步与路线图以 README「Roadmap」为准；不要在此文件维护第二份路线图。
