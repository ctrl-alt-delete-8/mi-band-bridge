var __$$app$$__ = __$$hmAppManager$$__.currentApp;
var __$$module$$__ = __$$app$$__.current;

// Written in conservative ES5: var, plain functions, no arrows, no const/let,
// no template literals. The band's engine does not transpile, and anything it
// cannot parse kills the whole page and shows a black screen with no error.

var SCREEN_W = 192;
var SCREEN_H = 490;
// Two pills stacked. SAFE_BOTTOM leaves a strip of bare screen below them, so
// the back swipe has somewhere to start that is not a button.
var MARGIN = 6;
var GAP = 10;
var SAFE_BOTTOM = 70;
var BW = SCREEN_W - MARGIN * 2;
var BH = (SCREEN_H - MARGIN - SAFE_BOTTOM - GAP) / 2;
var RADIUS = BW / 2;
var TOP_Y = MARGIN;
var BOTTOM_Y = MARGIN + BH + GAP;

// One signal per button. The host matches these bytes as lowercase hex.
var SIGNAL_UP = 0x02;
var SIGNAL_DOWN = 0x03;

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

function emit(second) {
  try {
    if (!ensureBle()) {
      return false;
    }
    if (hmBle.connectStatus() !== true) {
      return false;
    }
    var buf = new Uint8Array([0xbb, second]);
    hmBle.send(buf.buffer, buf.byteLength);
    return true;
  } catch (e) {
    return false;
  }
}

function pill(y, label, image, pressImage, signal) {
  return hmUI.createWidget(hmUI.widget.BUTTON, {
    x: MARGIN,
    y: y,
    w: BW,
    h: BH,
    radius: RADIUS,
    text: label,
    text_size: 34,
    color: 0xffffff,
    normal_src: image,
    press_src: pressImage,
    click_func: function () {
      emit(signal);
      // Each press starts the lit time again, so reading never runs out.
      holdScreen();
    }
  });
}

__$$module$$__.module = DeviceRuntimeCore.Page({
  onInit: function () {
    pill(TOP_Y, 'UP', 'up.png', 'up_press.png', SIGNAL_UP);
    pill(BOTTOM_Y, 'DOWN', 'down.png', 'down_press.png', SIGNAL_DOWN);
    holdScreen();
    keepPageOnWake();
  },
  onDestroy: function () {
    releaseScreen();
  }
});
