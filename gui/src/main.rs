//! The BandBridge window.
//!
//! This holds no behavior of its own. Every button calls the same function in
//! the `bandbridge` library that the matching command calls, so the two front
//! ends cannot drift apart.
//!
//! While this window holds the band, it also answers the socket, so every
//! command keeps working, including ones this window has no button for.
//! Pairing, forgetting and the key are this window's alone: it asks the band
//! task for them on its own channel, and the socket refuses them.

// On Windows a release build opens no console window beside the window. A
// debug build keeps its console, for the log. macOS ignores this.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bandbridge::action::Outcome;
use bandbridge::config;
use bandbridge::error::BandError;
use bandbridge::event::Record;
use bandbridge::ipc;
use bandbridge::supervisor::{
    Command as SupCommand, ConnectOptions, NO_KEY_TO_CONNECT, Standing, Status, Supervisor,
};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tokio::sync::mpsc;

#[cfg(target_os = "macos")]
mod desktop_switch;
mod recorder;

/// One line for the window to show.
#[derive(Clone, Serialize)]
struct Line {
    /// What the line is, so the page can tell them apart: `state` (for the
    /// pill and the connection card), `event` (a record for the event log),
    /// `error` (a connection problem, for the card), or `pair` (how a pair
    /// goes, for the setup screen). What the band task says as it goes is
    /// for developers, and never reaches the page: see [`line_for`].
    ///
    /// Two more come from the window as it starts, once each at most:
    /// `config` (the config file could not be read, and why: no action fires
    /// until it is fixed) and `permission` (an action posts keystrokes, and
    /// macOS does not let this program post them).
    kind: &'static str,
    /// The words, on every line but an `event`. On a `state` line, the
    /// radio's word: `ready`, `scanning`, `backoff (attempt 2)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    /// On a `state` line, where the connection stands: `state` (connected,
    /// connecting, retrying or disconnected), `reason` (started, you, dropped
    /// or failed), and `since`, local time in RFC 3339. The page words them.
    #[serde(skip_serializing_if = "Option::is_none")]
    connection: Option<Standing>,
    /// On an `event` line, the record for the event log: a press, with its
    /// signal, the action it fired and how that went, or an app installed or
    /// removed. A press whose action takes time comes a second time, with
    /// its result, under the same `id`. See [`Record`].
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<Record>,
}

impl Line {
    /// A line of words, with no connection state on it.
    fn new(kind: &'static str, text: String) -> Self {
        Self {
            kind,
            text: Some(text),
            connection: None,
            event: None,
        }
    }

    /// A record for the event log.
    fn event(record: Record) -> Self {
        Self {
            kind: "event",
            text: None,
            connection: None,
            event: Some(record),
        }
    }
}

/// Lines waiting for the page to collect.
///
/// This lives apart from the band on purpose. It used to hang off the band
/// task, so a failure to start that task left nothing to read and the window
/// sat on "starting" with no reason given. The buffer now exists before the
/// band does, and outlives a band that never came.
#[derive(Default, Clone)]
struct Lines(std::sync::Arc<std::sync::Mutex<Vec<Line>>>);

impl Lines {
    fn push(&self, kind: &'static str, text: String) {
        let mut held = match self.0.lock() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        };
        held.push(Line::new(kind, text));
    }

    fn take(&self) -> Vec<Line> {
        let mut held = match self.0.lock() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        };
        std::mem::take(&mut *held)
    }
}

/// The band handles, once there are any.
///
/// The page loads and calls in before setup has finished, so something must be
/// managed from the start. An empty slot answers "not ready" rather than
/// failing with a message about unmanaged state, which says nothing to a
/// person.
///
/// The band task always starts, with or without a paired band: with none, it
/// waits for a pair. So the slot is empty only for the moment before setup
/// fills it.
#[derive(Default)]
struct BandSlot {
    band: std::sync::Mutex<Option<Band>>,
}

impl BandSlot {
    fn with<T>(&self, use_it: impl FnOnce(&Band) -> T) -> Result<T, String> {
        let held = self
            .band
            .lock()
            .map_err(|_| "the band task failed and left its lock broken".to_string())?;
        match held.as_ref() {
            Some(band) => Ok(use_it(band)),
            None => Err("the band is not running yet".to_string()),
        }
    }
}

/// The handles the window needs to reach the band task.
struct Band {
    commands: mpsc::Sender<SupCommand>,
    requests: mpsc::Sender<ipc::Incoming>,
}

impl Band {
    /// Ask the band task, over a channel taken from the slot, and collect
    /// what it answers.
    ///
    /// Taking the channel first, rather than holding the slot, keeps a lock out
    /// of an await.
    async fn ask_on(
        requests: &mpsc::Sender<ipc::Incoming>,
        request: ipc::Request,
    ) -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        Self::ask_as_it_goes(requests, request, |text| lines.push(text)).await?;
        Ok(lines)
    }

    /// Ask the band task, and hand on each line of the answer as it arrives,
    /// for a request that runs long enough for a person to watch it go.
    ///
    /// The window's channel to the band task is not the socket, so it carries
    /// the requests the socket refuses.
    async fn ask_as_it_goes(
        requests: &mpsc::Sender<ipc::Incoming>,
        request: ipc::Request,
        mut each: impl FnMut(String),
    ) -> Result<(), String> {
        let (tx, mut rx) = mpsc::channel(32);
        requests
            .send(ipc::Incoming { request, reply: tx })
            .await
            .map_err(|_| "the band task is gone".to_string())?;

        while let Some(reply) = rx.recv().await {
            match reply {
                ipc::Reply::Line { text } | ipc::Reply::Data { json: text } => each(text),
                ipc::Reply::Done => return Ok(()),
                ipc::Reply::Failed { reason } => return Err(reason),
            }
        }
        Ok(())
    }

    /// Ask the band task, and keep only the data in its answer. `status`
    /// answers with a line for a person and with data for a window.
    async fn data_on(
        requests: &mpsc::Sender<ipc::Incoming>,
        request: ipc::Request,
    ) -> Result<String, String> {
        let (tx, mut rx) = mpsc::channel(32);
        requests
            .send(ipc::Incoming { request, reply: tx })
            .await
            .map_err(|_| "the band task is gone".to_string())?;

        let mut data = None;
        while let Some(reply) = rx.recv().await {
            match reply {
                ipc::Reply::Data { json } => data = data.or(Some(json)),
                ipc::Reply::Line { .. } => {}
                ipc::Reply::Done => break,
                ipc::Reply::Failed { reason } => return Err(reason),
            }
        }
        data.ok_or_else(|| "the band task sent no data".to_string())
    }
}

/// Take the lines that have piled up since the last call.
///
/// The page pulls these rather than relying only on an event reaching it. A
/// push that never arrives leaves an empty list with nothing to explain it,
/// which is exactly what happened.
#[tauri::command]
fn drain_lines(lines: State<'_, Lines>) -> Vec<Line> {
    lines.take()
}

#[tauri::command]
async fn connect(slot: State<'_, BandSlot>) -> Result<(), String> {
    let commands = slot.with(|band| band.commands.clone())?;
    commands
        .send(SupCommand::Connect)
        .await
        .map_err(|_| "the band task is gone".to_string())
}

/// Let the band go, and stay disconnected. Cancel while connecting is this
/// too: the connect runs to its end, and lets the band go when it lands.
#[tauri::command]
async fn disconnect(slot: State<'_, BandSlot>) -> Result<(), String> {
    let commands = slot.with(|band| band.commands.clone())?;
    commands
        .send(SupCommand::Disconnect)
        .await
        .map_err(|_| "the band task is gone".to_string())
}

/// Stop the retries after a drop. The band stays away until a person
/// connects it. Anything but a retry is left as it is, so a band that came
/// back a moment before stays connected.
#[tauri::command]
async fn stop_trying(slot: State<'_, BandSlot>) -> Result<(), String> {
    let commands = slot.with(|band| band.commands.clone())?;
    commands
        .send(SupCommand::StopTrying)
        .await
        .map_err(|_| "the band task is gone".to_string())
}

/// Where the connection stands, as JSON: `state`, `reason` and `since`, the
/// same as on a `state` line. The page asks once when it opens, and follows
/// the lines after that. The pill and the connection card are worded from
/// it, not from the radio's word.
#[tauri::command]
async fn connection(slot: State<'_, BandSlot>) -> Result<String, String> {
    let requests = slot.with(|band| band.requests.clone())?;
    Band::data_on(&requests, ipc::Request::Status).await
}

/// The lists the window arranges: the app menu and the shortcuts. The
/// control center is not in the window (D5). The band task keeps it, and
/// the command line reaches it.
fn window_list(list: &str) -> Result<(), String> {
    match list {
        "menu" | "shortcuts" => Ok(()),
        other => Err(format!(
            "the window arranges the app menu and the shortcuts, not {other}"
        )),
    }
}

/// One screen list, as data the page can lay out: each entry's `id`, its
/// `name`, whether it `is_app`, whether it was `installed_here`, whether it
/// is `shown`, and its `position`. `list` is `menu` or `shortcuts`.
#[tauri::command]
async fn screens_list(slot: State<'_, BandSlot>, list: String) -> Result<String, String> {
    window_list(&list)?;
    let requests = slot.with(|band| band.requests.clone())?;
    let lines = Band::ask_on(&requests, ipc::Request::ScreenList { list }).await?;
    lines
        .into_iter()
        .next()
        .ok_or_else(|| "the band sent no list".to_string())
}

/// Replace what one list shows with `shown`, in this order. Every other
/// entry is kept out of view, as the band had it. The band task reads the
/// list first and writes it back whole, and answers what it shows then.
#[tauri::command]
async fn screens_apply(
    slot: State<'_, BandSlot>,
    list: String,
    shown: Vec<String>,
) -> Result<Vec<String>, String> {
    window_list(&list)?;
    let requests = slot.with(|band| band.requests.clone())?;
    Band::ask_on(
        &requests,
        ipc::Request::Screens {
            list,
            // A list of nothing is written too: the band then shows none.
            set: Some(shown),
            hide: vec![],
            tidy: false,
        },
    )
    .await
}

