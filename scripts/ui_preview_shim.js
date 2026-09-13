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

  // Mirrors the real configuration that caused a user to believe their relay endpoint was
  // in use: custom fields filled in, provider left on DeepSeek. The preview therefore shows
  // the warning that is supposed to prevent exactly that misunderstanding.
  const settings = {
    sourceLang: "auto",
    targetLang: "zh",
    provider: "custom",
    modelOverride: "deepseek-v4-pro-0813",
    customBaseUrl: "https://tokenrhythm.studio/v1",
    customModel: "",
    customKeyEnv: "",
    proxy: "",
    batchChars: 900,
    polish: true,
    review: true,
    bookUnderstanding: true,
    bilingual: false,
    mono: true,
    outputDir: "D:\\Books\\output",
    outputFormat: "",
  };

  const effective = {
    endpoint: "https://tokenrhythm.studio/v1",
    model: "deepseek-v4-pro-0813",
    providerKind: "openai-compatible",
    apiKeyEnv: "CUSTOM_API_KEY",
    customFieldsIgnored: false,
    notes: [
      "尚未存入密钥：如果这个接口需要鉴权（中转站、云服务），请把密钥填入「接口密钥」并存入凭据库。本地模型不需要密钥。",
    ],
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

  // A shelf of three: one with a cover, one without (typographic placeholder), one whose
  // file has gone missing.
  const svgCover = (title, from, to) =>
    "data:image/svg+xml;utf8," +
    encodeURIComponent(
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 300">
         <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
           <stop offset="0" stop-color="${from}"/><stop offset="1" stop-color="${to}"/>
         </linearGradient></defs>
         <rect width="200" height="300" fill="url(#g)"/>
         <text x="100" y="150" fill="#ffffff" font-size="26" font-family="serif"
               text-anchor="middle">${title}</text>
       </svg>`
    );

  const shelfBooks = [
    {
      input: "C:\\Users\\you\\Downloads\\TheEconomist.2026.09.12.epub",
      inputExists: true,
      command: "translate",
      updatedAt: "1757600000",
      title: "TheEconomist.2026.09.12",
      hasState: true,
      chaptersDone: 10,
      chaptersTotal: 21,
      sourceLang: "en",
      targetLang: "zh",
      stateDir: null,
      outputDir: "C:\\Users\\you\\Downloads\\output",
      outputs: [],
      cover: svgCover("Economist", "#1d3f57", "#0a1f2e"),
    },
    {
      input: "C:\\Users\\you\\Books\\Kokoro.txt",
      inputExists: true,
      command: "translate",
      updatedAt: "1757500000",
      title: "心",
      hasState: true,
      chaptersDone: 0,
      chaptersTotal: 0,
      sourceLang: "ja",
      targetLang: "zh",
      stateDir: null,
      outputDir: "C:\\Users\\you\\Books\\output",
      outputs: [],
      cover: null,
    },
    {
      input: "C:\\Users\\you\\Books\\moved-elsewhere.docx",
      inputExists: false,
      command: "translate",
      updatedAt: "1757400000",
      title: "Agentic Design Patterns 完整版",
      hasState: false,
      chaptersDone: 0,
      chaptersTotal: 0,
      sourceLang: "en",
      targetLang: "zh",
      stateDir: null,
      outputDir: "C:\\Users\\you\\Books\\output",
      outputs: [],
      cover: null,
    },
  ];

  const listeners = {};
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  window.__TAURI__ = {
    core: {
      invoke: async (command, args) => {
        await sleep(0); // keep the async shape of the real bridge
        switch (command) {
          case "get_paths":
            return paths;
          case "load_settings":
            return settings;
          case "save_settings":
            return paths;
          case "api_key_status":
            return { CUSTOM_API_KEY: false, MINERU_API_KEY: false };
          case "get_effective_config":
            return effective;
          case "list_library":
            return shelfBooks.map(({ cover, ...book }) => book);
          case "add_books":
            return shelfBooks.map(({ cover, ...book }) => book);
          case "remove_book":
            return shelfBooks.slice(1).map(({ cover, ...book }) => book);
          case "book_cover": {
            const found = shelfBooks.find((b) => b.input === args?.input);
            return found ? found.cover : null;
          }
          case "list_models":
            return {
              ok: true,
              models: ["deepseek-chat", "deepseek-reasoner", "deepseek-flash", "deepseek-v4-pro-0813"],
              message: "共 4 个模型。",
            };
          case "test_connection":
            return { ok: false, message: "The endpoint rejected the API key. Check that the key belongs to the endpoint shown above." };
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
    document.getElementById("file-name").textContent =
      "C:\\Users\\you\\Downloads\\TheEconomist.2026.09.12.epub";
    document.getElementById("open-input-dir").disabled = false;

    // Which screen the preview shows is chosen by the URL hash, so one staged page can be
    // captured as the shelf and as the settings screen without duplicating the markup.
    const wanted = (location.hash || "#translate").slice(1);
    const active = document.querySelector(".tab.active");
    if (active) {
      active.classList.remove("active");
      active.setAttribute("aria-selected", "false");
    }
    const chosenTab = document.querySelector(`[data-tab="${wanted}"]`);
    if (chosenTab) {
      chosenTab.classList.add("active");
      chosenTab.setAttribute("aria-selected", "true");
    }
    for (const panel of document.querySelectorAll(".panel")) {
      panel.classList.toggle("active", panel.id === `panel-${wanted}`);
    }

    emit({ event: "started", input: "D:\\Books\\Kokoro.epub", command: "translate" });
    emit({ event: "stage", label: "Parsing document…" });
    emit({
      event: "error",
      message:
        "PDF input needs a MinerU key, which is separate from the translation model key. " +
        "Add it under Settings.",
    });
    // Enter the real running state so the preview shows what a run actually looks like:
    // the shelf locks, the cancel button enables, and the chapter poll reports counters.
    if (typeof setRunning === "function") setRunning(true);

    emit({ event: "progress", done: 653, total: 1313, label: "Middle East & Africa" });
    emit({ event: "usage", usage: { totals: { total_tokens: 486213 } } });
    await sleep(60);

    // Layout probe. A screenshot shows whether the layout looks right at one size; this
    // records whether it actually fits, which is what the preview cannot judge by eye.
    // Written into the DOM so `--dump-dom` can report it back.
    const doc = document.documentElement;
    const overflow = doc.scrollWidth - doc.clientWidth;
    const wide = [];
    const past = new Set();
    for (const el of document.querySelectorAll("body *")) {
      const rect = el.getBoundingClientRect();
      if (rect.width > 0 && rect.right > doc.clientWidth + 1) {
        past.add(el);
        wide.push(`${el.tagName.toLowerCase()}.${(el.className || "").toString().split(" ")[0]}`);
      }
    }
    // An ancestor is past the edge only because a descendant is; reporting the ancestors
    // alone names the symptom. Keep the deepest elements - the ones whose own content is
    // wider than the box they were given - with the values that explain why.
    const leaves = [];
    for (const el of past) {
      let hasChildPast = false;
      for (const child of el.children) {
        if (past.has(child)) { hasChildPast = true; break; }
      }
      if (hasChildPast) continue;
      const cs = getComputedStyle(el);
      leaves.push({
        el: `${el.tagName.toLowerCase()}.${(el.className || "").toString().split(" ").slice(0, 2).join(".")}`,
        over: Math.round(el.getBoundingClientRect().right - doc.clientWidth),
        scroll: el.scrollWidth,
        client: el.clientWidth,
        minWidth: cs.minWidth,
        whiteSpace: cs.whiteSpace,
        flex: cs.flex,
        text: (el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 48),
      });
    }
    // Also record values the breakpoints are supposed to change, so a check can tell an
    // adaptation that works from a media query that never matches.
    const chrome = document.querySelector(".chrome");
    const cover = document.querySelector(".book-cover");
    const shelf = document.querySelector(".shelf");
    const main = document.querySelector("main");
    const visible = (el) => !!el && el.getBoundingClientRect().width > 0;
    const probe = document.createElement("div");
    probe.id = "layout-probe";
    probe.textContent = JSON.stringify({
      viewport: `${doc.clientWidth}x${doc.clientHeight}`,
      rootFontSize: Math.round(parseFloat(getComputedStyle(doc).fontSize) * 10) / 10,
      overflowPx: overflow,
      clippedCount: wide.length,
      clipped: wide.slice(0, 6),
      overflowSources: leaves.slice(0, 6),
      chromeDirection: chrome ? getComputedStyle(chrome).flexDirection : null,
      // Overflow is only half the question. A capped, un-centred column leaves the window
      // half empty on a maximised window - the content "does not adapt" - and nothing that
      // only looks for overflow will ever notice.
      mainWidth: main ? Math.round(main.getBoundingClientRect().width) : null,
      viewportWidth: doc.clientWidth,
      // The shelf only exists on one panel. Reporting its box while that panel is hidden
      // would print a number that looks like a measurement but is not one.
      coverWidth: visible(cover) ? Math.round(cover.getBoundingClientRect().width) : null,
      mainPadding: main ? Math.round(parseFloat(getComputedStyle(main).paddingLeft)) : null,
      shelfColumns: visible(shelf) ? getComputedStyle(shelf).gridTemplateColumns.split(" ").length : null,
    });
    document.body.append(probe);
  });
})();
