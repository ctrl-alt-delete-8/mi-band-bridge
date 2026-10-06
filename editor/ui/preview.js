// Stand-ins for the band's modules, so a generated page runs in the editor.
//
// The preview runs the page itself, not a drawing of layout.json, so what it
// shows is what the band will run. An exception shows here, where the band
// would show a black screen with no message.
(function (root) {
  const KNOWN = new Set(["BUTTON", "IMG", "TEXT", "FILL_RECT"]);

  function runPage(source) {
    const widgets = [];
    const sent = [];
    const unknown = [];
    const errors = [];

    // A missing widget type resolves to its own name, so it can be reported.
    const names = new Proxy({}, { get: (_, name) => String(name) });

    const hmUI = {
      widget: names,
      prop: names,
      createWidget(type, props) {
        if (!KNOWN.has(type)) {
          unknown.push(type);
          return { setProperty() {} };
        }
        const widget = { type, props: Object.assign({}, props) };
        widgets.push(widget);
        return {
          setProperty(prop, value) {
            if (prop === "MORE") Object.assign(widget.props, value);
          },
        };
      },
    };
    const hmBle = {
      createConnect() {},
      disConnect() {},
      connectStatus: () => true,
      addListener() {},
      removeListener() {},
      send(buffer, size) {
        const bytes = Array.from(new Uint8Array(buffer).slice(0, size));
        sent.push(bytes.map((b) => b.toString(16).padStart(2, "0")).join(""));
      },
    };
    const hmSetting = { setBrightScreen: () => 0, setBrightScreenCancel: () => 0 };
    const hmApp = { setScreenKeep() {} };
    const manager = { currentApp: { current: {} } };
    const runtime = { Page: (definition) => definition, App: (definition) => definition };

    try {
      const page = new Function(
        "__$$hmAppManager$$__", "DeviceRuntimeCore", "hmUI", "hmBle", "hmSetting", "hmApp",
        source,
      );
      page(manager, runtime, hmUI, hmBle, hmSetting, hmApp);
      const module = manager.currentApp.current.module;
      if (module && module.onInit) module.onInit();
    } catch (e) {
      errors.push(String((e && e.stack) || e));
    }

    return {
      widgets,
      sent,
      unknown,
      errors,
      // Press a widget as a finger would. The answer is the signal it sent,
      // as hex, or null when it sent nothing.
      press(index) {
        const widget = widgets[index];
        if (!widget || !widget.props.click_func) return null;
        const before = sent.length;
        try {
          widget.props.click_func();
        } catch (e) {
          errors.push(String((e && e.stack) || e));
        }
        return sent.length > before ? sent[sent.length - 1] : null;
      },
    };
  }

  if (typeof module !== "undefined" && module.exports) {
    module.exports = { runPage };
  } else {
    root.BandPreview = { runPage };
  }
})(this);