/// The apps installed on the band, as data the page can lay out.
///
/// Only the apps this Mac can name, which is the apps installed from here. The
/// band holds other packages, and the page says how many rather than listing
/// what it cannot act on.
#[tauri::command]
async fn apps_list(slot: State<'_, BandSlot>) -> Result<String, String> {
    let requests = slot.with(|band| band.requests.clone())?;
    let lines = Band::ask_on(&requests, ipc::Request::AppList { all: false }).await?;
    lines
        .into_iter()
        .next()
        .ok_or_else(|| "the band sent no app list".to_string())
}

/// What this host knows about the band, as JSON: `name` (from `[device]`),
/// `mac`, `serial`, `hardware`, `firmware`, `battery`, and `connectedSince`,
/// local time in RFC 3339 while the link is up and null otherwise.
///
/// The address is read once per link, and the band reports its battery on
/// connect and whenever it changes, so this answers from what arrived rather
/// than holding the page while the band is asked.
#[tauri::command]
async fn device(slot: State<'_, BandSlot>) -> Result<String, String> {
    let requests = slot.with(|band| band.requests.clone())?;
    let lines = Band::ask_on(&requests, ipc::Request::Device).await?;
    lines
        .into_iter()
        .next()
        .ok_or_else(|| "the band sent nothing about itself".to_string())
}

/// Remove one app from the band, named by the id the list shows.
///
/// The band takes an id and gives back no name, so the removal is refused
/// unless the band itself lists that id. The page asks the person first.
#[tauri::command]
async fn apps_remove(slot: State<'_, BandSlot>, id: String) -> Result<Vec<String>, String> {
    let requests = slot.with(|band| band.requests.clone())?;
    Band::ask_on(&requests, ipc::Request::Apps { remove: Some(id), all: false }).await
}

/// Grow or shrink the window by this many points.
///
/// The page measures what it needs and asks, rather than the height being
/// guessed here: a guess leaves a scrollbar on a window that just opened.
#[tauri::command]
fn adjust_height(window: tauri::WebviewWindow, delta: f64) -> Result<(), String> {
    // A tiny difference is rounding, not a real overflow.
    if delta.abs() < 2.0 {
        return Ok(());
    }
    let size = page_size(&window).map_err(|err| err.to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let height = (size.height as f64 / scale + delta).max(200.0);
    let width = size.width as f64 / scale;
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|err| err.to_string())
}

/// The size of the area the page fills, in the terms `set_size` takes. On
/// the Mac the page fills the whole window, under the title bar.
#[cfg(target_os = "macos")]
fn page_size(window: &tauri::WebviewWindow) -> tauri::Result<tauri::PhysicalSize<u32>> {
    window.outer_size()
}

/// The size of the area the page fills, in the terms `set_size` takes. A
/// Windows window with no frame still has a border a few pixels wide, which
/// its outer size counts: given that size back, it would grow by the border
/// at every fit.
#[cfg(windows)]
fn page_size(window: &tauri::WebviewWindow) -> tauri::Result<tauri::PhysicalSize<u32>> {
    window.inner_size()
}

/// The place and size a zoomed window goes back to, on Windows.
#[cfg(windows)]
#[derive(Default)]
struct Unzoomed(std::sync::Mutex<Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>)>>);

/// Zoom the window, as the Mac's zoom does for a window as wide as it may be:
/// the full height of the screen it is on, at the width it has. A second zoom
/// gives the window its place and size back. The caption's middle button and
/// a double click on the bar zoom (W8). Windows' own maximize would fill the
/// screen's width as well, which the page is not drawn for, so the window is
/// not maximizable on Windows. Answers whether the window is zoomed now.
#[cfg(windows)]
#[tauri::command]
fn zoom(window: tauri::WebviewWindow, unzoomed: State<'_, Unzoomed>) -> Result<bool, String> {
    let work = work_area(&window)?;
    let mut before = unzoomed
        .0
        .lock()
        .map_err(|_| "the zoom failed and left its lock broken".to_string())?;
    let failed = |err: tauri::Error| err.to_string();
    let seen = visible_frame(&window)?;
    if fills(seen.top, seen.height, work.position.y, work.size.height) {
        if let Some((position, size)) = before.take() {
            window.set_position(position).map_err(failed)?;
            window.set_size(size).map_err(failed)?;
        }
        return Ok(false);
    }
    let position = window.outer_position().map_err(failed)?;
    let page = window.inner_size().map_err(failed)?;
    *before = Some((position, page));
    // The frame Windows draws is a pixel or two more than the page. The
    // window's outer size is more again: an edge for resizing that nobody
    // sees. The frame the eye sees goes from the top of the work area to its
    // bottom.
    let border = seen.height.saturating_sub(page.height);
    let above = seen.top - position.y;
    window
        .set_position(tauri::PhysicalPosition::new(
            position.x,
            work.position.y - above,
        ))
        .map_err(failed)?;
    window
        .set_size(tauri::PhysicalSize::new(
            page.width,
            work.size.height.saturating_sub(border),
        ))
        .map_err(failed)?;
    Ok(true)
}

/// Whether the window is zoomed: its frame fills the height of the screen
/// it is on. The page asks as the window's size changes, to draw the middle
/// button.
#[cfg(windows)]
#[tauri::command]
fn zoomed(window: tauri::WebviewWindow) -> Result<bool, String> {
    let work = work_area(&window)?;
    let seen = visible_frame(&window)?;
    Ok(fills(seen.top, seen.height, work.position.y, work.size.height))
}

/// The top and the height of the frame Windows draws for the window, in
/// pixels, as its compositor reports them.
#[cfg(windows)]
struct Frame {
    top: i32,
    height: u32,
}

/// The frame Windows draws for the window. Its outer size counts an edge for
/// resizing beyond it, which nobody sees.
#[cfg(windows)]
fn visible_frame(window: &tauri::WebviewWindow) -> Result<Frame, String> {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
    let handle = window.hwnd().map_err(|err| err.to_string())?;
    let mut rect = RECT::default();
    // SAFETY: the window's own handle, and a RECT the size the call is told.
    unsafe {
        DwmGetWindowAttribute(
            HWND(handle.0),
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut rect).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
    }
    .map_err(|err| format!("Windows did not say where the window is: {err}"))?;
    Ok(Frame {
        top: rect.top,
        height: u32::try_from(rect.bottom - rect.top).unwrap_or(0),
    })
}

/// The part of the window's screen that windows may take: all of it but the
/// taskbar.
#[cfg(windows)]
fn work_area(window: &tauri::WebviewWindow) -> Result<tauri::PhysicalRect<i32, u32>, String> {
    let monitor = window
        .current_monitor()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "the window is on no screen".to_string())?;
    Ok(*monitor.work_area())
}

/// Whether a frame from `top`, `height` high, fills a work area from
/// `work_top`, `work_height` high, give or take 2 pixels.
#[cfg(windows)]
fn fills(top: i32, height: u32, work_top: i32, work_height: u32) -> bool {
    (top - work_top).abs() <= 2 && height + 2 >= work_height
}

/// Whether this program may post key events.
///
/// The window is launched by the system, so it is its own responsible process
/// and needs its own grant. A command run from a terminal inherits the
/// terminal's, which is why one can fire keystrokes while the other cannot.
#[tauri::command]
fn input_permission() -> bool {
    bandbridge::keys::trusted(false)
}

/// Ask macOS for the input permission, showing its dialog.
///
/// Adding the app to the settings list by hand records an entry the system does
/// not always honour. The dialog records the one it does.
#[tauri::command]
fn request_input_permission() -> bool {
    bandbridge::keys::trusted(true)
}

/// The actions, as the window's list shows them and its editor reads them:
/// `{"actions": [{"kind": "shell", "action": {…}}], "postsKeys": false}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionList {
    /// Each action, in the file's order.
    actions: Vec<ListedAction>,
    /// Whether any action posts a keystroke. Only a keystroke needs the input
    /// permission, so only a keystroke is a reason to ask for it.
    posts_keys: bool,
}

/// One action in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ListedAction {
    /// The key its kind has in the file: `keystroke`, `shell`, `script`,
    /// `shortcut` or `web_request`.
    kind: &'static str,
    /// The action in the file's own shape,
    /// `{"name": "Open Music", "signal": "bb03", "shell": "open -a Music"}`,
    /// which is the shape the editor sends back to save or to try.
    action: config::Action,
}

impl ActionList {
    fn of(actions: Vec<config::Action>) -> Self {
        Self {
            posts_keys: bandbridge::binding::any_posts_keys(&actions),
            actions: actions
                .into_iter()
                .map(|action| ListedAction {
                    kind: action.does.kind(),
                    action,
                })
                .collect(),
        }
    }
}

/// The actions in the file, for the list and the editor. See [`ActionList`].
///
/// This and every command below that reads, changes, tries or learns an
/// action is the window's alone. The socket has no such request (D9).
#[tauri::command]
fn actions_list() -> Result<ActionList, String> {
    config::Config::load()
        .map(|config| ActionList::of(config.actions))
        .map_err(|err| err.to_string())
}

/// Save an action from the editor, and answer the list the file then holds.
///
/// `action` is in the file's shape. With no `was`, it is a new action, and
/// goes after the others. Otherwise it takes the place of the action the list
/// showed as `was`: its place, name and signal, since two actions may share
/// a signal (D6).
///
/// Every rule it breaks comes back at once, each named by its field, as
/// `{"errors": [{"field": "signal", "message": "…"}]}`, and nothing is
/// written. The band task is told at once, so the change counts from the
/// next press.
#[tauri::command]
fn actions_save(
    slot: State<'_, BandSlot>,
    was: Option<config::Listed>,
    action: serde_json::Value,
) -> Result<ActionList, config::Refused> {
    let actions = config::Config::save_action(was.as_ref(), &action)?;
    nudge(&slot);
    Ok(ActionList::of(actions))
}

