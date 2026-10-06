//! Give the focus back when a desktop switch hands it to BandBridge (macOS).
//!
//! macOS gives the focus back to the last active app of a desktop when a
//! person returns to it. Open BandBridge makes BandBridge active
//! (`open_window`), so BandBridge can be that app: back on its desktop, its
//! window comes forward with the focus, and a band press types into
//! BandBridge, not into the app the person works in.
//!
//! BandBridge asks for the focus only when a person opens it, or clicks its
//! window. So when a desktop switch makes it active, and it did not just
//! open, it hands the focus to the app that owns the frontmost ordinary
//! window on that desktop. With no other window there, it keeps the focus.
//! Its window stays open.
//!
//! A known cost: reaching BandBridge through Mission Control, by a click on
//! its window, may also hand the focus back once.

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy,
    NSApplicationDidBecomeActiveNotification, NSEvent, NSEventMask, NSEventType,
    NSRunningApplication, NSScreen, NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification,
};
use objc2_core_foundation::{CFDictionary, CFNumber, CFString, CFType, CGRect};
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGError, CGGetActiveDisplayList,
    CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption,
    kCGNullWindowID, kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer, kCGWindowOwnerPID,
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSOperationQueue, NSProcessInfo, NSRunLoop,
    NSRunLoopCommonModes, NSTimer,
};
use std::ptr::NonNull;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A switch notice and BandBridge becoming active this close together, in
/// either order, are one thing: the switch made BandBridge active.
const TOGETHER: Duration = Duration::from_secs(1);

/// How long BandBridge waits, when it became active just after a switch
/// notice, before it hands the focus back. It may be the first of two
/// quick switches, whose second notice then comes within this time: on this
/// Mac, BandBridge becomes active 0.28 to 0.37 s before a switch's notice.
const SETTLE: Duration = Duration::from_millis(500);

/// BandBridge asked for the focus itself when `open_window` ran this
/// recently.
const ASKED: Duration = Duration::from_secs(2);

/// A mouse press this recent may have made BandBridge active.
const CLICK: Duration = Duration::from_millis(500);

/// What macOS said.
#[derive(Clone, Copy, Debug)]
enum Heard {
    /// The active desktop changed. The notice comes once the switch has
    /// settled.
    Switched,
    /// BandBridge became the active app. `clicked`: a click in one of its
    /// windows did it, which is the person's own choice.
    Activated { clicked: bool },
    /// A mouse button went down in one of BandBridge's windows: the person
    /// uses BandBridge. After a click on its window while another app was
    /// active, this comes just after the activation, which may not have
    /// seen the click.
    Clicked,
}

/// What BandBridge does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Next {
    /// Nothing, for now.
    Wait,
    /// Hand the focus back now.
    GiveBack,
    /// After [`SETTLE`], ask [`Moments::settled`] about the activation at
    /// this moment.
    Settle(Instant),
}

/// What BandBridge heard and has not used yet, and when a person last
/// opened it.
#[derive(Debug, Default)]
struct Moments {
    /// A switch notice that no activation has taken.
    switched: Option<Instant>,
    /// BandBridge became active, not by a click, with no notice just before:
    /// it waits for the notice of the switch that made it active.
    activated: Option<Instant>,
    /// BandBridge became active, not by a click, just after a notice: it
    /// waits for the switch to settle, or for the notice of a second switch.
    settling: Option<Instant>,
    opened: Option<Instant>,
}

impl Moments {
    /// Note what was heard at `now`, and say what to do.
    ///
    /// A switch that made BandBridge active hands the focus back at its
    /// notice, when BandBridge became active first, as on this Mac: the
    /// switch has settled, so the window chosen is on the new desktop. When
    /// the notice came first, it hands the focus back [`SETTLE`] after the
    /// activation, unless a second notice comes first. Each notice and each
    /// activation is used once, and not after [`TOGETHER`]. A click in
    /// BandBridge's window never hands the focus back, and nothing heard
    /// before it pairs with anything after it.
    fn heard(&mut self, what: Heard, now: Instant) -> Next {
        let recent = |at: Option<Instant>| {
            at.is_some_and(|at| now.saturating_duration_since(at) <= TOGETHER)
        };
        match what {
            Heard::Activated { clicked: true } | Heard::Clicked => {
                (self.switched, self.activated, self.settling) = (None, None, None);
                Next::Wait
            }
            Heard::Activated { clicked: false } => {
                if recent(self.switched.take()) {
                    self.settling = Some(now);
                    Next::Settle(now)
                } else {
                    self.activated = Some(now);
                    Next::Wait
                }
            }
            Heard::Switched => {
                let activated = recent(self.activated.take());
                let settling = recent(self.settling.take());
                if activated || settling {
                    self.judge(now)
                } else {
                    self.switched = Some(now);
                    Next::Wait
                }
            }
        }
    }

