// Which of the toolbar's controls work, from what the window knows. It
// touches no page, so tests/toolbar.rs runs it under Node.
(function (root) {
  // facts: toolsMissing, whether the build tools are missing; folder,
  // whether a folder is open; apps, how many apps it holds; open, whether an
  // app is open; errors, the open app's check errors; run, "build" or
  // "install" while one runs, or null; bridge, what BandBridge said last:
  // "connected", "not_connected", "not_running", or null before it answers.
  //
  // Each control works once it has something to work on: the app picker
  // once the folder holds apps, a new app once a folder is open, and Build
  // and Install on band once an app is open. A check error stops both: what
  // is on screen cannot be built, so the last build is not it either. While
  // a build or an install runs, neither starts another. With the build tools
  // missing, nothing works.
  //
  // Install on band, and Install again on the card of an install that
  // stopped, work only while BandBridge says its band is connected, since an
  // install goes nowhere else. The title bar says where BandBridge stands.
  // Install again belongs to the install that stopped, which may be another
  // app's, so the open app's errors do not stop it.
  function controls(facts) {
    const works = !facts.toolsMissing;
    const idle = !facts.run;
    const connected = facts.bridge === "connected";
    const buildable = works && facts.open && facts.errors === 0 && idle;
    return {
      folder: works,
      apps: works && facts.apps > 0,
      newApp: works && facts.folder,
      build: buildable,
      install: buildable && connected,
      again: works && idle && connected,
    };
  }

  if (typeof module !== "undefined" && module.exports) {
    module.exports = { controls };
  } else {
    root.BandToolbar = { controls };
  }
})(this);