/// Delete the action the list showed as `was`, its place, name and signal,
/// and answer the list the file then holds. The band task is told at once.
#[tauri::command]
fn actions_delete(slot: State<'_, BandSlot>, was: config::Listed) -> Result<ActionList, String> {
    let actions = config::Config::delete_action(&was).map_err(|err| err.to_string())?;
    nudge(&slot);
    Ok(ActionList::of(actions))
}

/// Run an action from the editor once, now, before it is saved: Try it now.
/// The answer is how it went, in the shape of an event's result, such as
/// `{"type": "exit", "ok": true, "code": 0}`.
///
/// Only what the action does is read, so it can be tried before it has a
/// name or a signal. A rule it breaks comes back as for a save. A keystroke
/// goes at once to the app in front, so the page counts down first (D8).
#[tauri::command]
async fn action_try(action: serde_json::Value) -> Result<Outcome, config::Refused> {
    try_from_editor(&action).await
}

/// Run what an action from the editor does, once. See [`action_try`].
async fn try_from_editor(written: &serde_json::Value) -> Result<Outcome, config::Refused> {
    let does = config::Does::from_editor(written)?;
    Ok(bandbridge::action::run_once(&does).await)
}

/// Learn from the device: wait for the next press, and answer its signal,
/// such as `"bb04"`. The band task fires nothing for that press.
///
/// Null when the learn was stopped, by `learn_stop` or by another learn,
/// which takes its place: there is nothing to show. An error when the band is
/// not connected, goes away, or sends no press within a minute.
#[tauri::command]
async fn learn(slot: State<'_, BandSlot>) -> Result<Option<String>, String> {
    let commands = slot.with(|band| band.commands.clone())?;
    bandbridge::supervisor::learn(&commands, bandbridge::supervisor::LEARN_LIMIT).await
}

/// Stop the learn that waits, if any: when the editor closes, or its Learn is
/// pressed a second time. The next press fires as usual. Without this, a
/// learn left waiting would take a press a person meant to fire.
#[tauri::command]
async fn learn_stop(slot: State<'_, BandSlot>) -> Result<(), String> {
    let commands = slot.with(|band| band.commands.clone())?;
    commands
        .send(SupCommand::StopLearning)
        .await
        .map_err(|_| "the band task is gone".to_string())
}

/// The shortcuts in Shortcuts, by name, for the editor's picker, in the
/// order Shortcuts gives them: `["Start Focus", …]`. Refresh asks again. A
/// failure says why, rather than answer an empty list.
#[tauri::command]
async fn shortcuts_list() -> Result<Vec<String>, String> {
    bandbridge::action::shortcut_names().await
}

/// The names a keystroke can use, for the editor's key list (D3):
/// `{"keys": […], "mediaKeys": […], "modifiers": […], "aliases": {"keys":
/// {…}, "mediaKeys": {…}, "modifiers": {…}}}`. See
/// [`bandbridge::keys::KeyNames`].
#[tauri::command]
fn key_names() -> bandbridge::keys::KeyNames {
    bandbridge::keys::names()
}

/// Record keys: the next key a person presses, heard by an event tap that
/// exists only while this waits. The answer is the keystroke in the file's
/// shape, `{"heard": {"key_code": "4", "modifiers": ["command", "shift"]}}`,
/// or, when the tap cannot hear the keys, where to listen instead,
/// `{"page": "permission"}` (or `secure_input`, `no_tap`): the page then
/// listens itself. Null when the recording was stopped, or the window does
/// not have the focus. See [`recorder::record`].
///
/// `ticket` is the page's number for this recording, which its stop names.
#[tauri::command]
async fn record_keys(
    window: tauri::WebviewWindow,
    recordings: State<'_, recorder::Recordings>,
    ticket: recorder::Ticket,
) -> Result<Option<recorder::Recorded>, String> {
    // A window whose focus cannot be read records nothing.
    recorder::record(&recordings, ticket, || window.is_focused().unwrap_or(false)).await
}

/// Stop the recording `ticket` names: the editor closed, Record keys was
/// pressed again, or the page lost the focus. Its tap goes at once, and a
/// recording that has not begun yet stops as it begins. With no ticket, stop
/// whatever records: a page that just loaded runs no recording.
#[tauri::command]
fn record_keys_stop(recordings: State<'_, recorder::Recordings>, ticket: Option<recorder::Ticket>) {
    match ticket {
        Some(ticket) => recordings.stop(ticket),
        None => recordings.stop_all(),
    }
}

/// Whether a window event ends every recording: the window lost the focus,
/// so a key typed now is for another app, or it closed.
fn ends_recordings(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::Focused(false) | WindowEvent::CloseRequested { .. }
    )
}

/// Ask for the script file of a shell action, for the Shell tab's Choose…,
/// and answer its path with the home folder as `~`, as the file keeps it and
/// a person reads it: `~/scripts/toggle-desk-lamp.sh`. Null when the person
/// canceled.
///
/// A script runs as `sh <file>` on the Mac, so it needs no execute bit. On
/// Windows it runs as a double-click runs it, and a `.ps1` by PowerShell.
/// Either way, any file may be picked.
#[tauri::command]
async fn pick_script(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose a script")
        .pick_file(move |picked| {
            let _ = tx.send(picked);
        });
    let Some(picked) = rx
        .await
        .map_err(|_| "the file dialog closed without an answer".to_string())?
    else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|err| format!("that file cannot be used as a script: {err}"))?;
    Ok(Some(shown_path(&path, home_folder().as_deref())))
}

/// Tell the band task the file changed, so it takes the change up now, not
/// at its next look. Not waited on: a band task that is busy takes it up at
/// that look anyway.
fn nudge(slot: &BandSlot) {
    if let Ok(commands) = slot.with(|band| band.commands.clone()) {
        let _ = commands.try_send(SupCommand::ConfigChanged);
    }
}

/// The settings as the file holds them, by the names the file gives them:
/// `auto_reconnect`, `new_app_on_top`, and `appearance`, which is `light`,
/// `dark` or `system`.
///
/// The window's alone. The socket has no such request (D9).
#[tauri::command]
fn settings_get() -> Result<config::Settings, String> {
    config::Config::load()
        .map(|config| config.settings)
        .map_err(|err| err.to_string())
}

/// Change the settings that `change` names, in the file, and give back the
/// settings the file then holds. `[device]` and the actions are written back
/// as they were read, and a file that does not load is not written.
///
/// Each change takes effect with no restart. The window takes its
/// appearance here. The band task is told to read the file again, so auto
/// reconnect and new app on top count from now, and it would find the change
/// at its next look anyway.
///
/// The window's alone. The socket has no such request (D9).
#[tauri::command]
fn settings_set(
    window: tauri::WebviewWindow,
    slot: State<'_, BandSlot>,
    change: config::SettingsChange,
) -> Result<config::Settings, String> {
    let settings = config::Config::change_settings(&change).map_err(|err| err.to_string())?;
    show_appearance(&window, settings.appearance);
    nudge(&slot);
    Ok(settings)
}

/// The window's own theme for an appearance: light, dark, or none, which
/// follows the system.
fn theme_for(appearance: config::Appearance) -> Option<tauri::Theme> {
    match appearance {
        config::Appearance::Light => Some(tauri::Theme::Light),
        config::Appearance::Dark => Some(tauri::Theme::Dark),
        config::Appearance::System => None,
    }
}

/// Give the window the appearance its page shows, so the window buttons that
/// macOS draws over the title bar match the bar under them.
///
/// On macOS this is the appearance of the whole app, so the menu bar item's
/// menu and the file picker follow it too. The page sets its own theme from
/// the same setting.
fn show_appearance(window: &tauri::WebviewWindow, appearance: config::Appearance) {
    if let Err(err) = window.set_theme(theme_for(appearance)) {
        tracing::warn!("the window's appearance was not set: {err}");
    }
}

/// Where the config file is, the way the status bar shows it.
#[tauri::command]
fn config_path() -> Result<String, String> {
    let Some(path) = config::Config::path() else {
        return Err("no home directory".to_string());
    };
    Ok(shown_config_path(&path))
}

/// The config file's path with the home folder written as `~`:
/// `~/Library/Application Support/BandBridge/config.toml`.
#[cfg(target_os = "macos")]
fn shown_config_path(path: &std::path::Path) -> String {
    shown_path(path, home_folder().as_deref())
}

/// The config file's path as Windows writes the folder it is in:
/// `%APPDATA%\BandBridge\config.toml`, which File Explorer's address bar
/// takes as it is.
#[cfg(windows)]
fn shown_config_path(path: &std::path::Path) -> String {
    let appdata = std::env::var_os("APPDATA").map(std::path::PathBuf::from);
    match appdata.as_deref().and_then(|folder| path.strip_prefix(folder).ok()) {
        Some(rest) => format!("%APPDATA%\\{}", rest.display()),
        None => shown_path(path, home_folder().as_deref()),
    }
}

/// The home folder, which a path shows as `~`. On Windows it is the user's
/// profile folder, which a script's `~` names there too.
fn home_folder() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    let home = std::env::var_os("HOME");
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE");
    home.map(std::path::PathBuf::from)
}

