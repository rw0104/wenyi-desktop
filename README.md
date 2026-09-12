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

## 快速开始（三条命令）

不需要手动准备引擎：bootstrap 脚本会按锁定版本克隆上游并自动打好补丁。

```powershell
# 1. 取一份完整可用的引擎（克隆上游 @ 锁定 commit + 应用 P0 补丁）
.\engine-patch\bootstrap_engine.ps1 `
    -Dest ..\wenyi-engine `
    -Proxy http://127.0.0.1:10808      # 按需，直连可省略

# 2. 把引擎打包成 sidecar（产出 src-tauri\binaries\wenyi-core-*.exe）
.\sidecar\build_sidecar.ps1 -RepoRoot ..\wenyi-engine

# 3. 构建安装器
npm install
npm run tauri build
```

产物在 `src-tauri\target\release\bundle\`：NSIS `.exe` 安装器 + MSI。

> 也可用 `.\scripts\build.ps1` 一次跑完 2–3 步。脚本兼容 Windows PowerShell 5.1 与 PowerShell 7。
> 图标已随仓库提供；如需换成自己的图标：`npm run tauri icon .\your-icon.png`。

## 引擎补丁的完整性

上游仓库（`BigDawnGhost/wenyi`）不属于本项目，所以 P0 补丁以**固定版本 + 补丁**的方式交付，
而不是依赖任何人的本地工作区：

- `engine-patch/upstream.json` 记录上游地址与补丁所基于的 commit（`818e70b`）；
- `bootstrap_engine.ps1` 按该 commit 精确检出后再应用补丁，任何人都能得到同一份完整引擎；
- `apply.ps1` 在 checkout 版本不匹配时给出警告，避免"补丁悄悄打歪"。

因此 clone 本仓库的用户**不会拿到残缺引擎**：跑一次 bootstrap 即可得到包含 `--json-events`
的完整引擎，并可用 `uv run --no-sync pytest -q tests/test_json_events.py` 自行验证（11 项通过）。


## 开发（热重载）

```powershell
npm install
npm run dev        # 即 tauri dev，打开窗口并连接前端
```

## 功能

- **翻译**：拖入 / 选择书籍，选语言方向与流程开关（润色、审校、预扫、双语），一键运行，
  实时进度条与 token 用量，可随时取消。
- **断点续跑**：单独一页列出未完成的翻译，显示章节进度，一键继续。按**文件内容
  （SHA-256）**匹配引擎状态，重命名或移动书籍后依然能续跑。
- **设置持久化**：语言、模型、流程开关、代理等保存在应用数据目录并在每次运行时生效，
  同时自动重写引擎的 `config.yaml`。
- **API 密钥**：存入操作系统凭据库（Windows 凭据管理器 / macOS 钥匙串 / Linux kernel keyring），
  不落明文；界面只能看到"是否已设置"，引擎通过环境变量在启动瞬间拿到密钥。
- **产物打开**：完成后可直接打开译文或所在文件夹。
- **模型接入**：DeepSeek、Google Gemini，或任意 OpenAI 兼容端点（自定义 base_url + 模型 ID，
  适用于本地 Ollama / vLLM 或中转网关）。

## 关键约定

- **sidecar 命名**：Tauri v2 通过 `externalBin: ["binaries/wenyi-core"]` 识别，二进制需命名为
  `wenyi-core-<target-triple>[.exe]`（Windows 为 `x86_64-pc-windows-msvc`）。
- **JSONL 事件协议**：sidecar 以 `--json-events` 运行，stdout 为纯 JSONL，人类可读输出转到
  stderr；Rust 层把两者分别转发为前端 `engine-event`。协议细节见
  [engine-patch/README.md](engine-patch/README.md)。
- **密钥安全**：密钥只存在于系统凭据库与子进程环境变量中，绝不写入 `config.yaml`，
  也绝不出现在命令行参数里（避免进程列表泄露）。
- **工作目录**：引擎以 `<应用数据目录>\workspace` 为工作目录运行，因此 `state/` 位置固定、
  可被续跑页发现；译文仍按引擎默认规则输出到源文件旁的 `output/`。
- **前端零构建**：所有特权操作（文件对话框、凭据、引擎控制）都在 Rust 命令里实现，
  前端只调用 `invoke`，因此不需要打包器，也不依赖插件 JS 全局对象。

## 状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| P0 | 引擎 `--json-events` 事件流 + 测试 | ✅ 完成（`engine-patch/`） |
| P1 | Tauri 壳、IPC、前端、打包脚本 | ✅ 完成 |
| P2 | 设置持久化、凭据库密钥、断点续跑、产物打开 | ✅ 完成 |
| P3 | 代码签名、自动更新、多平台 CI | ⏳ 待做 |

已验证：引擎补丁可重复应用（SHA-256 逐字节一致）、bootstrap 从零得到完整引擎（11 项测试通过）、
`cargo check` 零警告、前端语法检查通过、`tauri build` 产出 NSIS + MSI 安装器。

