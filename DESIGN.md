# Wenyi Desktop — 架构与落地方案

> 基于 2026-09-12 技术现状，把 Wenyi（纯 Python CLI 翻译引擎 `trans-novel`）包装为带图形界面、可一键安装的原生桌面应用。
>
> 技术路线：**Tauri v2（Rust + WebView2）+ Python sidecar**。

## 1. 目标

在不修改 Wenyi 核心翻译语义的前提下，交付：

1. 一个现代桌面 GUI（拖入书籍 → 选语言/模型 → 配 API key → 一键翻译 → 实时进度 → 打开产物）。
2. 一个 Windows 安装器（NSIS `.exe`，可选 MSI）。
3. 核心引擎复用现有 `trans_novel` 源码，**零翻译逻辑改动**。

## 2. 总体架构

```text
┌──────────────────────────────────────────────────────────────┐
│  Tauri v2 桌面壳（Rust + 系统 WebView2）                       │
│  ┌────────────────────────────────────────────────────────┐  │
│  │  前端 UI（Web：原生 JS / React / Vue，随包内嵌）        │  │
│  │  · 文件选择 / 拖拽（.epub .docx .txt .srt .pdf …）     │  │
│  │  · 语言方向、润色/审校开关                              │  │
│  │  · 模型与 API key 配置（key 走系统凭据，不落明文）      │  │
│  │  · 代理设置（对接本地 10808 等 HTTP 代理）              │  │
│  │  · 实时进度条 + token 用量                              │  │
│  │  · 输出目录 / 打开产物 / 断点续跑                       │  │
│  └───────────────────────────┬────────────────────────────┘  │
│        Tauri IPC（invoke）     │  JSONL 事件流（stdout/stderr） │
│  ┌───────────────────────────▼────────────────────────────┐  │
│  │  Rust 命令层（src-tauri/src/lib.rs）                    │  │
│  │  · spawn sidecar 子进程                                 │  │
│  │  · 转发 JSONL 事件 → 前端（emit）                      │  │
│  │  · 注入环境变量（API key、代理）                       │  │
│  │  · 文件对话框、打开产物                                 │  │
│  └───────────────────────────┬────────────────────────────┘  │
│                              │ 子进程（stdin/stdout/stderr）   │
│  ┌───────────────────────────▼────────────────────────────┐  │
│  │  sidecar：wenyi-core                                    │  │
│  │  = PyInstaller 打包的 trans_novel + 新增 JSONL 事件模式  │  │
│  └─────────────────────────────────────────────────────────┘  │
│              │ HTTPS                                            │
│      DeepSeek / OpenAI / Gemini / Ollama / vLLM … API          │
└──────────────────────────────────────────────────────────────┘
```

### 2.1 为什么 sidecar 而不是直接 import

- Wenyi 的 CLI 是长流程程序（翻译数小时），且有**跨进程文件锁**（`msvcrt`/`fcntl`）、Rich 进度渲染、`interrupt_scope` 等设计。
- 作为独立子进程运行：崩溃隔离、可中断、可续跑、不阻塞 GUI 事件循环。
- sidecar 与 GUI 之间用**进程间 JSONL 事件流**通信，是 Tauri v2 官方支持的成熟模式。

## 3. 关键改造点（核心仓库需要的最小增量）

> 这些是对 `trans_novel` 的**非侵入式**增强，不影响现有 CLI 行为，也不改变 MRI-style 架构边界。

### 3.1 `--json-events` 输出模式（✅ 已实现，见 `engine-patch/`）

目标文件：`trans_novel/cli.py` + 新增 `trans_novel/json_events.py`。

现状：`translate`/`prepare`/`review` 通过 `_RichProgressBridge` + `Rich` Progress 画终端进度条，非结构化。

已落地的设计：

- 新增全局选项 `--json-events`：开启后阶段回调（`ProgressFn(done, total, label)`）改写为 **JSONL**，
  每行一条 JSON 对象，实时 flush：

```json
{"event":"stage","label":"Parsing document…"}
{"event":"progress","done":120,"total":4800,"label":"Chapter 3 · 某某章"}
{"event":"usage","usage":{...}}
{"event":"done","outputs":["D:\\...\\book.zh.epub"],"chapters_done":40,"chapters_total":40}
{"event":"error","message":"..."}
```

- **stdout 保持纯 JSONL**：开启该模式时把 Rich `console` 重新绑定到 stderr，人类可读的进度摘要与
  报错走 stderr。否则 GUI 逐行解析 stdout 会被非 JSON 行破坏。
- **必定以终止事件收尾**：除常规异常外增加兜底分支，任何未预期异常也先发 `error` 事件再非零退出，
  避免桌面客户端在死流上永久等待。非 JSON 模式下该兜底不生效，终端行为完全不变。
- **管道断开不致中断翻译**：consumer 退出导致 `BrokenPipeError` 时丢弃事件但继续翻译（可续跑）。
- **每次调用重置 sink**：在根回调里清理缓存，避免长驻进程/测试间事件写到旧的 stdout。
- 非 `--json-events` 时行为与原来完全一致（Rich 终端 UI 为默认）。

覆盖范围：`translate`、`prepare`、`review` 以及 SRT 字幕路径。


### 3.2 API key 与代理注入

sidecar 子进程启动时注入环境变量（由 Rust 层构造）：

- `DEEPSEEK_API_KEY` / `OPENAI_API_KEY` / `GEMINI_API_KEY` …（按 GUI 选择）
- `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY`（GUI 代理设置，如 `http://127.0.0.1:10808`）

密钥**不写入** `config.yaml`，也不出现在命令行参数里（避免进程列表泄露）。