/// A path with the home folder written as `~`, which is how a person reads
/// it, and shorter in a status bar.
fn shown_path(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// Show the config file in the Finder, or in File Explorer on Windows,
/// selected: the status bar's button, and the one in the note that says the
/// file could not be read.
///
/// Before the first save there is no file to select, so the folder it will
/// be in is shown.
#[tauri::command]
fn reveal_config() -> Result<(), String> {
    let Some(path) = config::Config::path() else {
        return Err("no home directory".to_string());
    };
    let target = match path.parent() {
        Some(folder) if !path.exists() => folder.to_path_buf(),
        _ => path,
    };
    if !target.exists() {
        return Err(format!("{} does not exist yet", target.display()));
    }
    show_in_file_manager(&target)
}

/// Show a file in the Finder, selected.
#[cfg(target_os = "macos")]
fn show_in_file_manager(target: &std::path::Path) -> Result<(), String> {
    let status = std::process::Command::new("open")
        .arg("-R")
        .arg(target)
        .status()
        .map_err(|err| format!("cannot open the Finder: {err}"))?;
    if !status.success() {
        return Err(format!("the Finder could not show {}", target.display()));
    }
    Ok(())
}

/// Show a file in File Explorer, selected.
///
/// File Explorer reads `/select,` and the quoted path as one argument, so
/// the argument goes as it is written here. It answers 1 whether or not it
/// showed the file, so only a start that failed has anything to say.
#[cfg(windows)]
fn show_in_file_manager(target: &std::path::Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("explorer.exe")
        .raw_arg(select_argument(target))
        .spawn()
        .map(drop)
        .map_err(|err| format!("cannot open File Explorer: {err}"))
}

/// The argument that has File Explorer show a file selected.
#[cfg(windows)]
fn select_argument(target: &std::path::Path) -> String {
    format!("/select,\"{}\"", target.display())
}

/// Put the band's auth key on the clipboard, from the system's secret store.
///
/// Copy key on the connection card calls this, and so does the forget sheet,
/// before a forget takes the key out of the store.
///
/// Rust writes the clipboard itself, so the key never enters the page. The
/// page learns only whether it worked. The key is marked for clipboard history
/// apps to leave out, and the apps that follow the convention do.
#[tauri::command]
async fn copy_key() -> Result<(), String> {
    // The store may ask the person first, in a dialog of the system's own,
    // so wait for it away from the thread that draws the window.
    tauri::async_runtime::spawn_blocking(copy_key_now)
        .await
        .map_err(|_| "the copy stopped before it finished".to_string())?
}

/// Put the paired band's key on the clipboard, from the store. It blocks while
/// the store answers.
fn copy_key_now() -> Result<(), String> {
    let config = config::Config::load().map_err(|err| err.to_string())?;
    let key = key_to_copy(&config, config::AuthKey::read)?;
    to_clipboard(&key)
}

/// Pair the band that takes this key, and call it by this name.
///
/// The key goes to the band task inside the request, and nowhere else. With
/// actions and settings a forget kept, `setup` keeps them, which is the
/// default, or deletes them.
///
/// A key that no band takes keeps the search going for all of its 25 seconds,
/// so what the pair says as it goes reaches the page at once, as `pair` lines,
/// rather than all together at the end. None of them holds the key.
#[tauri::command]
async fn pair(
    slot: State<'_, BandSlot>,
    lines: State<'_, Lines>,
    key: String,
    name: String,
    setup: Option<config::Setup>,
) -> Result<(), String> {
    let requests = slot.with(|band| band.requests.clone())?;
    let request = ipc::Request::Pair {
        key: ipc::KeyText::new(key),
        name,
        setup: setup.unwrap_or_default(),
    };
    let lines = lines.inner().clone();
    Band::ask_as_it_goes(&requests, request, |text| lines.push("pair", text)).await
}

/// The page that says how to get a band's auth key from its account: by
/// logging in with huami-token, among other ways.
const KEY_HELP: &str = "https://gadgetbridge.org/basics/pairing/huami-xiaomi-server/";

/// Open the page about the auth key in the browser.
///
/// The address is fixed here, so the page can open this one address and no
/// other.
#[tauri::command]
fn open_key_help() -> Result<(), String> {
    open_in_browser(KEY_HELP)
}

/// The project's public repository, which holds the licenses and the notices.
const REPOSITORY: &str = "https://github.com/ctrl-alt-delete-8/mi-band-bridge";

/// The address of each page the About section opens, by its name. The page
/// asks by name, so it can open these pages and no others.
fn about_page(name: &str) -> Option<String> {
    let file = match name {
        "app-license" => "LICENSE-APP.md",
        "source-license" => "LICENSE",
        "notices" => "THIRD-PARTY-NOTICES",
        _ => return None,
    };
    Some(format!("{REPOSITORY}/blob/main/{file}"))
}

/// Open one of the About section's pages in the browser.
#[tauri::command]
fn open_about_page(name: String) -> Result<(), String> {
    let address =
        about_page(&name).ok_or_else(|| format!("the About section has no page called {name}"))?;
    open_in_browser(&address)
}

/// The version the About section shows: this build's own.
#[tauri::command]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Open a web page in the browser.
#[cfg(target_os = "macos")]
fn open_in_browser(address: &str) -> Result<(), String> {
    let status = std::process::Command::new("open")
        .arg(address)
        .status()
        .map_err(|err| format!("cannot open the browser: {err}"))?;
    if !status.success() {
        return Err(format!("the browser could not open {address}"));
    }
    Ok(())
}

/// Open a web page in the browser, as the shell's open verb does.
#[cfg(windows)]
fn open_in_browser(address: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};
    // SAFETY: strings that live for the call, and no window to own a
    // dialog.
    let opened = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(address),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // The shell answers a number above 32 when it opened the page.
    if opened.0 as usize <= 32 {
        return Err(format!("the browser could not open {address}"));
    }
    Ok(())
}

/// Forget the paired band: its key leaves the store, and its actions and
/// settings are kept or deleted.
#[tauri::command]
async fn forget(slot: State<'_, BandSlot>, setup: config::Setup) -> Result<Vec<String>, String> {
    let requests = slot.with(|band| band.requests.clone())?;
    Band::ask_on(&requests, ipc::Request::Forget { setup }).await
}

/// The paired band, if any, and what a forget kept, as JSON: `device` (a name
/// and a MAC, or null), `actions` (a count) and `settingsChanged`.
#[tauri::command]
async fn saved_setup(slot: State<'_, BandSlot>) -> Result<String, String> {
    let requests = slot.with(|band| band.requests.clone())?;
    let lines = Band::ask_on(&requests, ipc::Request::SavedSetup).await?;
    lines
        .into_iter()
        .next()
        .ok_or_else(|| "the band task sent no setup".to_string())
}

/// The key the store holds for the paired band, or why there is none.
fn key_to_copy(
    config: &config::Config,
    read: impl FnOnce(&str) -> Result<Option<config::AuthKey>, BandError>,
) -> Result<config::AuthKey, String> {
    let Some(mac) = config.mac() else {
        return Err("no band is paired, so there is no key to copy".to_string());
    };
    read(mac)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("{} holds no key for {mac}", config::STORE))
}

/// Write the key to the clipboard, marked for clipboard history to leave out.
fn to_clipboard(key: &config::AuthKey) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|err| format!("cannot reach the clipboard: {err}"))?;
    let set = clipboard.set();
    #[cfg(target_os = "macos")]
    let set = arboard::SetExtApple::exclude_from_history(set);
    #[cfg(windows)]
    let set = arboard::SetExtWindows::exclude_from_history(set);
    set.text(key.to_hex())
        .map_err(|err| format!("cannot write the clipboard: {err}"))
}

/// Ask for an app package, for Install an app (.bin): its path, or null
/// when the person canceled. The page then installs it, and says which.
///
/// The window has a working directory of its own, so it never guesses where a
/// package is. The picker returns a path that means the same thing anywhere.
#[tauri::command]
async fn pick_app(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Zepp OS app", &["bin"])
        .set_title("Install an app")
        .pick_file(move |picked| {
            let _ = tx.send(picked);
        });
    let Some(picked) = rx
        .await
        .map_err(|_| "the file dialog closed without an answer".to_string())?
    else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|err| format!("that file cannot be installed: {err}"))?;
    Ok(Some(path.display().to_string()))
}

/// Install an app, and answer what the band task said as it went: the
/// package's size, a line each time the upload's percentage moves, then
/// "Installed.".
///
/// This is the request the socket takes from the band app editor: the band
/// task lets the band go, sends the app, and connects again. With New app
/// on top on, that connect puts the new app first in the menu (D10).
#[tauri::command]
async fn install(slot: State<'_, BandSlot>, file: String) -> Result<Vec<String>, String> {
    let requests = slot.with(|band| band.requests.clone())?;
    Band::ask_on(&requests, ipc::Request::Install { file }).await
}

/// The line the page gets for what the supervisor reports, or None for what
/// the page never shows.
///
/// A state change carries where the connection stands, for the page to word.
/// A press, and an app installed or removed, is a record for the event log.
/// A failure is a connection problem, which the page shows on the connection
/// card when the card's own line does not say it.
///
/// What the band sends, as it came, is for a terminal: a press reaches the
/// page as its record, and the rest is the band's own traffic, its answers to
/// our requests and its battery reports, which the event log never shows.
/// What the band task says as it goes, such as the menu put in order after an
/// install or a setting taken up, is for developers: it goes to this app's
/// own output, and `follow` prints it as well.
fn line_for(status: Status) -> Option<Line> {
    Some(match status {
        Status::State(state, standing) => Line {
            kind: "state",
            text: Some(state.to_string()),
            connection: Some(standing),
            event: None,
        },
        Status::Event(_) | Status::Notice(_) => return None,
        Status::Pressed(press) | Status::Finished(press) => Line::event(Record::Press(press)),
        Status::AppChanged(change) => Line::event(Record::App(change)),
        Status::Failed(reason) => Line::new("error", reason),
    })
}

/// The config the window starts with: the file, or, when it does not load,
/// the defaults, which fire nothing and name no band. The page then hears
/// why, as a `config` line, in the buffer it collects from. The band task
/// follows the file from here, so a fix counts from its next look, and a
/// Connect after the fix takes up the band the file names.
///
/// The page has no permission to listen for events, so a line sent that way
/// reaches nothing, and this reason never showed.
fn config_at_start(loaded: Result<config::Config, BandError>, lines: &Lines) -> config::Config {
    loaded.unwrap_or_else(|err| {
        lines.push(
            "config",
            format!(
                "the config file could not be read, so no action fires until it is fixed: {err}"
            ),
        );
        config::Config::default()
    })
}

