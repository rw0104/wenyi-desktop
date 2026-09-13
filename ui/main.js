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

/**
 * Append one line to the log.
 * @param {string} message
 * @param {"info"|"engine"|"error"|"done"} kind drives the line's emphasis.
 */
function log(message, kind = "info") {
  const el = $("log");
  const line = document.createElement("div");
  line.className = `log-line log-${kind}`;

  const time = document.createElement("span");
  time.className = "log-time";
  time.textContent = new Date().toLocaleTimeString(undefined, { hour12: false });

  const text = document.createElement("span");
  text.className = "log-text";
  // textContent, never innerHTML: engine output is untrusted text.
  text.textContent = String(message);

  line.append(time, text);
  el.append(line);
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
  const percent = $("progress-percent");
  const indeterminate = ratio === null;
  track.classList.toggle("indeterminate", indeterminate);
  if (indeterminate) {
    // Indeterminate: no honest number exists yet, so do not invent one. The track
    // travels instead, which reads as working rather than frozen.
    fill.style.removeProperty("--p");
    track.removeAttribute("aria-valuenow");
    percent.textContent = "";
    return;
  }
  const clamped = Math.max(0, Math.min(1, ratio));
  fill.style.setProperty("--p", String(clamped));
  track.setAttribute("aria-valuenow", String(Math.round(clamped * 100)));
  // Showing the number answers "how much longer" without the user doing arithmetic.
  percent.textContent = `${Math.round(clamped * 100)}%`;
}

function setRunning(running) {
  state.running = running;
  $("cancel").disabled = !running;
  $("run").disabled = running;
  $("prepare").disabled = running;
  // The resume list offers its own start buttons; they must not be able to hijack the slot
  // while a translation occupies it.
  for (const button of document.querySelectorAll("[data-resume]")) {
    button.disabled = running;
  }
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

/** Credential name the key is stored under. Mirrors Settings::api_key_env in Rust. */
function currentKeyAccount() {
  const provider = $("provider").value;
  if (provider === "custom") {
    return $("custom-key-env").value.trim() || DEFAULT_CUSTOM_KEY_ENV;
  }
  return PROVIDER_KEYS[provider];
}

function applySettingsToForm(s) {
  $("source-lang").value = s.sourceLang;
  $("target-lang").value = s.targetLang;
  $("provider").value = s.provider;
  state.settings = s;
  // Show whatever will actually be requested, including a legacy custom model id.
  $("model-override").value = s.modelOverride || s.customModel || "";
  $("custom-base-url").value = s.customBaseUrl;
  $("custom-key-env").value = s.customKeyEnv;
  $("proxy").value = s.proxy;
  $("batch-chars").value = s.batchChars || 1800;
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
    modelOverride: $("model-override").value.trim(),
    customBaseUrl: $("custom-base-url").value.trim(),
    // Legacy field: settings written before the model field was unified still carry it.
    customModel: state.settings?.customModel || "",
    customKeyEnv: $("custom-key-env").value.trim(),
    proxy: $("proxy").value.trim(),
    batchChars: Number($("batch-chars").value) || 1800,
    polish: $("polish").checked,
    review: $("review").checked,
    bookUnderstanding: $("book-understanding").checked,
    bilingual: $("bilingual").checked,
    // The engine always writes a monolingual edition; the checkbox only adds one.
    mono: true,
  };
}

const MINERU_ACCOUNT = "MINERU_API_KEY";
const DEFAULT_CUSTOM_KEY_ENV = "CUSTOM_API_KEY";

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
  refreshEffective();
}

/**
 * Show what the engine will actually request.
 *
 * The provider selector decides everything, so without this a user can fill in the custom
 * endpoint fields, leave the provider on DeepSeek, and believe their endpoint is in use
 * while every request goes to api.deepseek.com instead.
 */
async function refreshEffective() {
  try {
    const eff = await call("get_effective_config", { settings: readSettingsFromForm() });
    $("eff-endpoint").textContent = eff.endpoint;
    $("eff-model").textContent = eff.model;
    const notes = $("eff-notes");
    if (eff.notes && eff.notes.length) {
      notes.innerHTML = eff.notes.map(escapeHtml).join("<br>");
      notes.hidden = false;
    } else {
      notes.hidden = true;
    }
  } catch (error) {
    $("eff-endpoint").textContent = "—";
    $("eff-model").textContent = "—";
  }
}

async function refreshKeyStatus() {
  await refreshModelKeyStatus();
}

let refreshModelKeyStatus = async () => {};

// Log milestones. The log is how a user tells a long run is alive, so routine progress must
// appear there -- but logging every batch would flood it (hundreds of requests per book), so
// only chapter transitions, ten-percent crossings and the resume point are reported.
let progressLog = { label: "", milestone: -1, resumed: false };

