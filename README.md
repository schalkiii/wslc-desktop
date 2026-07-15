![icon](assets/icon@512.png)

# wslc-desktop

> 一个用于管理 **Microsoft WSL Containers**（`wslc.exe`）的原生桌面 GUI —— 纯 Rust 实现，单文件可执行，零运行时依赖。

`wslc` 是微软于 2026 Build 大会发布、2026-06-29 进入公开预览的容器方案，采用 Docker 兼容语法、底层基于 Moby，但**官方只提供 CLI，没有图形界面**。社区现有的 `lazywslc` / `lazywslcontainer` 也都是终端 TUI。`wslc-desktop` 填补这块空白：把容器、镜像、卷的日常管理搬进一个响应式的窗口应用，并提供实时的 CPU / 内存曲线。

---

## ✨ 功能特性

| 分类 | 能力 |
| --- | --- |
| **容器** | 列表（含已停止、**实时的 CPU / 内存 / 网络 I/O / 块 I/O / PIDs 列**、**点击表头排序**）、启动 / 停止 / 重启 / 强杀、删除（可强制）、批量清理（prune）、端口一键在浏览器打开 |
| **镜像** | 列表、删除、清理（prune）、从仓库拉取（pull，含超时保护）；Pull 弹窗内置**国内镜像源一键套用**（1Panel / DaoCloud / 毫秒镜像 / 南京大学 / 轩辕镜像 / rat.dev / dockerpull），自动改写注册表前缀 |
| **卷** | 列表、删除、创建（guest / vhd 驱动）、批量清理（prune） |
| **网络** | 列表、创建、删除、批量清理（prune）—— 两个 TUI 竞品均无此能力 |
| **容器创建向导（Run）** | image / name / 端口映射 / 环境变量 / 卷挂载 / 网络 / 工作目录 / 用户 / 主机名 / 内存 / CPU 数 / 入口点 / 命令 / 后台运行 / 自动删除 / 全端口发布；实时生成 `wslc run …` 预览并可复制 |
| **详情面板** | 日志（**流式跟随 `-f`**、时间戳、重新加载、全量复制）、`inspect` 原始 JSON、选中容器的 CPU% / 内存曲线图、Exec 命令执行（运行中容器） |
| **常用命令库（Saved Commands）** | 保存并持久化任意 `wslc run …` 命令行，一键运行 / 复制 / 编辑（📝）/ 删除；**点击 Name / Description / Command 表头排序**；首次启动自动从 `wslc-menu.ps1` 导入 **27 个 homelab 容器**预设；支持「恢复预设」按需补齐（不覆盖你的自定义条目） |
| **实时监控** | 后台 2s 轮询 `wslc stats`，滚动保留采样点绘制趋势图；容器列表直接展示 CPU / Mem / Mem% / Net I/O / Block I/O / PIDs |
| **体验** | 暗色 / 亮色主题切换（持久化）、按名称 / 镜像 / 驱动过滤、危险操作二次确认、操作结果 Toast、底部状态栏（计数 + 实时/暂停 + N 秒前刷新）、状态色标、**内置中文字体**（Noto Sans SC 子集，命令描述等中文正常显示，不再出现方框）、**图标 / 符号全内置**（窗口图标 + 动作按钮 / 排序指示符均随字体打包，任何主机都不会出现方框） |

> 状态映射（与 `lazywslc`/`lazywslcontainer` 交叉核对并在本机 wslc 2.9.3.0 上验证）：`1=Created`、`2=Running`、`3=Exited`、`4=Paused`。
> **已知边界**：wslc 当前不提供 `pause`/`rename`、CVE/SBOM 扫描、文件拖拽挂载、Compose —— 这些能力在本工具中不做假命令模拟。

---

## 🏗️ 架构

```
┌─────────────────────────────────────────────┐
│                UI 层 (egui/eframe)            │
│  top_bar · sidebar · 资源表 · 详情面板 · 弹窗   │
└───────────────▲───────────────┬──────────────┘
                │ WorkerEvent    │ UiRequest
        (mpsc channel，非阻塞)   │
┌───────────────┴───────────────▼──────────────┐
│           后台 worker 线程 (poller.rs)          │
│   周期轮询 + 处理 UI 请求，全部 wslc 调用在此    │
└───────────────────────┬──────────────────────┘
                        │
┌───────────────────────▼──────────────────────┐
│      wslc 后端 (src/wslc/, 与框架无关)          │
│  client(std::process) · commands · types(serde)│
└───────────────────────┬──────────────────────┘
                        │  wslc.exe --format json
                        ▼
                  Microsoft WSL Containers
```

**关键设计：**
- **UI 永不阻塞**：所有 `wslc` 子进程调用都在后台线程执行，通过 `std::sync::mpsc` 与 UI 通信；不使用 tokio。
- **不闪控制台**：Windows 下对每个子进程设置 `CREATE_NO_WINDOW`（`0x08000000`）。
- **超时可控**：每条命令带超时（默认 20s），超时即 kill 并返回错误，避免卡死。
- **后端可复用**：`src/wslc/` 完全不依赖 egui，可被 TUI / CLI / 测试独立复用。

---

## 🧰 技术栈