/// Say so plainly, as a `permission` line, when an action posts keystrokes
/// and macOS does not let this program post them: otherwise a press looks
/// like it did nothing. Only a keystroke needs the permission.
fn permission_at_start(actions: &[config::Action], allowed: bool, lines: &Lines) {
    if bandbridge::binding::any_posts_keys(actions) && !allowed {
        lines.push(
            "permission",
            "This program cannot post key events, so presses will not become keystrokes. Open \
             System Settings, Privacy & Security, Device Control and Data Access, and switch \
             Mi Band Bridge on."
                .to_string(),
        );
    }
}

/// The key for the band in `[device]`, as the window starts, from what
/// [`config::AuthKey::load`] gave.
///
/// With no band paired there is none, and the band task waits for a pair,
/// which the setup screen asks for. A paired band whose key cannot be had is
/// an `error` line, for the connection card, and the band task still starts
/// with no key. This is not the last word: a Connect reads the file and asks
/// the store again, so a store that answers then, or a file fixed since, is
/// taken up with no restart (`Supervisor::look_again`).
///
/// A key the store does not hold is said in the words a Connect gets, since
/// looking again finds none either: forget the band, and pair it again. Where
/// the search looked names `BAND_AUTH_KEY`, a developer's override, so it
/// goes to this app's own output alone. Any other reason, such as a store
/// that could not be read, is said as it is: the key may well be there, a
/// Connect asks again, and a forget would delete it.
fn key_at_start(
    loaded: Result<config::AuthKey, BandError>,
    paired: bool,
    lines: &Lines,
) -> Option<config::AuthKey> {
    match loaded {
        Ok(key) => Some(key),
        Err(_) if !paired => None,
        Err(BandError::MissingAuthKey(looked)) => {
            tracing::warn!("no auth key: {looked}");
            lines.push("error", NO_KEY_TO_CONNECT.to_string());
            None
        }
        Err(err) => {
            lines.push("error", format!("cannot connect: {err}"));
            None
        }
    }
}

/// Run the band task on a thread of its own.
///
/// The supervisor owns a live session whose notification stream is not `Sync`,
/// so it cannot be spawned onto a shared runtime. It gets a current-thread
/// runtime instead, which needs no such bound.
///
/// It starts with or without a paired band. With none, it waits for a pair,
/// and answers the socket meanwhile, so a pair can reach it from either side.
///
/// `config` is the file as the window read it at start. The band task
/// follows the file from there.
fn start_band(app: AppHandle, lines: Lines, config: config::Config) -> Band {
    let auth_key = key_at_start(
        config::AuthKey::load(config.mac()),
        config.device.is_some(),
        &lines,
    );

    // The window fires actions the same way a session does, because the
    // supervisor holds them.
    permission_at_start(&config.actions, bandbridge::keys::trusted(false), &lines);

    let opts = ConnectOptions {
        peripheral_id: config.uuid().map(str::to_string),
        mac: config.mac().map(str::to_string),
        raw: false,
        mtu: None,
        subscribe_all: false,
        scan_seconds: 20,
    };

    let buffer = lines.clone();

    let (cmd_tx, cmd_rx) = mpsc::channel::<SupCommand>(8);
    let (status_tx, mut status_rx) = mpsc::channel::<Status>(64);
    let (req_tx, req_rx) = mpsc::channel::<ipc::Incoming>(8);

    let to_band = req_tx.clone();

    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(err) => {
                tracing::error!("no runtime for the band task: {err}");
                return;
            }
        };

        runtime.block_on(async move {
            // Answer the socket, so a command reaches the band through here.
            // The socket refuses what only this window may ask.
            tokio::spawn(async move {
                if let Err(err) = ipc::serve(to_band).await {
                    tracing::debug!("not serving other processes: {err}");
                }
            });

            // Carry what the supervisor reports into the window. What it
            // says as it goes, and why a connection failed, also go to this
            // app's own output, for developers: the window shows no notice,
            // and a failure only on the connection card.
            tokio::spawn(async move {
                while let Some(status) = status_rx.recv().await {
                    match &status {
                        Status::Notice(text) => tracing::info!("{text}"),
                        Status::Failed(reason) => tracing::warn!("{reason}"),
                        _ => {}
                    }
                    let Some(line) = line_for(status) else {
                        continue;
                    };
                    // Keep the push, and also hold the line for the page to
                    // pull. Either route alone has failed silently before.
                    let _ = app.emit("band", line.clone());
                    if let Ok(mut held) = buffer.0.lock() {
                        held.push(line);
                        // Never grow without bound while no window is reading.
                        let extra = held.len().saturating_sub(500);
                        held.drain(..extra);
                    }
                }
            });

            let supervisor = Supervisor::new(auth_key, opts, cmd_rx, status_tx)
                .serving(req_rx)
                .following(&config);
            if let Err(err) = supervisor.run().await {
                tracing::error!("the band task stopped: {err}");
            }
        });
    });

    Band {
        commands: cmd_tx,
        requests: req_tx,
    }
}

/// Keep this program running while no window is open.
///
/// Closing the window only hides it, because the band link and the socket
/// live in this process. But macOS counts a menu bar app with no window on
/// screen as idle, and quits it on its own soon after (automatic
/// termination), which drops the band. Seen on macOS 27: the app quit on a
/// Quit event from the system about two seconds after its window closed.
/// This turns that off for the whole run. Quit on the menu bar item still
/// quits.
#[cfg(target_os = "macos")]
fn keep_running_without_a_window() {
    use objc2_foundation::{NSProcessInfo, NSString};
    NSProcessInfo::processInfo()
        .disableAutomaticTermination(&NSString::from_str("BandBridge holds the band"));
}

/// Bring the window to the front, for a person who asked for it: Open
/// BandBridge on the menu bar item, or opening the app again while it runs.
///
/// Showing the window is not enough. macOS does not bring a menu bar app
/// forward because it showed a window, so the window stayed behind the app
/// in front. Here the app comes forward too, and the window takes the focus.
///
/// The window's first appearance, as the program starts, still leaves the
/// focus where it is: a band press posts its keystroke to the app in front,
/// and that is not this window unless the person brings it forward.
fn open_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    #[cfg(target_os = "macos")]
    desktop_switch::opened();
    let opened = (|| -> tauri::Result<()> {
        // After command H, the app is hidden as a whole.
        #[cfg(target_os = "macos")]
        app.show()?;
        window.unminimize()?;
        window.show()?;
        window.set_focus()
    })();
    if let Err(err) = opened {
        tracing::warn!("the window could not be brought to the front: {err}");
    }
    // The focus above asks for the app to come forward in the way macOS 14
    // retired, so ask in the current way as well.
    #[cfg(target_os = "macos")]
    if let Some(main_thread) = objc2::MainThreadMarker::new() {
        objc2_app_kit::NSApplication::sharedApplication(main_thread).activate();
    }
}

/// Tell the page it runs on Windows, before its first script runs, so it
/// draws the Windows top bar from the first frame and words the few things
/// that differ. A page that hears nothing from here is the Mac's.
#[cfg(windows)]
fn tell_the_page_it_runs_on_windows<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("system")
        .js_init_script("window.__BANDBRIDGE_SYSTEM__ = \"windows\";".to_string())
        .build()
}

/// The tray icon on Windows: the app's own icon, and a left click opens the
/// window, as a tray icon does on Windows. The menu is the Mac's, on a right
/// click (W9).
#[cfg(windows)]
fn windows_tray(tray: TrayIconBuilder<tauri::Wry>) -> tauri::Result<TrayIconBuilder<tauri::Wry>> {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-windows.png"))?;
    Ok(tray
        .icon(icon)
        .tooltip("Mi Band Bridge")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                open_window(tray.app_handle());
            }
        }))
}

/// What Windows' uninstaller passes to a copy of BandBridge when a person
/// ticks "Delete the application data" (`windows/hooks.nsh`).
#[cfg(windows)]
const DELETE_APP_DATA: &str = "--delete-app-data";

