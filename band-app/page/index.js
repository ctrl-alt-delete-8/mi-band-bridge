var __$$app$$__ = __$$hmAppManager$$__.currentApp;
var __$$module$$__ = __$$app$$__.current;

// Written in conservative ES5: var, plain functions, no arrows, no const/let,
// no template literals. zmake leaves JS untranspiled by default (esbuild is
// off), so anything the band's engine does not parse kills the whole page and
// shows a black screen with no error.

var SCREEN_W = 192;
var SCREEN_H = 490;
// One tall pill. The margin keeps the round ends off the bezel, and SAFE_BOTTOM
// leaves a strip of bare screen so the back swipe has somewhere to start that
// is not the button.
var MARGIN = 6;
var SAFE_BOTTOM = 70;
var BX = MARGIN;
var BY = MARGIN;
var BW = SCREEN_W - MARGIN * 2;
var BH = SCREEN_H - MARGIN - SAFE_BOTTOM;
var RADIUS = BW / 2;

var mode = 0;
var button = null;
var bleReady = false;

// How long the screen stays lit while this app is open, in seconds. The band's
// own setting stops at 15, which is too short to read a screen and press a
// button. Zepp OS lets an app raise it for as long as the app runs, and the
// band goes back to its own setting when the app closes.
var SCREEN_ON_SECONDS = 120;

function holdScreen() {
  try {
    if (typeof hmSetting !== 'undefined' && hmSetting.setBrightScreen) {
      hmSetting.setBrightScreen(SCREEN_ON_SECONDS);
    }
  } catch (e) {
    // A firmware without the call must not take the page down.
  }
}

// Come back to this page when the band wakes, rather than to the watch face.
// This does not keep the screen on. It only decides what is on screen after
// the band wakes: without it, a screen off sends you back to the start.
function keepPageOnWake() {
  try {
    if (typeof hmApp !== 'undefined' && hmApp.setScreenKeep) {
      hmApp.setScreenKeep(true);
    }
  } catch (e) {
    // A firmware without the call must not take the page down.
  }
}

function releaseScreen() {
  try {
    if (typeof hmSetting !== 'undefined' && hmSetting.setBrightScreenCancel) {
      hmSetting.setBrightScreenCancel();
    }
  } catch (e) {
    // Leaving it set would drain the band, but throwing here helps nobody.
  }
}

function look(m) {
  if (m === 0) {
    return { text: 'A', normal_color: 0x1f6feb, press_color: 0x8fc0ff };
  }
  return { text: 'B', normal_color: 0xd29922, press_color: 0xffd479 };
}

// Opened lazily on first press, not during onInit: a throw there would leave
// the page half built, which is what a blank screen looks like.
function ensureBle() {
  if (bleReady) {
    return true;
  }
  try {
    if (typeof hmBle === 'undefined') {
      return false;
    }
    hmBle.createConnect(function (index, data, size) {});
    bleReady = true;
  } catch (e) {
    bleReady = false;
  }
  return bleReady;
}

// The signal. Identical on every press; the A/B toggle is visual only.
function emit() {
  try {
    if (!ensureBle()) {
      return false;
    }
    if (hmBle.connectStatus() !== true) {
      return false;
    }
    var buf = new Uint8Array([0xbb, 0x01]);
    hmBle.send(buf.buffer, buf.byteLength);
    return true;
  } catch (e) {
    return false;
  }
}

function paint(m) {
  var l = look(m);
  button.setProperty(hmUI.prop.MORE, {
    x: BX,
    y: BY,
    w: BW,
    h: BH,
    radius: RADIUS,
    text: l.text,
    text_size: 72,
    color: 0xffffff,
    normal_color: l.normal_color,
    press_color: l.press_color
  });
}

function onPress() {
  var sent = emit();
  // Each press starts the lit time again, so reading never runs out mid use.
  holdScreen();
  // The face changes only when the signal left the band. A press that went
  // nowhere leaves the face where it was, and that is the only way this app
  // can report a failure: it has no room for a message, and the host is the
  // thing that is missing.
  //
  // This means the send left the band, not that the host acted on it. The
  // band cannot know the second thing.
  if (!sent) {
    return;
  }
  mode = mode === 0 ? 1 : 0;
  try {
    paint(mode);
  } catch (e) {
    // A failed repaint must not take the page down.
  }
}

__$$module$$__.module = DeviceRuntimeCore.Page({
  onInit: function () {
    var l = look(mode);
    button = hmUI.createWidget(hmUI.widget.BUTTON, {
      x: BX,
      y: BY,
      w: BW,
      h: BH,
      radius: RADIUS,
      text: l.text,
      text_size: 72,
      color: 0xffffff,
      normal_color: l.normal_color,
      press_color: l.press_color,
      click_func: function () {
        onPress();
      }
    });
    holdScreen();
    keepPageOnWake();
  },
  onDestroy: function () {
    releaseScreen();
  }
});
