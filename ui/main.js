// Wenyi Desktop frontend.
//
// All privileged work (dialogs, credentials, engine control) happens in Rust commands.
// This file only talks to `window.__TAURI__.core.invoke` and listens for events, so it
// needs no bundler and no plugin JS packages.
//
// Motion/feedback notes: progress is written straight through to a compositor-friendly
// transform (no tweening that could lag behind the engine), and announcements to
// assistive tech are throttled to meaningful moments rather than every progress tick.

const invoke = window.__TAURI__?.core?.invoke;
const listen = window.__TAURI__?.event?.listen;

const $ = (id) => document.getElementById(id);

const state = {
  input: null,
  lastOutputs: [],
  paths: null,
  settings: null,
  running: false,
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

function baseName(path) {
  return path.split(/[\\/]/).pop() || path;
}

function dirName(path) {
  const parts = path.split(/[\\/]/);
  parts.pop();
  return parts.join("\\");
}

let savedTimer = null;

/** Transient confirmation that settings were persisted, without stealing focus. */
function flashSaved(text = "已保存") {
  const pill = $("saved-pill");
  pill.textContent = text;
  pill.classList.add("visible");
  clearTimeout(savedTimer);
  savedTimer = setTimeout(() => pill.classList.remove("visible"), 1600);
}

/** Announce only meaningful transitions; per-batch updates would flood a screen reader. */
function announce(message) {
  $("announcer").textContent = message;
}

async function call(command, args = {}) {
  if (!invoke) throw new Error("请在 Tauri 容器中运行（tauri dev 或打包后的应用）。");
  return invoke(command, args);
}

// ── Progress ──────────────────────────────────────────────────────────────────

/**
 * Drive the bar from a 0..1 ratio.
 * @param {number|null} ratio null renders an indeterminate track.
 */
function setProgress(ratio) {
  const fill = $("progress-fill");
  const track = $("progress-track");
  const indeterminate = ratio === null;
  track.classList.toggle("indeterminate", indeterminate);
  if (indeterminate) {
    // Indeterminate: no honest number exists yet, so do not invent one. The track
    // travels instead, which reads as working rather than frozen.
    fill.style.removeProperty("--p");
    track.removeAttribute("aria-valuenow");
    return;
  }
  const clamped = Math.max(0, Math.min(1, ratio));
  fill.style.setProperty("--p", String(clamped));
  track.setAttribute("aria-valuenow", String(Math.round(clamped * 100)));
}

function setRunning(running) {
  state.running = running;
  $("cancel").disabled = !running;
  $("run").disabled = running;
  $("prepare").disabled = running;
}

// ── Tabs ──────────────────────────────────────────────────────────────────────

function selectTab(name, { focus = false } = {}) {
  for (const tab of document.querySelectorAll(".tab")) {
    const selected = tab.dataset.tab === name;
    tab.classList.toggle("active", selected);
    tab.setAttribute("aria-selected", String(selected));
    // Roving tabindex: only the active tab is in the tab order.
    tab.tabIndex = selected ? 0 : -1;
    if (selected && focus) tab.focus();
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
    // The engine always writes a monolingual edition; the checkbox only adds one.
    mono: true,
  };
}

function syncProviderFields() {
  const provider = $("provider").value;
  $("custom-fields").hidden = provider !== "custom";
  $("key-label").textContent =
    provider === "custom"
      ? "自定义接口密钥（本地模型可留空不设置）"
      : provider === "gemini"
        ? "Gemini API Key"
        : "DeepSeek API Key";
  refreshKeyStatus();
}

async function refreshKeyStatus() {
  const account = currentKeyAccount();
  const badge = $("key-status");
  updateSaveKeyButton();
  if (!account) {
    badge.textContent = "无需密钥";
    badge.className = "badge ok";
    return;
  }
  try {
    const status = await call("api_key_status", { accounts: [account] });
    if (status[account]) {
      badge.textContent = "已存入凭据库";
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

/** Inline affordance: the button is only actionable once there is something to save. */
function updateSaveKeyButton() {
  $("save-key").disabled = !currentKeyAccount() || !$("api-key").value.trim();
}

async function saveSettings({ quiet = true } = {}) {
  try {
    state.settings = readSettingsFromForm();
    state.paths = await call("save_settings", { settings: state.settings });
    renderPaths(state.paths);
    if (quiet) {
      flashSaved();
    } else {
      flashSaved("已保存");
    }
    return true;
  } catch (error) {
    flashSaved("保存失败");
    log("保存设置失败: " + error);
    return false;
  }
}

async function saveApiKey() {
  const account = currentKeyAccount();
  const secret = $("api-key").value.trim();
  if (!account) return;
  if (!secret) return;
  try {
    await call("set_api_key", { account, secret });
    $("api-key").value = "";
    updateSaveKeyButton();
    flashSaved("密钥已保存");
    await refreshKeyStatus();
  } catch (error) {
    log("保存密钥失败: " + error);
    flashSaved("密钥保存失败");
  }
}

async function clearApiKey() {
  const account = currentKeyAccount();
  if (!account) return;
  try {
    await call("clear_api_key", { account });
    flashSaved("密钥已清除");
    await refreshKeyStatus();
  } catch (error) {
    log("清除失败: " + error);
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
  setProgress(0);
  setRunning(true);
  log(`启动 ${command} …`);

  // Persist settings first: the engine's config.yaml is regenerated from them.
  await saveSettings();

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
    announce("启动失败");
  } finally {
    setRunning(false);
    refreshRuns();
  }
}

function handleEvent(payload) {
  if (!payload || !payload.event) return;
  switch (payload.event) {
    case "started":
      $("progress-label").textContent = "已启动…";
      setProgress(null);
      announce("翻译已启动");
      break;
    case "stage":
      $("progress-label").textContent = payload.label || "";
      setProgress(null);
      if (payload.label) announce(payload.label);
      break;
    case "progress": {
      const total = payload.total || 0;
      const done = payload.done || 0;
      setProgress(total > 0 ? done / total : null);
      $("progress-label").textContent =
        `${payload.label || ""}${total > 0 ? ` (${done}/${total})` : ""}`;
      break;
    }
    case "usage": {
      // The engine serialises its Python usage ledger verbatim: snake_case keys.
      const tokens = payload.usage?.totals?.total_tokens;
      log(`用量：${typeof tokens === "number" ? tokens.toLocaleString() : "?"} tokens`);
      break;
    }
    case "done":
      if (Array.isArray(payload.outputs) && payload.outputs.length) {
        state.lastOutputs = payload.outputs;
        $("output-actions").hidden = false;
      }
      setProgress(1);
      $("progress-label").textContent = "完成";
      log("完成：" + (payload.outputs || []).join(", "));
      announce("翻译完成");
      break;
    case "error":
      $("progress-label").textContent = "失败";
      log("出错：" + (payload.message || JSON.stringify(payload)));
      announce("翻译失败");
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
      // Announce a short summary rather than the whole list.
      announce("没有未完成的翻译");
      return;
    }
    const unfinished = runs.filter((r) => r.hasState && r.chaptersDone < r.chaptersTotal).length;
    announce(`${runs.length} 条运行记录，其中 ${unfinished} 条未完成`);
    container.innerHTML = runs
      .map((run, index) => {
        const ratio =
          run.chaptersTotal > 0 ? Math.min(1, run.chaptersDone / run.chaptersTotal) : 0;
        const progress = run.hasState
          ? `${run.chaptersDone}/${run.chaptersTotal} 章`
          : "尚无状态";
        const title = run.title || baseName(run.input);
        const missing = run.inputExists ? "" : ' <span class="badge missing">文件缺失</span>';
        const openBtn = run.outputs.length
          ? `<button class="ghost small" data-open="${index}">打开译文</button>`
          : "";
        return `
          <div class="run-row">
            <div class="run-main">
              <div class="run-title">${escapeHtml(title)}${missing}</div>
              <div class="muted run-path" title="${escapeHtml(run.input)}">${escapeHtml(run.input)}</div>
              <div class="run-progress">
                <div class="progress-bar small"><div class="fill" style="--p:${ratio}"></div></div>
                <span class="muted">${progress}${run.targetLang ? ` · ${run.targetLang}` : ""}</span>
              </div>
            </div>
            <div class="run-actions">
              <button class="primary small" data-resume="${index}" ${run.inputExists ? "" : "disabled"}>
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
        setInput(run.input);
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
    container.innerHTML = `<p class="muted">读取运行记录失败：${escapeHtml(String(error))}</p>`;
  }
}

/** Paths come from the filesystem and are interpolated into HTML, so escape them. */
function escapeHtml(value) {
  return String(value).replace(
    /[&<>"']/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]
  );
}

// ── Input selection ───────────────────────────────────────────────────────────

function setInput(path) {
  state.input = path;
  const name = $("file-name");
  name.textContent = path;
  name.title = path;
  $("open-input-dir").disabled = false;
}

async function chooseFile() {
  try {
    const picked = await call("pick_input_file");
    if (picked) {
      setInput(picked);
      log("已选择: " + picked);
      announce("已选择文件 " + baseName(picked));
    }
  } catch (error) {
    log("选择文件失败: " + error);
  }
}

// ── Wiring ────────────────────────────────────────────────────────────────────

const AUTO_SAVE_IDS = [
  "source-lang",
  "target-lang",
  "polish",
  "review",
  "book-understanding",
  "bilingual",
  "proxy",
  "custom-base-url",
  "custom-model",
];

async function init() {
  document.querySelectorAll(".tab").forEach((tab) => {
    tab.addEventListener("click", () => selectTab(tab.dataset.tab));
    // Standard tablist keyboard behaviour: arrows move between tabs.
    tab.addEventListener("keydown", (event) => {
      const order = ["translate", "resume", "settings"];
      const current = order.indexOf(tab.dataset.tab);
      let next = null;
      if (event.key === "ArrowRight") next = (current + 1) % order.length;
      if (event.key === "ArrowLeft") next = (current - 1 + order.length) % order.length;
      if (event.key === "Home") next = 0;
      if (event.key === "End") next = order.length - 1;
      if (next !== null) {
        event.preventDefault();
        selectTab(order[next], { focus: true });
      }
    });
  });

  const dropZone = $("drop-zone");
  dropZone.addEventListener("click", chooseFile);
  dropZone.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      chooseFile();
    }
  });

  $("run").addEventListener("click", () => startRun("translate"));
  $("prepare").addEventListener("click", () => startRun("prepare"));
  $("cancel").addEventListener("click", async () => {
    try {
      await call("cancel");
      log("已请求取消。");
      announce("已请求取消");
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
    saveSettings();
  });
  // Selecting a URL in the key field is common for local endpoints with no key.
  $("custom-key-env").addEventListener("change", () => {
    updateSaveKeyButton();
    refreshKeyStatus();
    saveSettings();
  });
  $("api-key").addEventListener("input", updateSaveKeyButton);
  $("save-key").addEventListener("click", saveApiKey);
  $("clear-key").addEventListener("click", clearApiKey);
  $("refresh-runs").addEventListener("click", refreshRuns);

  // `change` fires on blur/Enter, so text fields persist without a debounce timer.
  for (const id of AUTO_SAVE_IDS) {
    $(id).addEventListener("change", () => saveSettings());
  }

  if (listen) {
    listen("engine-event", (event) => handleEvent(event.payload));

    // Native drag-and-drop: the drop zone hover state tracks enter/over/leave so the
    // target reacts continuously while the file is held over it.
    listen("drag-state", (event) => {
      dropZone.classList.toggle("active", Boolean(event.payload?.active));
    });
    listen("file-dropped", (event) => {
      const path = event.payload?.path;
      if (!path) return;
      dropZone.classList.remove("active");
      setInput(path);
      log("已拖入: " + path);
      announce("已选择文件 " + baseName(path));
    });
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

  setRunning(false);
}

window.addEventListener("DOMContentLoaded", init);