fn main() {
    // The uninstaller runs the copy after BandBridge has quit and its files
    // are gone. It deletes the band's key and BandBridge's folder, and ends
    // before anything else starts, so no window, hook or pipe comes up.
    #[cfg(windows)]
    if std::env::args_os().nth(1).is_some_and(|arg| arg == DELETE_APP_DATA) {
        std::process::exit(match bandbridge::config::delete_app_data() {
            Ok(()) => 0,
            Err(_) => 1,
        });
    }

    tracing_subscriber::fmt().init();

    // Record keys uses a low-level keyboard hook, installed once here, before
    // the WebView2 window starts, as the R-006 review measured it hearing
    // keys (see recorder::windows). Between recordings it passes every key on.
    #[cfg(windows)]
    recorder::install();

    let builder = tauri::Builder::default();
    // One BandBridge at a time (W10). The plugin goes first, as it asks: a
    // second launch brings this window forward, and quits before it has a
    // window, a link to the band or a pipe of its own. macOS opens one copy
    // of an app by itself.
    #[cfg(windows)]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            open_window(app)
        }))
        .plugin(tell_the_page_it_runs_on_windows());

    builder
        .setup(|app| {
            // A menu bar item with no dock icon: this is a small utility, and
            // an app switcher entry for it would be noise.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            #[cfg(target_os = "macos")]
            keep_running_without_a_window();
            #[cfg(target_os = "macos")]
            desktop_switch::watch();

            let show = MenuItem::with_id(app, "show", "Open Mi Band Bridge", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            // A menu bar mark, not the app icon: an outline with no fill.
            // Marked as a template, so macOS colors it for the bar it sits in
            // and it stays legible in a light bar as well as a dark one.
            #[cfg(target_os = "macos")]
            let tray = {
                let mark =
                    tauri::image::Image::from_bytes(include_bytes!("../icons/tray@2x.png"))?;
                TrayIconBuilder::new()
                    .icon(mark)
                    .icon_as_template(true)
                    .show_menu_on_left_click(true)
            };
            #[cfg(windows)]
            let tray = windows_tray(TrayIconBuilder::new())?;
            tray.menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => open_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // Managed first, and filled once the band task is up. The page
            // can call in before this finishes.
            app.manage(BandSlot::default());
            app.manage(recorder::Recordings::default());
            #[cfg(windows)]
            app.manage(Unzoomed::default());

            // Closing puts the window away rather than ending the program.
            // The band link and the socket live in this process, so quitting on
            // a close would drop them and leave a command with nothing to talk
            // to. Quit is on the menu bar item. macOS would still quit the
            // program on its own once no window is open, unless told not to:
            // see keep_running_without_a_window.
            //
            // A window that loses the focus, or closes, stops Record keys
            // itself, whatever the page does: a key typed with another app in
            // front is that app's, and the tap would swallow it.
            if let Some(window) = app.get_webview_window("main") {
                let hidden = window.clone();
                window.on_window_event(move |event| {
                    if ends_recordings(event) {
                        hidden.state::<recorder::Recordings>().stop_all();
                    }
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = hidden.hide();
                    }
                });
            }
            let lines = Lines::default();
            app.manage(lines.clone());

            // A config that will not load must not stop the program. It used
            // to, and the window then sat there with no band and nothing to
            // explain why. An action that breaks a rule lands here too.
            let config = config_at_start(config::Config::load(), &lines);
            // The saved appearance, before the window first draws, so its
            // buttons and the page agree from the start.
            if let Some(window) = app.get_webview_window("main") {
                show_appearance(&window, config.settings.appearance);
            }

            let band = start_band(app.handle().clone(), lines, config);
            if let Ok(mut slot) = app.state::<BandSlot>().band.lock() {
                *slot = Some(band);
            }
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            connect,
            disconnect,
            stop_trying,
            drain_lines,
            connection,
            install,
            pick_app,
            screens_list,
            apps_list,
            apps_remove,
            device,
            screens_apply,
            settings_get,
            settings_set,
            actions_list,
            actions_save,
            actions_delete,
            action_try,
            learn,
            learn_stop,
            shortcuts_list,
            key_names,
            record_keys,
            record_keys_stop,
            pick_script,
            adjust_height,
            input_permission,
            request_input_permission,
            config_path,
            reveal_config,
            copy_key,
            pair,
            forget,
            saved_setup,
            open_key_help,
            open_about_page,
            version,
            #[cfg(windows)]
            zoom,
            #[cfg(windows)]
            zoomed
        ])
        .build(tauri::generate_context!())
        .expect("the window could not start")
        .run(|app, event| {
            // Opening BandBridge again while it runs, from Applications,
            // Spotlight or Raycast, brings the window forward, as Open
            // BandBridge does. Without this, a closed window stayed closed.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                open_window(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests {
    /// The About section opens the project's two licenses and its notices,
    /// in the public repository, and nothing else.
    #[test]
    fn the_about_links_open_the_licenses_and_the_notices() {
        let repo = "https://github.com/ctrl-alt-delete-8/mi-band-bridge/blob/main";
        assert_eq!(
            super::about_page("app-license"),
            Some(format!("{repo}/LICENSE-APP.md"))
        );
        assert_eq!(
            super::about_page("source-license"),
            Some(format!("{repo}/LICENSE"))
        );
        assert_eq!(
            super::about_page("notices"),
            Some(format!("{repo}/THIRD-PARTY-NOTICES"))
        );
        assert_eq!(super::about_page("https://example.com"), None);
    }

    /// The Mac release signs the app with the hardened runtime, which needs
    /// one entitlement only: Apple Events, since an action's script may
    /// control another app, and the app is responsible for what its scripts
    /// send. Bluetooth, the event tap and posting keys need none: each asks
    /// for its permission, with the plist's words where it has them.
    #[test]
    fn the_mac_release_asks_for_apple_events_and_nothing_else() {
        // A Windows checkout may hold the file with CRLF line endings.
        let entitlements = include_str!("../../packaging/bridge.entitlements").replace("\r\n", "\n");
        assert_eq!(entitlements.matches("<key>").count(), 1, "{entitlements}");
        assert!(entitlements.contains(
            "<key>com.apple.security.automation.apple-events</key>\n    <true/>"
        ));
        let plist = include_str!("../../packaging/Info.plist");
        assert!(plist.contains("<key>NSAppleEventsUsageDescription</key>"));
        assert!(plist.contains("<key>NSBluetoothAlwaysUsageDescription</key>"));
    }

    /// People see the name Mi Band Bridge and the version 1.0.0: in the
    /// window, in the Mac bundle and in the Windows installer. The names
    /// that hold the band's key and settings stay BandBridge's.
    #[test]
    fn the_app_is_mi_band_bridge_1_0_0() {
        assert_eq!(super::version(), "1.0.0");
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["productName"], "Mi Band Bridge");
        assert_eq!(conf["version"], "1.0.0");
        assert_eq!(conf["app"]["windows"][0]["title"], "Mi Band Bridge");
        assert_eq!(conf["identifier"], "com.ellengu.bandbridge.gui");
        let windows: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.windows.conf.json")).unwrap();
        assert_eq!(windows["app"]["windows"][0]["title"], "Mi Band Bridge");
        let plist = include_str!("../../packaging/Info.plist");
        for line in [
            "<key>CFBundleName</key><string>Mi Band Bridge</string>",
            "<key>CFBundleDisplayName</key><string>Mi Band Bridge</string>",
            "<key>CFBundleShortVersionString</key><string>1.0.0</string>",
            "<key>CFBundleExecutable</key><string>bandbridge-gui</string>",
        ] {
            assert!(plist.contains(line), "{line}");
        }
        let page = include_str!("../ui/index.html");
        assert!(page.contains("<title>Mi Band Bridge</title>"));
        assert!(page.contains("<h1>Mi Band Bridge</h1>"));
    }

    use super::*;

    const MAC: &str = "0A:1B:2C:3D:4E:5F";

    /// The uninstaller asks with the flag BandBridge answers, after a real
    /// uninstall with the box ticked, from a copy it made first.
    #[cfg(windows)]
    #[test]
    fn the_uninstaller_passes_the_flag_bandbridge_answers() {
        let hooks = include_str!("../windows/hooks.nsh");
        let call = format!("\"$PLUGINSDIR\\${{MAINBINARYNAME}}.exe\" {DELETE_APP_DATA}");
        assert!(hooks.contains(&call), "hooks.nsh does not run {call}");
        assert!(hooks.contains("$DeleteAppDataCheckboxState = 1"), "the box does not decide");
    }

    fn paired() -> config::Config {
        config::Config::parse(&format!("[device]\nname = \"Band\"\nmac = \"{MAC}\"\n")).unwrap()
    }

    #[test]
    fn copy_key_needs_a_paired_band() {
        let err = key_to_copy(&config::Config::default(), |_| panic!("the store was asked"))
            .unwrap_err();
        assert!(err.contains("no band is paired"), "{err}");
    }

    #[test]
    fn copy_key_reads_the_store_under_the_bands_mac() {
        let err = key_to_copy(&paired(), |mac| {
            assert_eq!(mac, MAC);
            Ok(None)
        })
        .unwrap_err();
        assert!(err.contains(&format!("{} holds no key for {MAC}", config::STORE)), "{err}");

        let key = key_to_copy(&paired(), |_| {
            Ok(Some(config::AuthKey::from_hex("000102030405060708090a0b0c0d0e0f").unwrap()))
        })
        .unwrap();
        assert_eq!(key.bytes()[15], 0x0f);
    }

    /// The socket refuses a pair, and the window's own channel carries it to
    /// the band task. Each line of the answer reaches the page while the pair
    /// still runs, not all together at its end.
    #[tokio::test]
    async fn the_window_asks_for_a_pair_itself_and_hears_each_line_at_once() {
        let request = ipc::Request::Pair {
            key: ipc::KeyText::new("000102030405060708090a0b0c0d0e0f"),
            name: "Band".to_string(),
            setup: config::Setup::Keep,
        };
        assert_eq!(request.window_only(), Some("pair"));

        let (requests, mut band) = mpsc::channel::<ipc::Incoming>(8);
        let (go_on, first_line_shown) = tokio::sync::oneshot::channel::<()>();
        let band_task = tokio::spawn(async move {
            let incoming = band.recv().await.expect("the pair reached the band task");
            assert!(matches!(incoming.request, ipc::Request::Pair { .. }));
            let answer = ipc::Answer(incoming.reply);
            answer.line("looking for bands nearby, for up to 25 seconds").await;
            first_line_shown.await.expect("the window saw the first line");
            answer.line("Band 5301 refused the key").await;
            answer.failed("no band nearby accepts this key: 1 band refused it").await;
        });

        let (shown, mut page) = mpsc::unbounded_channel::<String>();
        let asking = tokio::spawn(async move {
            Band::ask_as_it_goes(&requests, request, |text| shown.send(text).unwrap()).await
        });

        assert_eq!(page.recv().await.unwrap(), "looking for bands nearby, for up to 25 seconds");
        go_on.send(()).unwrap();
        assert_eq!(page.recv().await.unwrap(), "Band 5301 refused the key");
        assert_eq!(
            asking.await.unwrap(),
            Err("no band nearby accepts this key: 1 band refused it".to_string())
        );
        band_task.await.unwrap();
    }

    /// A short answer is still collected whole, as the other commands read it.
    #[tokio::test]
    async fn a_short_answer_is_collected_whole() {
        let (requests, mut band) = mpsc::channel::<ipc::Incoming>(8);
        tokio::spawn(async move {
            let incoming = band.recv().await.unwrap();
            assert_eq!(incoming.request, ipc::Request::SavedSetup);
            let json = r#"{"device":null,"actions":4,"settingsChanged":true}"#.to_string();
            incoming.reply.send(ipc::Reply::Data { json }).await.unwrap();
            incoming.reply.send(ipc::Reply::Done).await.unwrap();
        });
        let lines = Band::ask_on(&requests, ipc::Request::SavedSetup).await.unwrap();
        assert_eq!(lines, [r#"{"device":null,"actions":4,"settingsChanged":true}"#]);
    }

    /// A state line carries where the connection stands, for the page to
    /// word. Every other line is as it was.
    #[test]
    fn a_state_line_carries_where_the_connection_stands() {
        use bandbridge::supervisor::{ConnState, Phase, Reason};
        use std::time::{Duration, Instant, SystemTime};

        let since = SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_298_133);
        let standing = Standing {
            state: Phase::Retrying,
            reason: Reason::Dropped,
            since,
        };
        let backoff = ConnState::Backoff {
            until: Instant::now() + Duration::from_secs(4),
            attempt: 2,
        };
        let line = serde_json::to_value(line_for(Status::State(backoff, standing))).unwrap();
        assert_eq!(line["kind"], "state");
        assert_eq!(line["text"], "backoff (attempt 2)", "the words the page reads today");
        assert_eq!(
            line["connection"],
            serde_json::json!({
                "state": "retrying",
                "reason": "dropped",
                "since": bandbridge::supervisor::local_time(since),
            })
        );

        let line = serde_json::to_value(line_for(Status::Failed("band not found".into()))).unwrap();
        assert_eq!(
            line,
            serde_json::json!({ "kind": "error", "text": "band not found" })
        );
    }

    // --- the event log ----------------------------------------------------------

    use bandbridge::action::Outcome;
    use bandbridge::event::{AppChange, BandEvent, Change, Press};

    /// The moment every record below is from.
    fn moment() -> std::time::SystemTime {
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_298_251)
    }

    fn press(action: Option<&str>, result: Option<Outcome>) -> Press {
        Press {
            id: 7,
            at: moment(),
            signal: "bb04".to_string(),
            action: action.map(str::to_string),
            does: action.map(|_| "POST http://lights.local/webhook".to_string()),
            result,
        }
    }

    /// The line the page gets for this, as JSON.
    fn page_line(status: Status) -> serde_json::Value {
        serde_json::to_value(line_for(status).expect("the page gets this")).unwrap()
    }

    /// Every record reaches the page as an `event` line, with no words of its
    /// own: the page draws the row from the record. These are the shapes the
    /// event log is built on, one for each kind and each result.
    #[test]
    fn each_record_reaches_the_page_as_an_event_line() {
        let at = bandbridge::supervisor::local_time(moment());
        let desk_lights = Some("Turn on the desk lights");
        let exit = |code, error: Option<&str>| Outcome::Exit {
            code,
            error: error.map(str::to_string),
        };
        let cases = [
            (
                Status::Pressed(press(desk_lights, None)),
                serde_json::json!({"action": "Turn on the desk lights", "result": null}),
            ),
            (
                Status::Finished(press(desk_lights, Some(Outcome::Status(204)))),
                serde_json::json!({"action": "Turn on the desk lights",
                                   "result": {"type": "status", "ok": true, "code": 204}}),
            ),
            (
                Status::Finished(press(desk_lights, Some(Outcome::Status(503)))),
                serde_json::json!({"action": "Turn on the desk lights",
                                   "result": {"type": "status", "ok": false, "code": 503}}),
            ),
            (
                Status::Pressed(press(Some("Page up"), Some(Outcome::Done))),
                serde_json::json!({"action": "Page up", "result": {"type": "done", "ok": true}}),
            ),
            (
                Status::Finished(press(Some("Open Music"), Some(exit(0, None)))),
                serde_json::json!({"action": "Open Music",
                                   "result": {"type": "exit", "ok": true, "code": 0}}),
            ),
            (
                Status::Finished(press(
                    Some("Toggle the lamp"),
                    Some(exit(4, Some("the lamp is away"))),
                )),
                serde_json::json!({"action": "Toggle the lamp",
                                   "result": {"type": "exit", "ok": false, "code": 4,
                                              "error": "the lamp is away"}}),
            ),
            (
                Status::Finished(press(
                    Some("Start a focus block"),
                    Some(Outcome::Error("gave up after 10 seconds".to_string())),
                )),
                serde_json::json!({"action": "Start a focus block",
                                   "result": {"type": "error", "ok": false,
                                              "error": "gave up after 10 seconds"}}),
            ),
            (
                Status::Pressed(press(None, None)),
                serde_json::json!({"action": null, "result": null}),
            ),
        ];
        for (status, fields) in cases {
            let mut record =
                serde_json::json!({"kind": "press", "id": 7, "at": at, "signal": "bb04"});
            record
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            assert_eq!(
                page_line(status),
                serde_json::json!({"kind": "event", "event": record})
            );
        }

        for (change, word) in [
            (Change::Installed, "installed"),
            (Change::Removed, "removed"),
        ] {
            let status = Status::AppChanged(AppChange {
                id: 8,
                at: moment(),
                change,
                app: "testbandapp".to_string(),
            });
            assert_eq!(
                page_line(status),
                serde_json::json!({"kind": "event", "event": {
                    "kind": "app", "id": 8, "at": at, "change": word, "app": "testbandapp"}})
            );
        }
    }

    /// What the band sends, as it came, never reaches the page: a press comes
    /// as its record, and the rest is the band's own traffic, its answers to
    /// our requests and its battery reports.
    #[test]
    fn what_the_band_sends_as_it_came_never_reaches_the_page() {
        let sent = |endpoint, payload: &[u8]| {
            BandEvent::from_message(bandbridge::protocol::chunk::ZeppMessage {
                endpoint,
                payload: payload.to_vec(),
                encrypted: false,
            })
        };
        let mut list_reply = vec![0x02, 0x00, 0x01];
        list_reply.resize(24, 0);
        for event in [
            sent(0x00a0, &[0xbb, 0x01]),
            sent(0x00a0, &list_reply),
            sent(0x0029, &[0x04, 0x0f, 74, 0]),
            sent(0x0001, &[0x01, 0x02]),
            sent(0x001b, &[0x05, 0x05]),
            sent(0x0010, &[0xaa, 0xbb]),
        ] {
            let shown = event.to_string();
            assert!(
                line_for(Status::Event(event)).is_none(),
                "{shown} reached the page"
            );
        }
    }

    /// Only a record is an `event` line. A connection problem and a change
    /// to the connection, which the card shows, are other kinds, with no
    /// record.
    #[test]
    fn nothing_else_is_an_event_line() {
        use bandbridge::supervisor::{ConnState, Phase, Reason};
        let standing = Standing {
            state: Phase::Connected,
            reason: Reason::You,
            since: moment(),
        };
        for (status, kind) in [
            (
                Status::Failed("link lost: the band stopped answering".to_string()),
                "error",
            ),
            (Status::State(ConnState::Ready, standing), "state"),
        ] {
            let line = page_line(status);
            assert_eq!(line["kind"], kind, "{line}");
            assert!(line.get("event").is_none(), "{line}");
            assert!(line["text"].is_string(), "{line}");
        }
    }

    /// What the band task says as it goes never reaches the page: the menu
    /// put in order after an install, a setting taken up, the actions
    /// loaded, a band found, a disconnect by request. It is for developers,
    /// in this app's own output.
    #[test]
    fn what_the_band_task_says_as_it_goes_never_reaches_the_page() {
        for text in [
            "the menu is in order: 3 apps, then Settings",
            "auto reconnect is on: a link that drops is tried again",
            "4 action(s) loaded",
            "found Xiaomi Smart Band 7",
            "disconnected by request; auto-reconnect suppressed",
        ] {
            assert!(
                line_for(Status::Notice(text.to_string())).is_none(),
                "{text} reached the page"
            );
        }
    }

    /// `connection` keeps the data in the status answer, and leaves the line
    /// for a person out.
    #[tokio::test]
    async fn the_connection_is_the_data_in_the_status_answer() {
        let (requests, mut band) = mpsc::channel::<ipc::Incoming>(8);
        tokio::spawn(async move {
            let incoming = band.recv().await.unwrap();
            assert_eq!(incoming.request, ipc::Request::Status);
            let reply = incoming.reply;
            let text = "state disconnected".to_string();
            reply.send(ipc::Reply::Line { text }).await.unwrap();
            let json = r#"{"state":"disconnected","reason":"you","since":"2026-09-24T21:06:00-04:00"}"#;
            reply.send(ipc::Reply::Data { json: json.to_string() }).await.unwrap();
            reply.send(ipc::Reply::Done).await.unwrap();
        });
        let json = Band::data_on(&requests, ipc::Request::Status).await.unwrap();
        assert_eq!(
            json,
            r#"{"state":"disconnected","reason":"you","since":"2026-09-24T21:06:00-04:00"}"#
        );
    }

    // --- the actions, for the list and the editor --------------------------------

    /// Two actions, as the file holds them.
    const TWO_ACTIONS: &str = r#"
[[actions]]
name = "Screenshot a region"
signal = "bb01"
keystroke = { key_code = "4", modifiers = ["left_command", "left_shift"] }

[[actions]]
name = "Open Music"
signal = "bb03"
shell = "open -a Music"
"#;

    /// The list gives each action in the file's own shape, which the editor
    /// sends back, with its kind, and whether any action posts a keystroke.
    #[test]
    fn the_list_gives_each_action_in_the_file_shape_with_its_kind() {
        let actions = config::Config::parse(TWO_ACTIONS).unwrap().actions;
        assert_eq!(
            serde_json::to_value(ActionList::of(actions)).unwrap(),
            serde_json::json!({
                "actions": [
                    {"kind": "keystroke", "action": {
                        "name": "Screenshot a region",
                        "signal": "bb01",
                        "keystroke": {"key_code": "4", "modifiers": ["left_command", "left_shift"]},
                    }},
                    {"kind": "shell", "action": {
                        "name": "Open Music",
                        "signal": "bb03",
                        "shell": "open -a Music",
                    }},
                ],
                "postsKeys": true,
            })
        );
        assert_eq!(
            serde_json::to_value(ActionList::of(Vec::new())).unwrap(),
            serde_json::json!({"actions": [], "postsKeys": false})
        );
    }

    /// Every command about actions is the window's alone (D9). The socket
    /// has no request of that name, so it refuses each before the band task
    /// hears it.
    #[test]
    fn the_socket_has_no_request_for_what_the_editor_asks() {
        for name in [
            "actions_list",
            "actions_save",
            "actions_delete",
            "action_try",
            "learn",
            "learn_stop",
            "shortcuts_list",
            "key_names",
            "record_keys",
            "record_keys_stop",
            "pick_script",
        ] {
            let asked = serde_json::json!({"request": name});
            assert!(
                serde_json::from_value::<ipc::Request>(asked).is_err(),
                "{name}"
            );
        }
        // The same shape for a request the socket has is read.
        let status = serde_json::json!({"request": "status"});
        assert_eq!(
            serde_json::from_value::<ipc::Request>(status).unwrap(),
            ipc::Request::Status
        );
    }

    /// The window arranges the app menu and the shortcuts, and never the
    /// control center (D5), which the command line keeps.
    #[test]
    fn the_window_arranges_the_menu_and_the_shortcuts_alone() {
        assert_eq!(window_list("menu"), Ok(()));
        assert_eq!(window_list("shortcuts"), Ok(()));
        for list in ["control", "control-center", "", "MENU"] {
            let refused = window_list(list).unwrap_err();
            assert!(
                refused.contains("the app menu and the shortcuts"),
                "{refused}"
            );
        }
    }

    /// The window stops Record keys itself as it loses the focus, whatever
    /// the page does, and not as it gains the focus or moves.
    #[test]
    fn losing_the_focus_ends_every_recording() {
        assert!(ends_recordings(&WindowEvent::Focused(false)));
        assert!(!ends_recordings(&WindowEvent::Focused(true)));
        assert!(!ends_recordings(&WindowEvent::Moved(
            tauri::PhysicalPosition::new(0, 0)
        )));
    }

    /// A try answers how the action went, in the shape of an event's result,
    /// or each rule it breaks, by its field. It needs no name or signal. No
    /// keystroke here: it would type into the window in front.
    #[tokio::test]
    async fn a_try_answers_its_result_or_each_problem_by_its_field() {
        let tried = try_from_editor(&serde_json::json!({"shell": "exit 3"}))
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(tried).unwrap(),
            serde_json::json!({"type": "exit", "ok": false, "code": 3})
        );

        let refused = try_from_editor(&serde_json::json!({
            "name": "Lamp",
            "web_request": {"url": "lamp.local/on", "method": "POST"},
        }))
        .await
        .unwrap_err();
        assert_eq!(
            serde_json::to_value(refused).unwrap(),
            serde_json::json!({"errors": [{
                "field": "web_request.url",
                "message": "the url must start with http:// or https://",
            }]})
        );
    }

    /// The two things the window says as it starts reach the page through
    /// the buffer it collects from, each as a kind of its own: never an event,
    /// and never a connection problem, which the card shows.
    #[test]
    fn the_start_up_problems_reach_the_page_as_their_own_kinds() {
        let lines = Lines::default();
        let unread = BandError::Config("line 10: invalid type: string \"yes\"".to_string());
        let config = config_at_start(Err(unread), &lines);
        assert_eq!(
            config,
            config::Config::default(),
            "the defaults, which fire nothing"
        );
        let keystrokes = config::Config::parse(TWO_ACTIONS).unwrap().actions;
        permission_at_start(&keystrokes, false, &lines);
        assert_eq!(
            serde_json::to_value(lines.take()).unwrap(),
            serde_json::json!([
                {"kind": "config",
                 "text": "the config file could not be read, so no action fires until it is \
                          fixed: config error: line 10: invalid type: string \"yes\""},
                {"kind": "permission",
                 "text": "This program cannot post key events, so presses will not become \
                          keystrokes. Open System Settings, Privacy & Security, Device \
                          Control and Data Access, and switch Mi Band Bridge on."},
            ])
        );

        // A file that loads, a permission that is there, and actions that
        // post no keystroke, say nothing.
        let loaded = config::Config::parse(TWO_ACTIONS).unwrap();
        assert_eq!(config_at_start(Ok(loaded.clone()), &lines), loaded);
        permission_at_start(&keystrokes, true, &lines);
        permission_at_start(&keystrokes[1..], false, &lines);
        assert!(lines.take().is_empty());
    }

    /// A paired band whose key is not in the store says so in the words a
    /// Connect gets, with the way out. Where the search looked, which names
    /// BAND_AUTH_KEY, is a developer's, and the window never shows it. A store
    /// that could not be read says why, since the key may well be in it.
    #[test]
    fn a_paired_band_with_no_key_says_what_to_do() {
        let lines = Lines::default();
        let missing = || {
            BandError::MissingAuthKey(format!(
                "BAND_AUTH_KEY is not set, and {} holds no key for {MAC}",
                config::STORE
            ))
        };

        assert!(key_at_start(Err(missing()), true, &lines).is_none());
        let said = serde_json::to_value(lines.take()).unwrap();
        assert_eq!(
            said,
            serde_json::json!([{"kind": "error", "text": NO_KEY_TO_CONNECT}])
        );
        assert!(!said.to_string().contains("BAND_AUTH_KEY"), "{said}");
        for way_out in ["Forget the band", "the key from your account login"] {
            assert!(NO_KEY_TO_CONNECT.contains(way_out), "{NO_KEY_TO_CONNECT}");
        }

        let locked = BandError::KeyStore(format!(
            "{} cannot read the key for {MAC}: the Keychain is locked",
            config::STORE
        ));
        assert!(key_at_start(Err(locked), true, &lines).is_none());
        assert_eq!(
            serde_json::to_value(lines.take()).unwrap(),
            serde_json::json!([{
                "kind": "error",
                "text": format!(
                    "cannot connect: {} cannot read the key for 0A:1B:2C:3D:4E:5F: the Keychain \
                     is locked",
                    config::STORE
                )
            }])
        );

        // With no band paired, there is no key to have: the setup screen
        // shows, and nothing is said.
        assert!(key_at_start(Err(missing()), false, &lines).is_none());
        let key = config::AuthKey::from_hex("000102030405060708090a0b0c0d0e0f").unwrap();
        assert!(key_at_start(Ok(key), true, &lines).is_some());
        assert!(lines.take().is_empty());
    }

    /// Light and dark force the window's appearance, and Follow system gives
    /// it back to the system.
    #[test]
    fn each_appearance_is_a_window_theme_or_the_systems() {
        assert_eq!(
            theme_for(config::Appearance::Light),
            Some(tauri::Theme::Light)
        );
        assert_eq!(
            theme_for(config::Appearance::Dark),
            Some(tauri::Theme::Dark)
        );
        assert_eq!(theme_for(config::Appearance::System), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_status_bar_writes_the_home_folder_as_a_tilde() {
        use std::path::Path;
        let file = Path::new("/Users/ada/Library/Application Support/BandBridge/config.toml");

        assert_eq!(
            shown_path(file, Some(Path::new("/Users/ada"))),
            "~/Library/Application Support/BandBridge/config.toml"
        );
        // A folder whose name only starts with the home folder's is not in it.
        assert_eq!(
            shown_path(file, Some(Path::new("/Users/ad"))),
            file.display().to_string()
        );
        assert_eq!(shown_path(file, None), file.display().to_string());
    }

    /// On Windows a path under the profile folder is written with `~`, as a
    /// script's path is kept, and the config file under %APPDATA%, the way
    /// Windows names that folder.
    #[cfg(windows)]
    #[test]
    fn the_status_bar_writes_the_folders_as_windows_names_them() {
        use std::path::{Path, PathBuf};
        let script = Path::new(r"C:\Users\Ada Lovelace\scripts\toggle-desk-lamp.ps1");
        assert_eq!(
            shown_path(script, Some(Path::new(r"C:\Users\Ada Lovelace"))),
            r"~\scripts\toggle-desk-lamp.ps1"
        );
        // A folder whose name only starts with the home folder's is not in it.
        assert_eq!(
            shown_path(script, Some(Path::new(r"C:\Users\Ada"))),
            script.display().to_string()
        );

        let appdata = PathBuf::from(std::env::var_os("APPDATA").expect("Windows sets APPDATA"));
        let config = appdata.join("BandBridge").join("config.toml");
        assert_eq!(shown_config_path(&config), r"%APPDATA%\BandBridge\config.toml");
        assert_eq!(
            shown_config_path(&PathBuf::from(r"D:\elsewhere\config.toml")),
            r"D:\elsewhere\config.toml"
        );
    }

    /// A zoomed window reaches from the work area's top to its bottom, give
    /// or take 2 pixels of rounding. One lower down, or less high, is not.
    #[cfg(windows)]
    #[test]
    fn a_zoomed_window_fills_the_height_of_the_work_area() {
        assert!(fills(0, 2064, 0, 2064));
        assert!(fills(2, 2062, 0, 2064));
        assert!(fills(-1, 2065, 0, 2064));
        assert!(!fills(147, 960, 0, 2064), "the window as it opens");
        assert!(!fills(0, 960, 0, 2064), "at the top, not as high");
        assert!(!fills(10, 2064, 0, 2064), "as high, lower down");
    }

    /// File Explorer takes `/select,` and the quoted path as one argument, a
    /// path with spaces included.
    #[cfg(windows)]
    #[test]
    fn file_explorer_is_asked_to_select_the_file() {
        let file = std::path::Path::new(r"C:\Users\Ada Lovelace\AppData\Roaming\BandBridge");
        assert_eq!(
            select_argument(file),
            r#"/select,"C:\Users\Ada Lovelace\AppData\Roaming\BandBridge""#
        );
    }
}
