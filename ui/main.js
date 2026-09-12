const { invoke } = window.__TAURI__?.core ?? { invoke: null };
const { listen } = window.__TAURI__?.event ?? { listen: null };

let selectedFile = null;

const $ = (id) => document.getElementById(id);
const log = (msg) => {
  const el = $("log");
  el.textContent += msg + "\n";
  el.scrollTop = el.scrollHeight;
};

function buildEnv() {
  const env = {};
  const key = $("api-key").value.trim();
  if (key) env["DEEPSEEK_API_KEY"] = key;
  return env;
}

function buildFlags() {
  const flags = [];
  if (!$("polish").checked) flags.push("--no-polish");
  if (!$("review").checked) flags.push("--no-review");
  if ($("bilingual").checked) flags.push("--bilingual");
  return flags;
}

// Emit a synthetic config.yaml into a temp writable location is out of scope
// for the skeleton; forwarding language via flags/env is the P0 path. The real
// app will write config.yaml under %APPDATA%\wenyi first.
async function run(command) {
  if (!invoke) { log("请在 Tauri 容器中运行（tauri dev / 打包后的应用）。"); return; }
  if (!selectedFile) { log("请先选择输入文件。"); return; }
  $("log").textContent = "";
  log(`启动 ${command} ...`);
  try {
    await invoke("run_engine", {
      request: {
        command,
        input: selectedFile,
        config: null,
        flags: buildFlags(),
        env: buildEnv(),
        proxy: $("proxy").value.trim() || null,
      },
    });
  } catch (e) {
    log("错误: " + e);
  }
}

if (listen) {
  listen("engine-event", (event) => {
    const p = event.payload;
    if (!p) return;
    switch (p.event) {
      case "progress":
        const fill = $("progress-fill");
        const total = p.total || 0;
        const done = p.done || 0;
        fill.style.width = total > 0 ? `${Math.round((done / total) * 100)}%` : "0%";
        $("progress-label").textContent = `${p.label || ""} ${total > 0 ? `(${done}/${total})` : ""}`;
        break;
      case "stage":
        $("progress-label").textContent = p.label || p.stage || "";
        break;
      case "done":
        log("完成！输出: " + JSON.stringify(p.outputs ?? p.output));
        break;
      case "usage":
        log("用量: " + JSON.stringify(p));
        break;
      case "error":
        log("出错: " + (p.message || JSON.stringify(p)));
        break;
      case "terminated":
        log(`进程退出，退出码 ${p.exit_code}`);
        break;
      case "stderr":
        log("[stderr] " + p.message);
        break;
      case "log":
      default:
        log(p.message || JSON.stringify(p));
    }
  });
}

window.addEventListener("DOMContentLoaded", () => {
  $("pick-file").addEventListener("click", async () => {
    if (!window.__TAURI__?.dialog) { log("文件选择需在 Tauri 容器内使用。"); return; }
    const file = await window.__TAURI__.dialog.open({
      filters: [{ name: "书籍/字幕", extensions: ["epub","fb2","txt","md","html","pdf","docx","srt"] }],
    });
    if (file) { selectedFile = file; $("file-name").textContent = file; }
  });

  $("run").addEventListener("click", () => run("translate"));
  $("prepare").addEventListener("click", () => run("prepare"));
  $("cancel").addEventListener("click", async () => {
    if (invoke) await invoke("cancel");
    log("已请求取消。");
  });

  // Simple drag & drop → requires the fs/dialog to read an absolute path;
  // in the skeleton we only surface the dropped path via the file dialog plugin.
  const dz = $("drop-zone");
  dz.addEventListener("dragover", (e) => e.preventDefault());
  dz.addEventListener("drop", async (e) => {
    e.preventDefault();
    if (window.__TAURI__?.dialog) {
      const file = await window.__TAURI__.dialog.open({
        filters: [{ name: "书籍/字幕", extensions: ["epub","fb2","txt","md","html","pdf","docx","srt"] }],
      });
      if (file) { selectedFile = file; $("file-name").textContent = file; log("已选择: " + file); }
    }
  });
});