### 3.3 配置文件位置（desktop 模式）

- GUI 默认在用户数据目录（`%APPDATA%\wenyi`）生成/读写 `config.yaml` 与 `state/` 输出，而不是当前工作目录。
- 通过 `--config <path>` 传入，复用现有 `_CONFIG["path"]` 逻辑，零改动。

## 4. 工程骨架目录

```text
wenyi-desktop/
├── DESIGN.md                        # 本文档
├── README.md                        # 使用与构建说明
├── package.json                     # 前端/tauri-cli 依赖（若用 npm）
├── .gitignore
├── src/                             # (可选) 前端源码，若不用纯静态
├── ui/
│   ├── index.html                   # 单页应用（原生 JS，零构建亦可）
│   ├── main.js
│   └── style.css
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json              # Tauri v2 配置 + bundler(NSIS/MSI) + 更新器
│   ├── capabilities/default.json    # 权限（shell/fs/dialog/updater）
│   ├── icons/                       # 应用图标（icon.ico 等）
│   └── src/
│       ├── main.rs
│       └── lib.rs                   # sidecar 管理 + IPC 命令
├── sidecar/
│   ├── build_sidecar.ps1            # PyInstaller 打包 wenyi-core
│   └── sidecar_manifest.json        # (可选) 声明外部 sidecar
└── scripts/
    ├── build.ps1                    # 一键：打包 sidecar + tauri build
    └── sign.ps1                     # 可选：代码签名
```

## 5. 技术要点与 2026 技术栈

### 5.1 Tauri v2 —— 为什么

- 安装包极小（约 8–20 MB），内存占用低（用系统 WebView2，不捆 Chromium）。
- 官方 `shell` 插件 + sidecar 机制，天然支持"前端壳 + Python 子进程"。
- 内置 bundler 直接产出 **NSIS `.exe`** 和 **MSI（WiX）**，附 updater 插件。

参考实现：
- https://github.com/dieharders/example-tauri-v2-python-server-sidecar
- https://github.com/longsizhuo/BossZhiPin_Job_Search/blob/master/docs/wiki/adr/005-pytauri-standalone.md
- https://zenn.dev/rayk/articles/a75921cfb695c8
- https://v2.tauri.app/reference/acl/ （权限）
- https://v2.tauri.app/reference/cli/ （tauri CLI）

### 5.2 Python 侧打包

两条路（任选，骨架默认 A）：

- **A. PyInstaller（沿用仓库现有方案）**：与 `build.yml` 相同的 `--onefile`/`--collect-all` 参数，但入口改为可打印 JSONL 的 shim。
- **B. `python-build-standalone` + 精简 venv**：更灵活的"发行版 Python + 依赖"，适合需要动态 import 的场景；对 wenyi 而言 PyInstaller 已足够。

sidecar 二进制产物放入 `src-tauri/binaries/wenyi-core-<target-triple>.exe`，Tauri 会自动识别并注入到最终包。

### 5.3 安装器与签名（解决"打不开"的根因）

- NSIS `.exe`（默认）或 MSI：`tauri.conf.json` → `bundle.targets`。
- **代码签名**：OV/EV 证书 + CI 里 `signtool sign`，否则 SmartScreen 仍会拦截。这是让用户"双击无警告"的必要投入。
- 自动更新：Tauri updater 插件 + 一个静态 JSON 签名清单。

## 6. CI/CD 矩阵

在 `.github/workflows/wenyi-desktop.yml` 中：

1. `matrix`：Windows x64（`windows-latest`）、macOS arm64/x64、Linux x64/arm64。
2. 步骤：checkout → 装 Rust/Node → `uv` 同步 → `build_sidecar.ps1`（PyInstaller）→ `tauri build` → 上传 artifact + 生成 `SHA256SUMS.txt`。
3. 可选：`signtool` 签名、发布到 GitHub Release。

## 7. 分阶段实施路线

| 阶段 | 内容 | 产出 | 状态 |
|---|---|---|---|
| P0 | `trans_novel` 增加 `--json-events` + JSONL 报告模块 + 回归测试 | 核心可被 GUI 消费 | ✅ 完成 |
| P1 | Tauri v2 骨架（本目录）+ sidecar 打包脚本 | 可 `tauri dev` 出窗口 | ✅ 完成 |
| P2 | 前端 UI：设置持久化、断点续跑恢复、产物打开、凭据库 | 最小可用桌面应用 | ⏳ 待做 |
| P3 | NSIS/MSI + CI 构建 + 代码签名 + 自动更新 | 可分发安装包 | ⏳ 待做 |

P0 的交付物以补丁形式归档在 `engine-patch/`（上游仓库非本项目所有，无法直接推送），
并用 `engine-patch/apply.ps1` 幂等地应用到引擎源码检出。已验证：应用补丁后 `trans_novel/cli.py`
与开发态逐字节一致（SHA-256 相同），`tests/test_json_events.py` 11 项通过，全仓 Ruff 通过。

## 8. 风险与边界

- sidecar 长流程需正确处理**取消/中断**：Rust 层捕获终止信号，转发给子进程（对应已有的 `interrupt_scope`）。
- 事件流需保证**顺序 & 可恢复**：JSONL 按行追加，客户端按行解析，防止进度倒退。
- 密钥安全：优先走系统凭据库（Windows Credential Manager），GUI 仅做临时内存传递。
- 不改动 wenyi 核心架构：P0 增强集中在 `cli.py` 与新增的 `trans_novel/json_events.py`，
  未触碰 pipeline/agents/glossary 的依赖方向，符合 `AGENTS.md` 的架构边界与测试要求。