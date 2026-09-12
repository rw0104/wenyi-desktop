# Wenyi Desktop

把 Wenyi（`trans-novel` 翻译引擎）包装成带图形界面、可一键安装的原生桌面应用。
技术路线：**Tauri v2（Rust + WebView2）+ Python sidecar**。

详细架构与落地方案见 [DESIGN.md](DESIGN.md)。

## 目录

| 路径 | 作用 |
|---|---|
| `ui/` | 前端单页应用（原生 JS，无需构建工具） |
| `src-tauri/` | Rust 壳：sidecar 管理、IPC、安装器配置 |
| `engine-patch/` | **对上游引擎的 P0 补丁**（`--json-events` 事件流） |
| `sidecar/build_sidecar.ps1` | 用 PyInstaller 打包 `wenyi-core` 引擎 |
| `scripts/build.ps1` | 一键构建（sidecar + `tauri build`） |

## 架构一图

```
Tauri v2 桌面壳 (Rust + WebView2)
  ├─ 前端 UI：选书 / 语言 / 模型 / 密钥 / 代理 / 进度
  └─ Rust 命令层：spawn sidecar、转发 JSONL、注入环境变量
        │  stdin/stdout (JSONL)
  └─ sidecar：wenyi-core（PyInstaller 打包的 trans_novel）
        │  HTTPS（可走本地代理）
     DeepSeek / OpenAI / Gemini / Ollama …
```

## 前置条件

- [Node.js LTS](https://nodejs.org/)（含 npm）
- [Rust](https://rustup.rs/) + MSVC 工具链（`rustup default stable-msvc`）
- [uv](https://docs.astral.sh/uv/) 与 Python 3.10+
- 一份 Wenyi 引擎源码（默认取同级 `..\wenyi`）

## 构建步骤（Windows）

```powershell
# 0. 先给引擎打上 P0 补丁（事件流），否则 sidecar 无法被 GUI 消费
.\engine-patch\apply.ps1                       # 默认作用于 ..\wenyi

# 1. 打包 Python 引擎为 sidecar（产出 src-tauri\binaries\wenyi-core-*.exe）
.\sidecar\build_sidecar.ps1

# 2. 首次构建需生成应用图标（仓库只带图标说明，不含二进制图标）
npm install
npm run tauri icon .\path\to\icon.png          # 1024x1024 方形源图

# 3. 构建安装器
npm run tauri build
```

产物在 `src-tauri\target\release\bundle\`：NSIS `.exe` 安装器 + MSI。

> 也可用 `.\scripts\build.ps1` 一次跑完 1–3 步。
> 脚本兼容 Windows PowerShell 5.1 与 PowerShell 7。

## 开发（热重载）

```powershell
npm install
npm run dev        # 即 tauri dev，打开窗口并连接前端
```

## 关键约定

- **sidecar 命名**：Tauri v2 通过 `externalBin: ["binaries/wenyi-core"]` 识别，二进制需命名为
  `wenyi-core-<target-triple>[.exe]`（Windows 为 `x86_64-pc-windows-msvc`）。
- **JSONL 事件协议**：sidecar 以 `--json-events` 运行，stdout 为纯 JSONL，人类可读输出转到
  stderr；Rust 层把两者分别转发为前端 `engine-event`。协议细节见
  [engine-patch/README.md](engine-patch/README.md)。
- **密钥安全**：API key 仅经环境变量注入 sidecar 进程，不写入 `config.yaml`，也不出现在命令行参数中。

## 状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| P0 | 引擎 `--json-events` 事件流 + 测试 | ✅ 完成（见 `engine-patch/`） |
| P1 | Tauri 壳、IPC、前端、打包脚本 | ✅ 完成（本仓库） |
| P2 | 完整 GUI：设置持久化、断点续跑恢复、产物打开 | ⏳ 待做 |
| P3 | 代码签名、自动更新、多平台 CI | ⏳ 待做 |

P0 已通过真机验证：`--json-events` 下 stdout 为纯 JSONL，异常时必定以 `error` 事件收尾
（不会让客户端卡死），引擎既有测试与 Ruff 全绿。
