// The preview harness: the window's own page and script, in a browser, with a
// stand-in for the app that answers every call from fixtures.json. Each window
// change is screenshotted here, beside its design frame.
//
// Serve gui/ui, then open preview/index.html:
//
//   python3 -m http.server 8765 --directory gui/ui
//   http://localhost:8765/preview/index.html?fixtures=connected&open=all
//
// The address takes:
//
//   fixtures=<set>         a set from fixtures.json, connected when left out
//   system=windows         the page as it runs on Windows: the app says so
//                          before the page's first script, and the page
//                          draws Windows' top bar and words. Left out, the
//                          Mac's, with stand-ins for its window buttons.
//   appearance=light|dark|system
//                          the saved Appearance setting, which settings_get
//                          answers, so the page applies it as it does in the
//                          app: light and dark force the theme with the root
//                          class, and system follows the browser. Left out,
//                          the set's own setting stands, which is system.
//   open=all|none|<ids>    the sections to open, by id: connection, settings,
//                          events, actions, apps, arrange. Left out, each
//                          section opens as it does in the app.
//   fill.<id>=<text>       text typed into the field with that id once the
//                          page is up, before any click, such as
//                          fill.pair-name=Xiaomi%20Smart%20Band%207
//   click=<ids>            buttons to press once the page is up, in order,
//                          such as add-action,record-keys
//   wait=<ms>              time to let pass after the clicks, for answers
//                          that arrive over time
//
// When all of that is done, the root carries data-preview="ready", so a
// screenshot can wait for it. window.__preview.calls lists every call the
// page made, with its arguments. A key among them is kept by its length only.
// window.__preview.send(line, ...) hands lines to the page's next
// drain_lines, as the band task would, so a script can play a sequence at
// its own pace: a press, then Clear, then the press's late result.
//
// A set maps a command to its answer, returned as it is, unless it is one of
// these:
//
//   {"$error": "text"}             the call fails with this text
//   {"$json": value}               the value as a JSON string, the way the
//                                  commands that pass on the band's data answer
//   {"$each": [a, b, ...]}         one answer per call, in order. The last one
//                                  repeats. Under a $by, each value counts its
//                                  own calls.
//   {"$by": "arg", "values": {}}   the answer under the value of one argument
//   {"$merge": "arg",              the call's argument, merged into what the
//    "into": "command"}            command named answers, which answers so
//                                  from then on, and the merge is the answer.
//                                  A setting that is changed stays changed.
//   {"$pending": true}             the call never answers
//   {"$stream": [line, ...],       the lines reach the page one at a time,
//    "every": ms, "then": answer}  every ms (250 when left out), through
//                                  drain_lines, as a pair's lines do. Then the
//                                  call answers with `then`, which is null
//                                  when left out.
//
// A set holds only what differs from the set it names in "$extends", which is
// "base" when left out.
//
// The app never loads this page, and its page never names it. Run inside the
// app by some mistake, it stops at once rather than stand in for the app.
(() => {
  // Taken now: a script knows its own address only while it first runs.
  const here = new URL(".", document.currentScript.src);

  if (window.__TAURI_INTERNALS__) {
    document.body.textContent =
      "This is the preview harness. It runs in a browser, never in the app.";
    return;
  }

  const params = new URLSearchParams(location.search);
  const listed = (name) =>
    (params.get(name) || "").split(",").map((part) => part.trim()).filter(Boolean);
  const preview = { set: params.get("fixtures") || "connected", calls: [] };
  window.__preview = preview;

  start().catch((err) => {
    document.body.textContent = `The preview could not start: ${err}`;
    console.error(err);
  });

  async function start() {
    const sets = await fetchJson(new URL("fixtures.json", here));
    const answers = resolveSet(sets, preview.set);
    // The appearance asked for is the saved one, which the page reads.
    const saved = params.get("appearance");
    if (["light", "dark", "system"].includes(saved) && plain(answers.settings_get)) {
      answers.settings_get = { ...answers.settings_get, appearance: saved };
    }
    const calledBefore = {};
    // Lines a streaming answer has sent, for the next drain_lines to collect.
    const streamed = [];
    preview.send = (...lines) => {
      streamed.push(...lines.map((line) => structuredClone(line)));
    };

    function answer(name, value, args) {
      if (value === null || typeof value !== "object" || Array.isArray(value)) return value;
      if ("$error" in value) throw value.$error;
      if ("$json" in value) return JSON.stringify(value.$json);
      if ("$each" in value) {
        const turn = (calledBefore[name] = (calledBefore[name] ?? -1) + 1);
        return answer(name, value.$each[Math.min(turn, value.$each.length - 1)], args);
      }
      if ("$by" in value) {
        const key = args[value.$by];
        if (!(key in value.values)) {
          throw `the preview has no ${name} answer for ${value.$by} ${key}`;
        }
        return answer(`${name}/${key}`, value.values[key], args);
      }
      if ("$merge" in value) {
        if (!plain(answers[value.into])) {
          throw `the preview has no plain ${value.into} answer to merge ${name} into`;
        }
        answers[value.into] = { ...answers[value.into], ...args[value.$merge] };
        return answers[value.into];
      }
      return value;
    }

    // Whether an answer takes its time: it never comes, or its lines come
    // first.
    const later = (value) =>
      value !== null && typeof value === "object" && ("$pending" in value || "$stream" in value);

    async function answerLater(name, value, args) {
      if ("$pending" in value) return new Promise(() => {});
      for (const line of value.$stream) {
        await pause(value.every ?? 250);
        streamed.push(structuredClone(line));
      }
      const then = value.then ?? null;
      return later(then) ? answerLater(name, then, args) : answer(name, then, args);
    }

    // Rejects with a string, as the app does with a command's error.
    function invoke(name, args = {}) {
      preview.calls.push({ name, args: withoutKey(args) });
      if (!(name in answers)) {
        console.warn(`preview: no fixture for ${name}`);
        return Promise.reject(`the preview has no fixture for ${name}`);
      }
      const value = answers[name];
      if (later(value)) {
        return answerLater(name, value, args).then((result) => structuredClone(result));
      }
      try {
        const result = structuredClone(answer(name, value, args));
        if (name === "drain_lines") return Promise.resolve([...result, ...streamed.splice(0)]);
        return Promise.resolve(result);
      } catch (reason) {
        return Promise.reject(reason);
      }
    }

    // The preview keeps every call, so a key typed into it is kept by its
    // length alone, as the page itself keeps none.
    function withoutKey(args) {
      if (typeof args.key !== "string") return args;
      return { ...args, key: `(${args.key.length} characters, not kept)` };
    }

    window.__TAURI__ = {
      core: { invoke },
      // The app gives the page no event permission, so a listen delivers
      // nothing there. It delivers nothing here either.
      event: { listen: async () => () => {} },
    };
    // As the app on Windows says it, before the page's first script.
    if (params.get("system") === "windows") window.__BANDBRIDGE_SYSTEM__ = "windows";

    await loadPage(new URL("../index.html", here));

    const open = listed("open");
    if (open.length > 0) {
      for (const section of document.querySelectorAll("details.section")) {
        section.open = open.includes("all") || open.includes(section.id);
      }
    }

    // The page pulls its lines every half second.
    await pause(600);
    for (const [param, text] of params) {
      if (!param.startsWith("fill.")) continue;
      const field = document.getElementById(param.slice("fill.".length));
      if (!field) throw new Error(`the page has no field with the id ${param.slice(5)}`);
      field.value = text;
      field.dispatchEvent(new Event("input", { bubbles: true }));
    }
    for (const id of listed("click")) {
      const target = document.getElementById(id);
      if (!target) throw new Error(`the page has nothing with the id ${id}`);
      target.click();
      await pause(100);
    }
    await pause(Number(params.get("wait")) || 0);
    document.documentElement.dataset.preview = "ready";
  }

  // The page's styles and markup go into this document, then its scripts run,
  // in order. A script parsed into another document never runs, so each one
  // is made again here.
  async function loadPage(pageUrl) {
    const page = new DOMParser().parseFromString(await fetchText(pageUrl), "text/html");
    const root = document.documentElement;
    for (const { name, value } of page.documentElement.attributes) {
      root.setAttribute(name, value);
    }
    const forced = params.get("appearance");
    if (forced === "light" || forced === "dark") root.classList.add(forced);

    const scripts = [...page.querySelectorAll("script[src]")].map((script) => {
      script.remove();
      return new URL(script.getAttribute("src"), pageUrl);
    });
    // This document sits a folder below the page, so its relative addresses
    // are rebased. An in-page reference such as href="#i-watch" is left alone.
    for (const node of page.querySelectorAll("[src], link[href]")) {
      const attribute = node.hasAttribute("src") ? "src" : "href";
      node.setAttribute(attribute, new URL(node.getAttribute(attribute), pageUrl).href);
    }
    document.head.append(...page.head.querySelectorAll("style, link[rel=stylesheet]"));
    // The styles take effect before the markup arrives. With both in one
    // pass, WebKit, the app's engine, kept a stale style on <main>, and drew
    // the ground under the sections transparent.
    void getComputedStyle(root).color;
    document.body.replaceChildren(...page.body.childNodes);
    addWindowButtons();

    for (const src of scripts) {
      // A fresh address each time, so an edited script is never served stale.
      src.searchParams.set("preview", Date.now());
      await new Promise((resolve, reject) => {
        const script = document.createElement("script");
        script.src = src.href;
        script.onload = resolve;
        script.onerror = () => reject(new Error(`cannot load ${src.pathname}`));
        document.body.append(script);
      });
    }
  }

  // In the app, macOS draws the window buttons over the title bar. On
  // Windows the page draws its own.
  function addWindowButtons() {
    const bar = document.querySelector(".titlebar");
    if (!bar || window.__BANDBRIDGE_SYSTEM__ === "windows") return;
    const buttons = document.createElement("span");
    buttons.className = "preview-window-buttons";
    buttons.append(...[0, 1, 2].map(() => document.createElement("i")));
    bar.prepend(buttons);
  }

  // Whether an answer is a value as it is, with none of the forms above.
  function plain(value) {
    return value !== null && typeof value === "object" && !Array.isArray(value) &&
      !Object.keys(value).some((key) => key.startsWith("$"));
  }

  function resolveSet(sets, name, seen = []) {
    const set = sets[name];
    if (!set) throw new Error(`fixtures.json has no set named ${name}`);
    if (seen.includes(name)) throw new Error(`fixture sets extend in a circle: ${seen.join(", ")}`);
    const parent = set.$extends ?? (name === "base" ? null : "base");
    const inherited = parent ? resolveSet(sets, parent, [...seen, name]) : {};
    const own = { ...set };
    delete own.$extends;
    return { ...inherited, ...own };
  }

  async function fetchText(url) {
    const response = await fetch(url, { cache: "no-cache" });
    if (!response.ok) throw new Error(`${url.pathname}: ${response.status}`);
    return response.text();
  }

  async function fetchJson(url) {
    return JSON.parse(await fetchText(url));
  }

  function pause(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }
})();