function resetProgressLog() {
  progressLog = { label: "", milestone: -1, resumed: false };
}

/** Report a stage as it starts, but never repeat the same one. */
function noteStage(label) {
  if (!label || label === progressLog.label) return;
  progressLog.label = label;
  log(`阶段：${label}`);
}
let refreshMineruStatus = async () => {};

/**
 * Wire one credential row: status badge, save, clear.
 * Shared by the translation-model key and the MinerU key so they behave identically.
 */
function wireCredential({ accountName, input, status, save, clear, label }) {
  const resolve = () =>
    typeof accountName === "function" ? accountName() : accountName;

  const refresh = async () => {
    const badge = $(status);
    const name = resolve();
    if (!name) {
      badge.textContent = "无需密钥";
      badge.className = "badge ok";
      $(save).disabled = true;
      return;
    }
    try {
      const map = await call("api_key_status", { accounts: [name] });
      badge.textContent = map[name] ? "已存入凭据库" : "未设置";
      badge.className = map[name] ? "badge ok" : "badge missing";
    } catch (error) {
      badge.textContent = "无法读取";
      badge.className = "badge unknown";
    }
    $(save).disabled = !$(input).value.trim();
  };

  $(save).addEventListener("click", async () => {
    const name = resolve();
    const secret = $(input).value.trim();
    if (!name || !secret) return;
    try {
      await call("set_api_key", { account: name, secret });
      $(input).value = "";
      flashSaved(`${label}已保存`);
      await refresh();
    } catch (error) {
      log(`保存${label}失败: ` + error, "error");
    }
  });

  $(clear).addEventListener("click", async () => {
    const name = resolve();
    if (!name) return;
    try {
      await call("clear_api_key", { account: name });
      flashSaved(`${label}已清除`);
      await refresh();
    } catch (error) {
      log(`清除${label}失败: ` + error, "error");
    }
  });

  $(input).addEventListener("input", () => {
    $(save).disabled = !$(input).value.trim();
  });

  return refresh;
}

/**
 * Ask the endpoint which models it serves.
 *
 * The engine presets pin one model id each, so without this a user cannot discover what
 * their provider actually offers now -- which is how someone ends up stuck on an old model.
 */
async function fetchModels() {
  const button = $("fetch-models");
  const status = $("models-status");
  button.disabled = true;
  status.className = "muted";
  status.textContent = "正在获取…";
  try {
    await saveSettings();
    const result = await call("list_models", {
      settings: readSettingsFromForm(),
      ephemeralApiKey: $("api-key").value.trim() || null,
    });

    const list = $("model-options");
    list.innerHTML = "";
    if (result.ok) {
      for (const id of result.models) {
        const option = document.createElement("option");
        option.value = id;
        list.append(option);
      }
      status.className = "ok";
      status.textContent = `${result.message}点输入框可选择。`;
      const current = $("model-override").value.trim();
      if (current && !result.models.includes(current)) {
        status.textContent += `（当前填写的 ${current} 不在列表中）`;
      }
      log(`获取到 ${result.models.length} 个模型：${result.models.join(", ")}`, "done");
    } else {
      status.className = "error";
      status.textContent = result.message;
      log("获取模型列表失败: " + result.message, "error");
    }
  } catch (error) {
    status.className = "error";
    status.textContent = String(error);
  } finally {
    button.disabled = false;
  }
}
async function runConnectionTest() {
  const button = $("test-connection");
  const out = $("test-result");
  button.disabled = true;
  out.className = "muted";
  out.textContent = "正在测试…";
  try {
    await saveSettings();
    const result = await call("test_connection", {
      ephemeralApiKey: $("api-key").value.trim() || null,
    });
    out.className = result.ok ? "ok" : "error";
    out.textContent = result.ok ? "连接正常 ✓" : result.message;
    log(result.ok ? "连接测试通过" : "连接测试失败: " + result.message, result.ok ? "done" : "error");
  } catch (error) {
    out.className = "error";
    out.textContent = String(error);
    log("连接测试失败: " + error, "error");
  } finally {
    button.disabled = false;
  }
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
    log("保存设置失败: " + error, "error");
    return false;
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
  resetProgressLog();
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
    log("错误: " + error, "error");
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
      noteStage(payload.label);
      if (payload.label) announce(payload.label);
      break;
    case "progress": {
      const total = payload.total || 0;
      const done = payload.done || 0;
      const label = payload.label || "";
      setProgress(total > 0 ? done / total : null);
      $("progress-label").textContent = `${label}${total > 0 ? ` (${done}/${total})` : ""}`;

      // The first count on a resumed run is the work already on disk, which answers
      // "did my resume actually pick up where it left off?".
      if (!progressLog.resumed && total > 0) {
        progressLog.resumed = true;
        if (done > 0) {
          log(`已从 ${done}/${total} 继续（${Math.round((done / total) * 100)}%），前序内容不会重译`);
        }
      }
      if (label && label !== progressLog.label) {
        progressLog.label = label;
        log(`开始翻译：${label}`, "done");
      }
      if (total > 0) {
        const milestone = Math.floor((done / total) * 10);
        if (milestone > progressLog.milestone) {
          progressLog.milestone = milestone;
          if (milestone > 0) log(`进度 ${milestone * 10}% (${done}/${total})`);
        }
      }
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
      log("完成：" + (payload.outputs || []).join(", "), "done");
      announce("翻译完成");
      break;
    case "error":
      $("progress-label").textContent = "失败";
      log("出错：" + (payload.message || JSON.stringify(payload)), "error");
      announce("翻译失败");
      break;
    case "terminated":
      log(`引擎退出，退出码 ${payload.exitCode}`, "engine");
      break;
    case "stderr":
      log(payload.message, "engine");
      break;
    case "log":
    default:
      if (payload.message) log(payload.message, "engine");
  }
}

