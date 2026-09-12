// Wenyi Desktop frontend.
//
// All privileged work (dialogs, credentials, engine control) happens in Rust commands.
// This file only talks to `window.__TAURI__.core.invoke` and listens for `engine-event`,
// so it needs no bundler and no plugin JS packages.

const invoke = window.__TAURI__?.core?.invoke;
const listen = window.__TAURI__?.event?.listen;

const $ = (id) => document.getElementById(id);

const state = {
  input: null,
  lastOutputs: [],
  paths: null,
  settings: null,
};

// ── Small helpers ─────────────────────────────────────────────────────────────

function log(message) {
  const el = $("log");
  el.textContent += message + "\n";
  el.scrollTop = el.scrollHeight;
}

function clearLog() {
  $("log").textContent = "";
}

function setStatus(message, kind = "muted") {
  const el = $("settings-status");
  el.textContent = message;
  el.className = kind;
}

function baseName(path) {
  return path.split(/[\\/]/).pop() || path;
}

function dirName(path) {
  const parts = path.split(/[\\/]/);
  parts.pop();
  return parts.join("\\");
}

async function call(command, args = {}) {
  if (!invoke) throw new Error("请在 Tauri 容器中运行（tauri dev 或打包后的应用）。");
  return invoke(command, args);
}

// ── Tabs ──────────────────────────────────────────────────────────────────────

function selectTab(name) {
  for (const tab of document.querySelectorAll(".tab")) {
    tab.classList.toggle("active", tab.dataset.tab === name);
  }
  for (const panel of document.querySelectorAll(".panel")) {
    panel.classList.toggle("active", panel.id === `panel-${name}`);
  }
  if (name === "resume") refreshRuns();
}

// ── Settings ──────────────────────────────────────────────────────────────────

const PROVIDER_KEYS = {
  deepseek: "DEEPSEEK_API_KEY",
  gemini: "GEMINI_API_KEY",
  custom: null, // resolved from the custom-key-env field
};

function currentKeyAccount() {
  const provider = $("provider").value;
  if (provider === "custom") return $("custom-key-env").value.trim();
  return PROVIDER_KEYS[provider];
}

function applySettingsToForm(s) {
  $("source-lang").value = s.sourceLang;
  $("target-lang").value = s.targetLang;
  $("provider").value = s.provider;
  $("custom-base-url").value = s.customBaseUrl;
  $("custom-model").value = s.customModel;
  $("custom-key-env").value = s.customKeyEnv;
  $("proxy").value = s.proxy;
  $("polish").checked = s.polish;
  $("review").checked = s.review;
  $("book-understanding").checked = s.bookUnderstanding;
  $("bilingual").checked = s.bilingual;
  syncProviderFields();
}

function readSettingsFromForm() {
  return {
    sourceLang: $("source-lang").value,
    targetLang: $("target-lang").value,
    provider: $("provider").value,
    customBaseUrl: $("custom-base-url").value.trim(),
    customModel: $("custom-model").value.trim(),
    customKeyEnv: $("custom-key-env").value.trim(),
    proxy: $("proxy").value.trim(),
    polish: $("polish").checked,
    review: $("review").checked,
    bookUnderstanding: $("book-understanding").checked,
    bilingual: $("bilingual").checked,
    // The engine always writes a monolingual edition; the checkbox pair only adds one.
    mono: true,
  };
}

function syncProviderFields() {
  const provider = $("provider").value;
  $("custom-fields").hidden = provider !== "custom";
  const label = provider === "gemini" ? "Gemini API Key" : "DeepSeek API Key";
  $("key-label").textContent =
    provider === "custom" ? "自定义接口密钥（本地模型可留空不设置）" : label;
  refreshKeyStatus();
}

async function refreshKeyStatus() {
  const account = currentKeyAccount();
  const badge = $("key-status");
  if (!account) {
    badge.textContent = "无需密钥";
    badge.className = "badge ok";
    return;
  }
  try {
    const status = await call("api_key_status", { accounts: [account] });
    if (status[account]) {
      badge.textContent = "已保存到系统凭据库";
      badge.className = "badge ok";
    } else {
      badge.textContent = "未设置";
      badge.className = "badge missing";
    }
  } catch (error) {
    badge.textContent = "无法读取";
    badge.className = "badge unknown";
    log("密钥状态读取失败: " + error);
  }
}

async function saveSettings({ quiet = false } = {}) {
  try {
    state.settings = readSettingsFromForm();
    const paths = await call("save_settings", { settings: state.settings });
    state.paths = paths;
    renderPaths(paths);
    if (!quiet) setStatus("已保存 · " + new Date().toLocaleTimeString(), "ok");
    return true;
  } catch (error) {
    setStatus("保存失败: " + error, "error");
    return false;
  }
}

