// The preview harness: the editor's own page and script, in a browser, with a
// stand-in for the app that answers every call from fixtures.json. Each
// change to the window is screenshotted here, beside its design frame, as
// BandBridge's window is with gui/ui/preview.
//
// Serve editor/ui, then open preview/index.html:
//
//   python3 -m http.server 8766 --directory editor/ui
//   http://localhost:8766/preview/index.html?fixtures=start&appearance=dark
//
// The address takes:
//
//   fixtures=<set>         a set from fixtures.json, start when left out
//   appearance=light|dark  the theme to draw, which the app takes from the
//                          system. Left out, the page follows the browser.
//   system=windows         the page as it runs on Windows: the app says so,
//                          and the page draws the Windows top bar (W8).
//                          Left out, the Mac's.
//   fill.<id>=<text>       text typed into the field with that id once the
//                          page is up, before any click
//   click=<ids>            buttons to press once the page is up, in order
//   press=<selectors>      then what to press that has no id, such as a row
//                          of the Buttons pane or a button on the band's
//                          screen: CSS selectors, split by ";"
//   wait=<ms>              time to let pass after the clicks
//
// For example, fixtures=build-then-empty-folder&click=build,folder builds an
// app, then opens a folder with no apps: the start card must not keep the
// build's log. fixtures=slides&press=[data-row="2"] .brow-head;#screen
// .widget:nth-child(2) opens the third button's row, then clicks the second
// button on the screen.
//
// When all of that is done, the root carries data-preview="ready", so a
// screenshot can wait for it. window.__preview.calls lists every call the
// page made, with its arguments.
//
// A set maps a command to its answer, returned as it is, unless it is one of
// these:
//
//   {"$error": "text"}             the call fails with this text
//   {"$each": [a, b, ...]}         one answer per call, in order. The last one
//                                  repeats.
//   {"$by": "arg", "values": {}}   the answer under the value of one argument
//   {"$pending": true}             the call never answers
//   {"$bytes": "base64"}           bytes, as a command that returns a file
//                                  sends them: a list of numbers. It may sit
//                                  anywhere inside an answer.
//   {"$events": [e, ...],          the events the call sends as it runs, in
//    "$then": answer}              order, then its answer, which may be
//                                  {"$pending": true}. Each event is a line
//                                  of the Output drawer, as build_app and
//                                  install_app send them.
//   "at": "14:02:11"               a time, as today at that clock, in the
//                                  milliseconds the app sends. Anywhere.
//
// A set holds only what differs from the set it names in "$extends", which is
// "base" when left out. An answer with "$merge": true adds its keys to the
// answer it extends, so a set that changes only the preview's page keeps
// the pictures. Its "$folder" is the folder the editor opened last
// time, which the page reads from its storage. A set without one starts with
// no folder, as the editor does the first time.
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
  const preview = { set: params.get("fixtures") || "start", calls: [] };
  window.__preview = preview;

  start().catch((err) => {
    document.body.textContent = `The preview could not start: ${err}`;
    console.error(err);
  });

  async function start() {
    const sets = await fetchJson(new URL("fixtures.json", here));
    const answers = resolveSet(sets, preview.set);
    const calledBefore = {};

    function answer(name, value, args) {
      if (value === null || typeof value !== "object") return value;
      if (Array.isArray(value)) return value.map((item) => answer(name, item, args));
      if (typeof value.at === "string") value = { ...value, at: today(value.at) };
      if ("$error" in value) throw value.$error;
      if ("$bytes" in value) return Array.from(atob(value.$bytes), (c) => c.charCodeAt(0));
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
      return Object.fromEntries(
        Object.entries(value).map(([key, item]) => [key, answer(name, item, args)]),
      );
    }

    // Rejects with a string, as the app does with a command's error.
    function invoke(name, args = {}) {
      preview.calls.push({ name, args });
      if (!(name in answers)) {
        console.warn(`preview: no fixture for ${name}`);
        return Promise.reject(`the preview has no fixture for ${name}`);
      }
      let value = answers[name];
      if (value !== null && typeof value === "object" && "$events" in value) {
        for (const event of answer(name, value.$events, args)) emit("output", event);
        value = value.$then;
      }
      if (value !== null && typeof value === "object" && "$pending" in value) {
        return new Promise(() => {});
      }
      try {
        return Promise.resolve(structuredClone(answer(name, value, args)));
      } catch (reason) {
        return Promise.reject(reason);
      }
    }

    // The app's events: the page listens, and a call's "$events" reach it.
    const listeners = {};
    function listen(name, handler) {
      (listeners[name] = listeners[name] || []).push(handler);
      return Promise.resolve(() => {
        listeners[name] = listeners[name].filter((h) => h !== handler);
      });
    }
    function emit(name, payload) {
      for (const handler of listeners[name] || []) handler({ event: name, payload: structuredClone(payload) });
    }

    window.__TAURI__ = { core: { invoke }, event: { listen } };
    // The page as it runs on Windows: the app says so before its first
    // script, and the page draws the Windows top bar.
    if (params.get("system") === "windows") window.__BANDBRIDGE_SYSTEM__ = "windows";

    // The folder the editor remembers, as the app's storage would hold it.
    try {
      if (answers.$folder) localStorage.setItem("folder", answers.$folder);
      else localStorage.removeItem("folder");
    } catch (e) {
      throw new Error("the preview needs the browser's storage to set the folder");
    }

    await loadPage(new URL("../index.html", here));

    // The page asks for its folder, its apps and BandBridge's state first.
    await pause(300);
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
    for (const selector of (params.get("press") || "").split(";").map((s) => s.trim()).filter(Boolean)) {
      const target = document.querySelector(selector);
      if (!target) throw new Error(`the page has nothing that matches ${selector}`);
      target.click();
      await pause(100);
    }
    await pause(Number(params.get("wait")) || 0);
    await document.fonts.ready;
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
    // The app takes its theme from the system. Here the address can pick one.
    const forced = params.get("appearance");
    if (forced === "light" || forced === "dark") root.style.colorScheme = forced;

    const scripts = [...page.querySelectorAll("script[src]")].map((script) => {
      script.remove();
      return new URL(script.getAttribute("src"), pageUrl);
    });
    // This document sits a folder below the page, so its relative addresses
    // are rebased. An in-page reference such as href="#i-box" is left alone.
    for (const node of page.querySelectorAll("[src], link[href]")) {
      const attribute = node.hasAttribute("src") ? "src" : "href";
      node.setAttribute(attribute, new URL(node.getAttribute(attribute), pageUrl).href);
    }
    document.head.append(...page.head.querySelectorAll("style, link[rel=stylesheet]"));
    // The styles take effect before the markup arrives. With both in one
    // pass, WebKit, the app's engine, can keep a stale style.
    void getComputedStyle(root).color;
    document.body.replaceChildren(...page.body.childNodes);
    // Windows' buttons are the page's own, so only the Mac's are stood in.
    if (params.get("system") !== "windows") addWindowButtons();

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

  // In the app, macOS draws the window buttons over the title bar.
  function addWindowButtons() {
    const bar = document.querySelector(".titlebar");
    if (!bar) return;
    const buttons = document.createElement("span");
    buttons.className = "preview-window-buttons";
    buttons.append(...[0, 1, 2].map(() => document.createElement("i")));
    bar.prepend(buttons);
  }

  function resolveSet(sets, name, seen = []) {
    const set = sets[name];
    if (!set) throw new Error(`fixtures.json has no set named ${name}`);
    if (seen.includes(name)) throw new Error(`fixture sets extend in a circle: ${seen.join(", ")}`);
    const parent = set.$extends ?? (name === "base" ? null : "base");
    const inherited = parent ? resolveSet(sets, parent, [...seen, name]) : {};
    const own = { ...set };
    delete own.$extends;
    for (const [command, value] of Object.entries(own)) {
      if (value && typeof value === "object" && value.$merge) {
        const { $merge, ...keys } = value;
        own[command] = { ...inherited[command], ...keys };
      }
    }
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

  // "14:02:11" as today at that clock, in milliseconds since 1970.
  function today(clock) {
    const [h, m, sec] = clock.split(":").map(Number);
    const at = new Date();
    at.setHours(h, m, sec || 0, 0);
    return at.getTime();
  }
})();
