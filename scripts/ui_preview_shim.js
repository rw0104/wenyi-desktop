// Design-preview shim: makes the real UI renderable in a plain browser.
//
// `ui/` talks only to `window.__TAURI__.core.invoke` and `window.__TAURI__.event.listen`,
// so stubbing those two gives a faithful, populated preview of the actual markup and CSS —
// no build step, no running app. Used by scripts/preview_ui.ps1 to capture screenshots
// for design review.
//
// This file is never bundled into the application.

(() => {
  const paths = {
    configDir: "C:\\Users\\you\\AppData\\Roaming\\com.wenyi.desktop",
    workspaceDir: "C:\\Users\\you\\AppData\\Roaming\\com.wenyi.desktop\\workspace",
    configFile: "C:\\Users\\you\\AppData\\Roaming\\com.wenyi.desktop\\workspace\\config.yaml",
    stateDir: "C:\\Users\\you\\AppData\\Roaming\\com.wenyi.desktop\\workspace\\state",
    settingsFile: "C:\\Users\\you\\AppData\\Roaming\\com.wenyi.desktop\\desktop-settings.json",
    historyFile: "C:\\Users\\you\\AppData\\Roaming\\com.wenyi.desktop\\history.json",
  };

  const settings = {
    sourceLang: "ja",
    targetLang: "zh",
    provider: "deepseek",
    customBaseUrl: "",
    customModel: "",
    customKeyEnv: "",
    proxy: "http://127.0.0.1:10808",
    polish: true,
    review: true,
    bookUnderstanding: true,
    bilingual: true,
    mono: true,
  };

  const runs = [
    {
      input: "D:\\Books\\Kokoro.epub",
      inputExists: true,
      command: "translate",
      updatedAt: "1757600000",
      title: "心",
      hasState: true,
      chaptersDone: 68,
      chaptersTotal: 110,
      sourceLang: "ja",
      targetLang: "zh",
      stateDir: paths.stateDir + "\\心\\targets\\zh",
      outputDir: "D:\\Books\\output",
      outputs: [],
    },
    {
      input: "D:\\Books\\Snow Country.epub",
      inputExists: true,
      command: "translate",
      updatedAt: "1757500000",
      title: "雪国",
      hasState: true,
      chaptersDone: 46,
      chaptersTotal: 46,
      sourceLang: "ja",
      targetLang: "zh",
      stateDir: paths.stateDir + "\\雪国\\targets\\zh",
      outputDir: "D:\\Books\\output",
      outputs: ["D:\\Books\\output\\Snow Country.zh.epub"],
    },
    {
      input: "D:\\Books\\moved-elsewhere.epub",
      inputExists: false,
      command: "prepare",
      updatedAt: "1757400000",
      title: null,
      hasState: false,
      chaptersDone: 0,
      chaptersTotal: 0,
      sourceLang: null,
      targetLang: null,
      stateDir: null,
      outputDir: "D:\\Books\\output",
      outputs: [],
    },
  ];

  const listeners = {};
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  window.__TAURI__ = {
    core: {
      invoke: async (command) => {
        await sleep(0); // keep the async shape of the real bridge
        switch (command) {
          case "get_paths":
            return paths;
          case "load_settings":
            return settings;
          case "save_settings":
            return paths;
          case "api_key_status":
            return { DEEPSEEK_API_KEY: true };
          case "list_runs":
            return runs;
          case "pick_input_file":
            return "D:\\Books\\Kokoro.epub";
          case "open_path":
          case "cancel":
          case "set_api_key":
          case "clear_api_key":
          case "run_engine":
            return null;
          default:
            return null;
        }
      },
    },
    event: {
      listen: async (name, cb) => {
        listeners[name] = cb;
        return () => {};
      },
    },
  };

  // Drive the UI into a representative "translation in progress" state.
  const emit = (payload) => {
    const cb = listeners["engine-event"];
    if (cb) cb({ payload });
  };

  window.addEventListener("DOMContentLoaded", async () => {
    await sleep(60);
    document.getElementById("file-name").textContent = "D:\\Books\\Kokoro.epub";
    document.getElementById("open-input-dir").disabled = false;

    emit({ event: "started", input: "D:\\Books\\Kokoro.epub", command: "translate" });
    emit({ event: "stage", label: "Parsing document…" });
    emit({ event: "stage", label: "Analyzing book style…" });
    emit({ event: "progress", done: 3, total: 3, label: "Prescanning chapter digests" });
    emit({ event: "progress", done: 68, total: 110, label: "心 · 第二十三章" });
    emit({ event: "usage", usage: { totals: { total_tokens: 486213 } } });

    await sleep(40);
    const active = document.querySelector(".tab.active");
    if (active) active.classList.remove("active");
    const translateTab = document.querySelector('[data-tab="translate"]');
    if (translateTab) translateTab.classList.add("active");
    const panel = document.getElementById("panel-translate");
    if (panel) panel.classList.add("active");

    emit({ event: "progress", done: 68, total: 110, label: "心 · 第二十三章" });
  });
})();