    /// The switch before the activation at `at` has settled: hand the focus
    /// back, unless a notice took that activation meanwhile.
    fn settled(&mut self, at: Instant, now: Instant) -> Next {
        if self.settling != Some(at) {
            return Next::Wait;
        }
        self.settling = None;
        self.judge(now)
    }

    /// A switch made BandBridge active: hand the focus back, unless a person
    /// opened BandBridge just before, which asked for it.
    fn judge(&self, now: Instant) -> Next {
        let asked = self
            .opened
            .is_some_and(|opened| now.saturating_duration_since(opened) <= ASKED);
        if asked { Next::Wait } else { Next::GiveBack }
    }
}

/// A window on screen, as the window server lists it.
#[derive(Clone, Copy, Debug)]
struct Window {
    pid: i32,
    layer: i64,
    alpha: f64,
    frame: Rect,
    /// Whether its app is a regular one, with a Dock icon.
    regular: bool,
}

/// A rectangle in the window server's points, from the top left of the main
/// display.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn holds_the_middle_of(&self, other: &Rect) -> bool {
        let (x, y) = (other.x + other.w / 2.0, other.y + other.h / 2.0);
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// The app to hand the focus to: the owner of the frontmost ordinary window
/// that is not BandBridge's, in the window server's front to back order. An
/// ordinary window is on layer 0, can be seen, and belongs to a regular app,
/// one with a Dock icon. With `screens`, each display has desktops of its
/// own, so only a window on the display of BandBridge's window counts. A
/// window is on the display that holds its middle.
fn app_to_focus(windows: &[Window], own: i32, screens: Option<&[Rect]>) -> Option<i32> {
    let screen_of = |w: &Window| {
        screens?
            .iter()
            .position(|s| s.holds_the_middle_of(&w.frame))
    };
    let ours = windows
        .iter()
        .find(|w| w.pid == own && w.layer == 0)
        .and_then(screen_of);
    windows
        .iter()
        .filter(|w| w.pid != own && w.layer == 0 && w.alpha > 0.0 && w.regular)
        .find(|w| ours.is_none() || screen_of(w) == ours)
        .map(|w| w.pid)
}

/// The switch and the activation as the main thread hears them, and when a
/// person last opened BandBridge.
static MOMENTS: Mutex<Moments> = Mutex::new(Moments {
    switched: None,
    activated: None,
    settling: None,
    opened: None,
});

/// Note that a person opened BandBridge (`open_window`), so a switch that
/// follows keeps the focus.
pub fn opened() {
    if let Ok(mut moments) = MOMENTS.lock() {
        moments.opened = Some(Instant::now());
    }
}

/// Watch desktop switches for as long as BandBridge runs. Call it once, on
/// the main thread, as the app starts.
pub fn watch() {
    if MainThreadMarker::new().is_none() {
        tracing::warn!("desktop switches are not watched: this is not the main thread");
        return;
    }
    let queue = NSOperationQueue::mainQueue();
    let switched = RcBlock::new(|_: NonNull<NSNotification>| hear(Heard::Switched));
    let activated = RcBlock::new(|_: NonNull<NSNotification>| {
        let clicked = MainThreadMarker::new()
            .is_some_and(|main_thread| by_a_click(&NSApplication::sharedApplication(main_thread)));
        hear(Heard::Activated { clicked });
    });
    // SAFETY: AppKit's own names, and blocks that borrow nothing. They run
    // on the main queue, where AppKit may be used.
    let observers = unsafe {
        (
            NSWorkspace::sharedWorkspace()
                .notificationCenter()
                .addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceActiveSpaceDidChangeNotification),
                    None,
                    Some(&queue),
                    &switched,
                ),
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSApplicationDidBecomeActiveNotification),
                None,
                Some(&queue),
                &activated,
            ),
        )
    };
    // A click in BandBridge's own windows, heard as AppKit hands the mouse
    // press to them. Every event goes on unchanged.
    let presses =
        NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;
    let pressed = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        hear(Heard::Clicked);
        event.as_ptr()
    });
    // SAFETY: the block returns the event it was given, which is valid.
    let monitor =
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(presses, &pressed) };
    // Nothing removes them: they last as long as the app.
    std::mem::forget((observers, monitor));
    tracing::info!(
        "watching desktop switches, to give the focus back when one makes BandBridge active"
    );
}