async function saveApiKey() {
  const account = currentKeyAccount();
  const secret = $("api-key").value;
  if (!account) {
    setStatus("该提供方不需要密钥。", "muted");
    return;
  }
  if (!secret.trim()) {
    setStatus("请先输入密钥。", "error");
    return;
  }
  try {
    await call("set_api_key", { account, secret: secret.trim() });
    $("api-key").value = "";
    setStatus("密钥已存入系统凭据库。", "ok");
    await refreshKeyStatus();
  } catch (error) {
    setStatus("保存密钥失败: " + error, "error");
  }
}

async function clearApiKey() {
  const account = currentKeyAccount();
  if (!account) return;
  try {
    await call("clear_api_key", { account });
    setStatus("已清除保存的密钥。", "ok");
    await refreshKeyStatus();
  } catch (error) {
    setStatus("清除失败: " + error, "error");
  }
}

function renderPaths(paths) {
  const rows = [
    ["数据目录", paths.configDir],
    ["工作目录（引擎运行于此）", paths.workspaceDir],
    ["引擎配置", paths.configFile],
    ["翻译状态 state/", paths.stateDir],
  ];
  $("paths-list").innerHTML = rows
    .map(([k, v]) => `<dt>${k}</dt><dd title="${v}">${v}</dd>`)
    .join("");
}

// ── Run control ───────────────────────────────────────────────────────────────

function buildFlags() {
  const flags = [];
  if (!$("polish").checked) flags.push("--no-polish");
  if (!$("review").checked) flags.push("--no-review");
  // The monolingual edition is the engine default; --bilingual adds a second edition.
  if ($("bilingual").checked) flags.push("--bilingual");
  return flags;
}

async function startRun(command) {
  if (!state.input) {
    log("请先选择输入文件。");
    selectTab("translate");
    return;
  }
  clearLog();
  $("output-actions").hidden = true;
  state.lastOutputs = [];
  log(`启动 ${command} …`);

  // Persist settings first: the engine's config.yaml is regenerated from them.
  if (!(await saveSettings({ quiet: true }))) return;

  const ephemeral = $("api-key").value.trim();
  try {
    await call("run_engine", {
      request: {
        command,
        input: state.input,
        flags: buildFlags(),
        ephemeralApiKey: ephemeral || null,
      },
    });
  } catch (error) {
    log("错误: " + error);
    $("progress-label").textContent = "启动失败";
  } finally {
    refreshRuns();
  }
}

function handleEvent(payload) {
  if (!payload || !payload.event) return;
  switch (payload.event) {
    case "started":
      $("progress-label").textContent = "已启动…";
      break;
    case "stage":
      $("progress-label").textContent = payload.label || "";
      break;
    case "progress": {
      const total = payload.total || 0;
      const done = payload.done || 0;
      $("progress-fill").style.width = total > 0 ? `${Math.round((done / total) * 100)}%` : "0%";
      $("progress-label").textContent =
        `${payload.label || ""}${total > 0 ? ` (${done}/${total})` : ""}`;
      break;
    }
    case "usage": {
      // The engine serialises its Python usage ledger verbatim: snake_case keys.
      const totals = payload.usage?.totals;
      const tokens = totals?.total_tokens;
      log(`用量：${typeof tokens === "number" ? tokens.toLocaleString() : "?"} tokens`);
      break;
    }
    case "done":
      if (Array.isArray(payload.outputs) && payload.outputs.length) {
        state.lastOutputs = payload.outputs;
        $("output-actions").hidden = false;
      }
      $("progress-fill").style.width = "100%";
      $("progress-label").textContent = "完成";
      log("完成：" + (payload.outputs || []).join(", "));
      break;
    case "error":
      $("progress-label").textContent = "失败";
      log("出错：" + (payload.message || JSON.stringify(payload)));
      break;
    case "terminated":
      log(`引擎退出，退出码 ${payload.exitCode}`);
      break;
    case "stderr":
      log("[引擎] " + payload.message);
      break;
    case "log":
    default:
      if (payload.message) log(payload.message);
  }
}

// ── Resume list ───────────────────────────────────────────────────────────────

