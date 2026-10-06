// Which system the window runs on, on the root, before the page draws: the
// app says "windows" before the page's first script, and a page that hears
// nothing is the Mac's. The top bar and the few words that differ follow it
// from the first frame, so this is the one script in the head.
document.documentElement.dataset.system =
  window.__BANDBRIDGE_SYSTEM__ === "windows" ? "windows" : "macos";