/// One of the two, on the main queue.
fn hear(what: Heard) {
    let now = Instant::now();
    act(MOMENTS
        .lock()
        .map_or(Next::Wait, |mut moments| moments.heard(what, now)));
}

fn act(next: Next) {
    match next {
        Next::Wait => {}
        Next::GiveBack => {
            if let Some(main_thread) = MainThreadMarker::new() {
                give_the_focus_back(main_thread);
            }
        }
        Next::Settle(at) => after(SETTLE, move || {
            let now = Instant::now();
            act(MOMENTS
                .lock()
                .map_or(Next::Wait, |mut moments| moments.settled(at, now)));
        }),
    }
}

/// Run `then` once, `delay` from now, on the main thread, also while a menu
/// or a drag holds the run loop.
fn after(delay: Duration, then: impl Fn() + 'static) {
    let block = RcBlock::new(move |_: NonNull<NSTimer>| then());
    // SAFETY: the block holds only what `then` holds, an Instant here, and
    // runs on the main run loop.
    let timer =
        unsafe { NSTimer::timerWithTimeInterval_repeats_block(delay.as_secs_f64(), false, &block) };
    // SAFETY: AppKit's own mode name.
    unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
}

/// Whether a click in one of BandBridge's windows made it active: the event
/// it handles now is a fresh mouse press, or a mouse button is down with the
/// pointer over one of its windows. A desktop switch comes with neither.
fn by_a_click(app: &NSApplication) -> bool {
    let pressed = app.currentEvent().is_some_and(|event| {
        let age = NSProcessInfo::processInfo().systemUptime() - event.timestamp();
        matches!(
            event.r#type(),
            NSEventType::LeftMouseDown | NSEventType::RightMouseDown | NSEventType::OtherMouseDown
        ) && (0.0..CLICK.as_secs_f64()).contains(&age)
    });
    let held = NSEvent::pressedMouseButtons() != 0 && {
        let pointer = NSEvent::mouseLocation();
        app.windows().iter().any(|window| {
            let frame = window.frame();
            window.isVisible()
                && (frame.origin.x..frame.origin.x + frame.size.width).contains(&pointer.x)
                && (frame.origin.y..frame.origin.y + frame.size.height).contains(&pointer.y)
        })
    };
    pressed || held
}

/// Hand the focus to the app that owns the frontmost ordinary window on
/// BandBridge's desktop, or keep it when there is none. With macOS 14's
/// cooperative activation: BandBridge lets the other app take the focus,
/// then asks for it on the other app's behalf.
fn give_the_focus_back(main_thread: MainThreadMarker) {
    let app = NSApplication::sharedApplication(main_thread);
    // Another app has the focus already.
    if !app.isActive() {
        return;
    }
    let screens = NSScreen::screensHaveSeparateSpaces(main_thread).then(screens);
    let own = std::process::id() as i32;
    let Some(pid) = app_to_focus(&windows(), own, screens.as_deref()) else {
        tracing::info!(
            "a desktop switch made BandBridge active, and no other window is on that desktop, \
             so BandBridge keeps the focus"
        );
        return;
    };
    let Some(other) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
        tracing::info!(
            "a desktop switch made BandBridge active, and the app to give the focus to has \
             quit, so BandBridge keeps the focus"
        );
        return;
    };
    let name = other
        .localizedName()
        .map_or_else(|| format!("process {pid}"), |name| name.to_string());
    app.yieldActivationToApplication(&other);
    let this = NSRunningApplication::currentApplication();
    if other.activateFromApplication_options(&this, NSApplicationActivationOptions::empty()) {
        tracing::info!("a desktop switch made BandBridge active, so the focus went back to {name}");
    } else {
        tracing::warn!(
            "a desktop switch made BandBridge active, and macOS did not let the focus go back \
             to {name}"
        );
    }
}