async function refreshRuns() {
  const container = $("runs-list");
  try {
    const runs = await call("list_runs");
    if (!runs.length) {
      container.innerHTML = '<p class="muted">还没有运行记录。</p>';
      return;
    }
    container.innerHTML = runs
      .map((run, index) => {
        const pct =
          run.chaptersTotal > 0 ? Math.round((run.chaptersDone / run.chaptersTotal) * 100) : 0;
        const progress = run.hasState
          ? `${run.chaptersDone}/${run.chaptersTotal} 章`
          : "尚无状态";
        const title = run.title || baseName(run.input);
        const missing = run.inputExists ? "" : ' <span class="badge missing">文件缺失</span>';
        const openBtn = run.outputs.length
          ? `<button class="ghost" data-open="${index}">打开译文</button>`
          : "";
        return `
          <div class="run-row">
            <div class="run-main">
              <div class="run-title">${title}${missing}</div>
              <div class="muted run-path" title="${run.input}">${run.input}</div>
              <div class="run-progress">
                <div class="progress-bar small"><div style="width:${pct}%"></div></div>
                <span class="muted">${progress}${run.targetLang ? ` · ${run.targetLang}` : ""}</span>
              </div>
            </div>
            <div class="run-actions">
              <button class="primary" data-resume="${index}" ${run.inputExists ? "" : "disabled"}>
                继续翻译
              </button>
              ${openBtn}
            </div>
          </div>`;
      })
      .join("");

    container.querySelectorAll("[data-resume]").forEach((button) => {
      button.addEventListener("click", () => {
        const run = runs[Number(button.dataset.resume)];
        state.input = run.input;
        $("file-name").textContent = run.input;
        selectTab("translate");
        startRun("translate");
      });
    });
    container.querySelectorAll("[data-open]").forEach((button) => {
      button.addEventListener("click", async () => {
        const run = runs[Number(button.dataset.open)];
        try {
          await call("open_path", { path: run.outputs[0] });
        } catch (error) {
          log("打开失败: " + error);
        }
      });
    });
  } catch (error) {
    container.innerHTML = `<p class="muted">读取运行记录失败：${error}</p>`;
  }
}

// ── Wiring ────────────────────────────────────────────────────────────────────

async function chooseFile() {
  try {
    const picked = await call("pick_input_file");
    if (picked) {
      state.input = picked;
      $("file-name").textContent = picked;
      $("open-input-dir").disabled = false;
      log("已选择: " + picked);
    }
  } catch (error) {
    log("选择文件失败: " + error);
  }
}

async function init() {
  document.querySelectorAll(".tab").forEach((tab) => {
    tab.addEventListener("click", () => selectTab(tab.dataset.tab));
  });

  $("pick-file").addEventListener("click", chooseFile);
  $("drop-zone").addEventListener("click", chooseFile);
  $("run").addEventListener("click", () => startRun("translate"));
  $("prepare").addEventListener("click", () => startRun("prepare"));
  $("cancel").addEventListener("click", async () => {
    try {
      await call("cancel");
      log("已请求取消。");
    } catch (error) {
      log("取消失败: " + error);
    }
  });

  $("open-output").addEventListener("click", async () => {
    if (!state.lastOutputs.length) return;
    try {
      await call("open_path", { path: state.lastOutputs[0] });
    } catch (error) {
      log("打开失败: " + error);
    }
  });
  $("open-output-dir").addEventListener("click", async () => {
    const target = state.lastOutputs[0] ? dirName(state.lastOutputs[0]) : null;
    if (!target) return;
    try {
      await call("open_path", { path: target });
    } catch (error) {
      log("打开失败: " + error);
    }
  });
  $("open-input-dir").addEventListener("click", async () => {
    if (!state.input) return;
    try {
      await call("open_path", { path: dirName(state.input) });
    } catch (error) {
      log("打开失败: " + error);
    }
  });
  $("open-workspace").addEventListener("click", async () => {
    if (!state.paths) return;
    try {
      await call("open_path", { path: state.paths.workspaceDir });
    } catch (error) {
      log("打开失败: " + error);
    }
  });

  $("provider").addEventListener("change", () => {
    syncProviderFields();
    saveSettings({ quiet: true });
  });
  $("save-key").addEventListener("click", saveApiKey);
  $("clear-key").addEventListener("click", clearApiKey);
  $("save-settings").addEventListener("click", () => saveSettings());
  $("refresh-runs").addEventListener("click", refreshRuns);

  // Auto-save the run-affecting switches so the UI and config.yaml never drift.
  for (const id of [
    "source-lang",
    "target-lang",
    "polish",
    "review",
    "book-understanding",
    "bilingual",
  ]) {
    $(id).addEventListener("change", () => saveSettings({ quiet: true }));
  }

  if (listen) {
    listen("engine-event", (event) => handleEvent(event.payload));
  }

  try {
    state.paths = await call("get_paths");
    renderPaths(state.paths);
    applySettingsToForm(await call("load_settings"));
    await refreshKeyStatus();
    refreshRuns();
  } catch (error) {
    log("初始化失败: " + error);
  }
}

window.addEventListener("DOMContentLoaded", init);