| 组件 | 选型 | 说明 |
| --- | --- | --- |
| GUI 框架 | **egui / eframe 0.29** | 即时模式，适合实时表格与曲线；产物为单个自包含 `.exe`，无需 WebView2 / Node |
| 绘图 | egui_plot 0.29 | CPU / 内存趋势图 |
| 序列化 | serde / serde_json | 解析 `wslc --format json` |
| 错误处理 | anyhow | |
| 时间 | chrono | 相对时间显示 |

> 为何不用 Tauri：Tauri 需要 WebView2 运行时 + 前端工具链，产物更重、依赖更多。本机未检测到 WebView2，且用户偏好「自包含、无外部依赖」，故选 egui。Tauri 作为未来重 UI 需求的备选。

---

## 📦 环境要求

- **Windows**，已安装并可用的 `wslc.exe`（本项目针对 **wslc 2.9.3.0** 验证）
- 构建需要 **Rust**（stable，`edition 2021`；开发时使用 rustc 1.95 / cargo 1.96）

验证 wslc 是否可用：

```powershell
wslc version
wslc list --all --format json
```

---

## 🚀 构建与运行

```bash
# 开发运行
cargo run

# 发布构建（strip + thin-LTO，产出精简单文件）
cargo build --release
# 产物：target/release/wslc-desktop.exe
```

> **国内网络提示**：官方 crates.io CDN 在部分网络下极慢。仓库已内置 `.cargo/config.toml`，将源替换为 **rsproxy.cn** 镜像（sparse 索引 + crate 下载）。如你的环境能直连官方源，删除该文件即可。

---

## 🗂️ 项目结构

```
wslc-desktop/
├── Cargo.toml
├── build.rs                    # Windows 可执行文件图标嵌入（winresource）
├── .cargo/config.toml          # rsproxy 镜像 + 网络容错
├── assets/                     # 应用图标 + 内置字体
│   ├── icon.ico                #   可执行文件图标（winresource 嵌入）
│   ├── icon_rgba.bin           #   运行时窗口图标（eframe 直接加载，无解码依赖）
│   ├── fonts/NotoSansSC-Subset.otf  # 中文字体子集（GB2312，~1.8 MB）
│   ├── make_icon.py            #   图标生成脚本（Pillow）
│   └── make_font.py            #   字体子集化脚本（fontTools）
├── docs/
│   └── 竞品拆解与技术方案.md      # 竞品分析 + 架构/技术选型设计文档
├── src/
│   ├── main.rs                 # eframe 入口
│   ├── app.rs                  # 应用状态、事件循环、顶层布局
│   ├── poller.rs               # 后台 worker 线程（轮询 + 请求处理）
│   ├── wslc/                   # 与框架无关的 wslc 后端
│   │   ├── client.rs           #   子进程执行、超时、去版权头
│   │   ├── commands.rs         #   各 wslc 命令封装
│   │   ├── types.rs            #   serde 数据模型 + 状态枚举 + 单位换算
│   │   └── mod.rs
│   └── ui/                     # egui 渲染（均为 WslcDesktopApp 的 impl）
│       ├── layout.rs           #   顶栏 / 侧边栏 / 资源表 / 常用命令库
│       ├── detail.rs           #   日志 / 统计 / inspect 详情面板
│       └── dialogs.rs          #   确认框 / run / 命令编辑 对话框 / Toast
└── wslc-menu.ps1               # 常用命令库预设来源：原有 PowerShell 菜单脚本
```

---

## 🆚 竞品对比

| 项目 | 语言 | 形态 | GUI | 实时监控 |
| --- | --- | --- | --- | --- |
| **wslc-desktop** | Rust | 原生窗口 | ✅ | ✅ CPU/内存曲线 |
| lazywslc | Rust | 终端 TUI | ❌ | 部分 |
| lazywslcontainer | Go (bubbletea) | 终端 TUI | ❌ | 部分 |
| Docker Desktop | 多语言 | Electron | ✅ | ✅（但不管 wslc） |

详见 [`docs/竞品拆解与技术方案.md`](docs/竞品拆解与技术方案.md)。

---

## 🛣️ 路线图

- **M1（已发布，2026-07-15）**：容器/镜像/卷/网络的增删查改、生命周期操作、容器创建向导、实时 stats 曲线、日志流式跟随、inspect、Exec、暗色主题、端口一键打开、**常用命令库（27 预设 + 持久化 + 运行/编辑）**、零 warning 编译（`cargo clippy --all-targets` 亦无告警）。
- **M2（进行中）**：日志正则搜索 / 高亮、命令库分组 / 导入导出、MSI 打包、i18n。
  - ✅ *镜像源回退快捷项* 已在 Pull 弹窗落地（国内镜像一键套用）。
  - ✅ *中文字体内置* 已完成（Noto Sans SC 子集，解决命令描述方框问题）。
  - ✅ *表头点击列排序* 已完成：容器表支持按 Name / Image / State / CPU / Mem / Mem% / Net I/O / Block I/O / PIDs / Created 点击排序（再点切换升/降序）；命令库表支持按 Name / Description / Command 排序。
  - ✅ *图标与符号全内置*：窗口图标（`icon.ico` + 运行时 `icon_rgba.bin`）与所有动作按钮、排序箭头（▲▼）一并随中文字体打包，杜绝任何主机上的方框乱码。

---

## 📄 许可

MIT