/// The windows on screen, front to back.
fn windows() -> Vec<Window> {
    let options =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(listed) = CGWindowListCopyWindowInfo(options, kCGNullWindowID) else {
        return Vec::new();
    };
    // SAFETY: the window server describes each window in a dictionary with
    // string keys.
    let described = unsafe { listed.cast_unchecked::<CFDictionary<CFString, CFType>>() };
    described.iter().filter_map(|info| window(&info)).collect()
}

/// A window, from the window server's description of it.
fn window(info: &CFDictionary<CFString, CFType>) -> Option<Window> {
    // SAFETY: Core Graphics' own keys.
    let (owner, layer, alpha, bounds) = unsafe {
        (
            kCGWindowOwnerPID,
            kCGWindowLayer,
            kCGWindowAlpha,
            kCGWindowBounds,
        )
    };
    let number = |key: &CFString| info.get(key)?.downcast::<CFNumber>().ok();
    let pid = number(owner)?.as_i32()?;
    let layer = number(layer)?.as_i64()?;
    let alpha = number(alpha).and_then(|n| n.as_f64()).unwrap_or(1.0);
    let bounds = info.get(bounds)?.downcast::<CFDictionary>().ok()?;
    let mut frame = CGRect::ZERO;
    // SAFETY: the dictionary of a rectangle, and a rectangle to fill.
    if !unsafe { CGRectMakeWithDictionaryRepresentation(Some(&bounds), &mut frame) } {
        return None;
    }
    let regular = layer == 0
        && NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
            .is_some_and(|app| app.activationPolicy() == NSApplicationActivationPolicy::Regular);
    Some(Window {
        pid,
        layer,
        alpha,
        frame: rect(frame),
        regular,
    })
}

/// Each display, as a rectangle in the window server's points.
fn screens() -> Vec<Rect> {
    let mut ids: [CGDirectDisplayID; 16] = [0; 16];
    let mut count = 0;
    // SAFETY: room for 16 displays, and the count to fill.
    let listed = unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
    if listed != CGError::Success {
        return Vec::new();
    }
    ids.iter()
        .take(count as usize)
        .map(|&id| rect(CGDisplayBounds(id)))
        .collect()
}