// ── Resume list ───────────────────────────────────────────────────────────────

async function refreshRuns() {
  const container = $("runs-list");
  try {
    const runs = await call("list_runs");
    if (!runs.length) {
      // An empty state should say what this is and what to do, not just that it is empty.
      container.innerHTML = `
        <div class="empty">
          <span class="empty-title">还没有翻译记录</span>
          在「翻译」页选择一本书并开始，进度会出现在这里，随时可以中断和继续。
        </div>`;
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
              <button class="primary small" data-resume="${index}" ${run.inputExists && !state.running ? "" : "disabled"}>
                继续翻译
              </button>
              ${openBtn}
            </div>
          </div>`;
      })
      .join("");

    container.querySelectorAll("[data-resume]").forEach((button) => {
      button.addEventListener("click", () => {
        // Guard even though the button is disabled: the list may have rendered before a
        // run started, and switching books would otherwise stop the active one.
        if (state.running) {
          log("已有翻译任务在运行。请先点「取消」，或等它结束后再切换。", "error");
          return;
        }
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
          log("打开失败: " + error, "error");
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
    log("选择文件失败: " + error, "error");
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
  "batch-chars",
  "custom-base-url",
  "model-override",
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
      log("取消失败: " + error, "error");
    }
  });

  $("open-output").addEventListener("click", async () => {
    if (!state.lastOutputs.length) return;
    try {
      await call("open_path", { path: state.lastOutputs[0] });
    } catch (error) {
      log("打开失败: " + error, "error");
    }
  });
  $("open-output-dir").addEventListener("click", async () => {
    const target = state.lastOutputs[0] ? dirName(state.lastOutputs[0]) : null;
    if (!target) return;
    try {
      await call("open_path", { path: target });
    } catch (error) {
      log("打开失败: " + error, "error");
    }
  });
  $("open-input-dir").addEventListener("click", async () => {
    if (!state.input) return;
    try {
      await call("open_path", { path: dirName(state.input) });
    } catch (error) {
      log("打开失败: " + error, "error");
    }
  });
  $("open-workspace").addEventListener("click", async () => {
    if (!state.paths) return;
    try {
      await call("open_path", { path: state.paths.workspaceDir });
    } catch (error) {
      log("打开失败: " + error, "error");
    }
  });

  $("provider").addEventListener("change", () => {
    syncProviderFields();
    saveSettings();
  });
  // Both credential rows share one implementation so they cannot drift apart.
  refreshModelKeyStatus = wireCredential({
    accountName: currentKeyAccount,
    input: "api-key",
    status: "key-status",
    save: "save-key",
    clear: "clear-key",
    label: "模型密钥",
  });
  refreshMineruStatus = wireCredential({
    accountName: MINERU_ACCOUNT,
    input: "mineru-key",
    status: "mineru-status",
    save: "save-mineru",
    clear: "clear-mineru",
    label: "MinerU 密钥",
  });

  // The custom endpoint decides what is actually requested, so re-render on every edit.
  $("custom-key-env").addEventListener("change", () => {
    refreshKeyStatus();
    refreshEffective();
    saveSettings();
  });
  for (const id of ["custom-base-url", "model-override"]) {
    $(id).addEventListener("change", refreshEffective);
  }
  $("fetch-models").addEventListener("click", fetchModels);
  $("test-connection").addEventListener("click", runConnectionTest);
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
    await refreshMineruStatus();
    await refreshEffective();
    refreshRuns();
  } catch (error) {
    log("初始化失败: " + error, "error");
  }

  setRunning(false);
}

window.addEventListener("DOMContentLoaded", init);