fn rect(r: CGRect) -> Rect {
    Rect {
        x: r.origin.x,
        y: r.origin.y,
        w: r.size.width,
        h: r.size.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    const BY_A_SWITCH: Heard = Heard::Activated { clicked: false };
    const BY_A_CLICK: Heard = Heard::Activated { clicked: true };

    /// The order this Mac shows: BandBridge becomes active, and the switch's
    /// notice comes about 0.3 s later, once the switch has settled. The focus
    /// goes back at the notice, so it is given to a window on the new
    /// desktop.
    #[test]
    fn an_activation_before_the_notice_hands_the_focus_back_at_the_notice() {
        let t = Instant::now();
        let mut moments = Moments::default();
        assert_eq!(moments.heard(BY_A_SWITCH, at(t, 0)), Next::Wait);
        assert_eq!(moments.heard(Heard::Switched, at(t, 300)), Next::GiveBack);
    }

    /// The other order: the notice first, then BandBridge active. The focus
    /// goes back once the switch settles, half a second later.
    #[test]
    fn a_notice_before_the_activation_waits_for_the_switch_to_settle() {
        let t = Instant::now();
        let mut moments = Moments::default();
        assert_eq!(moments.heard(Heard::Switched, at(t, 0)), Next::Wait);
        let settling = moments.heard(BY_A_SWITCH, at(t, 200));
        assert_eq!(settling, Next::Settle(at(t, 200)));
        assert_eq!(
            moments.settled(at(t, 100), at(t, 700)),
            Next::Wait,
            "another activation's"
        );
        assert_eq!(moments.settled(at(t, 200), at(t, 700)), Next::GiveBack);
        assert_eq!(
            moments.settled(at(t, 200), at(t, 800)),
            Next::Wait,
            "only once"
        );
    }

    /// More than a second apart, the two are not one switch. A notice that
    /// ran out pairs with nothing, and the activation waits for a notice of
    /// its own.
    #[test]
    fn two_events_too_far_apart_are_not_a_switch() {
        let t = Instant::now();
        let mut moments = Moments::default();
        assert_eq!(moments.heard(BY_A_SWITCH, at(t, 0)), Next::Wait);
        assert_eq!(moments.heard(Heard::Switched, at(t, 1_001)), Next::Wait);

        let mut moments = Moments::default();
        assert_eq!(moments.heard(Heard::Switched, at(t, 0)), Next::Wait);
        assert_eq!(moments.heard(BY_A_SWITCH, at(t, 1_200)), Next::Wait);
    }

    /// Open BandBridge on the menu bar item, with its window on another
    /// desktop, switches to that desktop: BandBridge asked for the focus, and
    /// keeps it, in either order, and when the open comes between the two.
    /// An open three seconds before is not this switch.
    #[test]
    fn a_switch_right_after_open_bandbridge_keeps_the_focus() {
        let t = Instant::now();
        let opened = |ms| Moments {
            opened: Some(at(t, ms)),
            ..Moments::default()
        };
        let mut moments = opened(0);
        assert_eq!(moments.heard(BY_A_SWITCH, at(t, 50)), Next::Wait);
        assert_eq!(moments.heard(Heard::Switched, at(t, 600)), Next::Wait);

        let mut moments = Moments::default();
        assert_eq!(moments.heard(Heard::Switched, at(t, 0)), Next::Wait);
        moments.opened = Some(at(t, 100));
        assert_eq!(
            moments.heard(BY_A_SWITCH, at(t, 150)),
            Next::Settle(at(t, 150))
        );
        assert_eq!(moments.settled(at(t, 150), at(t, 650)), Next::Wait);

        let mut moments = opened(0);
        assert_eq!(moments.heard(BY_A_SWITCH, at(t, 3_000)), Next::Wait);
        assert_eq!(moments.heard(Heard::Switched, at(t, 3_300)), Next::GiveBack);
    }

    /// A person flips to desktop 2 and straight back, then clicks
    /// BandBridge's window. The notice of the switch away (0.3 s) comes just
    /// before BandBridge is active again on the return (0.5 s), and the
    /// return's own notice follows (0.8 s). The focus goes back once, at the
    /// return's notice, with the windows of the desktop the person is on.
    /// The click that follows within a second keeps the focus, and it would
    /// even if it were not seen as a click: every notice is used.
    #[test]
    fn two_quick_switches_and_a_click_hand_the_focus_back_once() {
        let t = Instant::now();
        for click in [BY_A_CLICK, BY_A_SWITCH] {
            let mut moments = Moments::default();
            assert_eq!(moments.heard(Heard::Switched, at(t, 300)), Next::Wait);
            assert_eq!(
                moments.heard(BY_A_SWITCH, at(t, 500)),
                Next::Settle(at(t, 500))
            );
            assert_eq!(moments.heard(Heard::Switched, at(t, 800)), Next::GiveBack);
            assert_eq!(moments.settled(at(t, 500), at(t, 1_000)), Next::Wait);
            assert_eq!(moments.heard(click, at(t, 1_500)), Next::Wait, "{click:?}");
            assert_eq!(
                moments.settled(at(t, 1_500), at(t, 2_000)),
                Next::Wait,
                "{click:?}"
            );
        }
    }

    /// A click in BandBridge's window is the person's own choice. It never
    /// hands the focus back, whatever came just before or after it.
    #[test]
    fn a_click_in_bandbridges_window_keeps_the_focus() {
        let t = Instant::now();
        let mut moments = Moments::default();
        assert_eq!(moments.heard(Heard::Switched, at(t, 0)), Next::Wait);
        assert_eq!(moments.heard(BY_A_CLICK, at(t, 200)), Next::Wait);
        assert_eq!(moments.settled(at(t, 200), at(t, 700)), Next::Wait);

        let mut moments = Moments::default();
        assert_eq!(moments.heard(BY_A_CLICK, at(t, 0)), Next::Wait);
        assert_eq!(moments.heard(Heard::Switched, at(t, 300)), Next::Wait);
    }

    /// A tap on BandBridge's window just after a switch notice. At the
    /// activation, AppKit may not have handed BandBridge the mouse press yet,
    /// so the activation is not seen as a click. The press itself comes 20 ms
    /// later, inside the settle wait, and keeps the focus. The same holds
    /// when the activation came before any notice.
    #[test]
    fn a_click_that_follows_its_activation_keeps_the_focus() {
        let t = Instant::now();
        let mut moments = Moments::default();
        assert_eq!(moments.heard(Heard::Switched, at(t, 0)), Next::Wait);
        assert_eq!(
            moments.heard(BY_A_SWITCH, at(t, 200)),
            Next::Settle(at(t, 200))
        );
        assert_eq!(moments.heard(Heard::Clicked, at(t, 220)), Next::Wait);
        assert_eq!(moments.settled(at(t, 200), at(t, 700)), Next::Wait);

        let mut moments = Moments::default();
        assert_eq!(moments.heard(BY_A_SWITCH, at(t, 0)), Next::Wait);
        assert_eq!(moments.heard(Heard::Clicked, at(t, 20)), Next::Wait);
        assert_eq!(moments.heard(Heard::Switched, at(t, 300)), Next::Wait);
    }

    /// A notice hands the focus back once. A later activation does not
    /// pair with it, and a notice alone takes nothing.
    #[test]
    fn a_notice_hands_the_focus_back_once() {
        let t = Instant::now();
        let mut moments = Moments::default();
        assert_eq!(moments.heard(BY_A_SWITCH, at(t, 0)), Next::Wait);
        assert_eq!(moments.heard(Heard::Switched, at(t, 300)), Next::GiveBack);
        assert_eq!(moments.heard(BY_A_SWITCH, at(t, 800)), Next::Wait);

        let mut moments = Moments::default();
        assert_eq!(moments.heard(Heard::Switched, at(t, 0)), Next::Wait);
        assert_eq!(moments.heard(Heard::Switched, at(t, 300)), Next::Wait);
    }

    const OWN: i32 = 100;

    fn window(pid: i32, frame: Rect) -> Window {
        Window {
            pid,
            layer: 0,
            alpha: 1.0,
            frame,
            regular: true,
        }
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    /// The frontmost ordinary window that is not BandBridge's, in front to
    /// back order. The window list on this Mac had a small layer 0 window of
    /// WindowManager, an app with no Dock icon, in front of the others.
    #[test]
    fn the_focus_goes_to_the_frontmost_ordinary_window() {
        let main = rect(704.0, 181.0, 583.0, 488.0);
        let windows = [
            window(OWN, main),
            Window {
                layer: 24,
                ..window(1, rect(0.0, 0.0, 1920.0, 30.0))
            },
            Window {
                regular: false,
                ..window(2, rect(859.0, 1383.0, 173.0, 30.0))
            },
            Window {
                alpha: 0.0,
                ..window(3, rect(0.0, 0.0, 800.0, 600.0))
            },
            window(4, rect(187.0, 118.0, 1018.0, 733.0)),
            window(5, rect(931.0, 97.0, 645.0, 742.0)),
        ];
        assert_eq!(app_to_focus(&windows, OWN, None), Some(4));
        assert_eq!(
            app_to_focus(&windows[..4], OWN, None),
            None,
            "no other window"
        );
        assert_eq!(app_to_focus(&[], OWN, None), None);
    }

    /// With desktops for each display, only a window on the display of
    /// BandBridge's window is on its desktop. With one desktop across the
    /// displays, every window on screen is.
    #[test]
    fn with_a_desktop_for_each_display_only_its_display_counts() {
        let screens = [
            rect(0.0, 0.0, 1920.0, 1080.0),
            rect(434.0, 1080.0, 1024.0, 576.0),
        ];
        let windows = [
            window(OWN, rect(704.0, 181.0, 583.0, 488.0)),
            window(7, rect(500.0, 1200.0, 600.0, 400.0)),
            window(8, rect(100.0, 100.0, 800.0, 600.0)),
        ];
        assert_eq!(app_to_focus(&windows, OWN, Some(&screens)), Some(8));
        assert_eq!(app_to_focus(&windows, OWN, None), Some(7));
        assert_eq!(app_to_focus(&windows[..2], OWN, Some(&screens)), None);
        // A window mostly on the other display, by its middle, is there.
        let across = [
            window(OWN, rect(704.0, 181.0, 583.0, 488.0)),
            window(9, rect(600.0, 900.0, 400.0, 500.0)),
        ];
        assert_eq!(app_to_focus(&across, OWN, Some(&screens)), None);
        // With BandBridge's window on no screen, every window counts.
        let lost = [
            window(OWN, rect(5000.0, 5000.0, 100.0, 100.0)),
            window(7, rect(500.0, 1200.0, 600.0, 400.0)),
        ];
        assert_eq!(app_to_focus(&lost, OWN, Some(&screens)), Some(7));
    }
}
