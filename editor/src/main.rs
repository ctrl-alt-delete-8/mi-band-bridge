// Keep a console window from opening beside the editor on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use band_editor::layout::{self, Button, Layout};

/// The side of a picture thumbnail in the form, in window pixels.
const THUMBNAIL: u32 = 88;
use band_editor::output::{self, Said};
use band_editor::{checks, generate, images, install, signals, zmake};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
struct AppEntry {
    dir: String,
    name: String,
}

#[derive(Serialize)]
struct PreviewData {
    page: String,
    images: HashMap<String, Vec<u8>>,
}

fn text<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn layout_file(app_dir: &str) -> PathBuf {
    Path::new(app_dir).join("layout.json")
}

/// The folder that holds this app and its siblings.
fn folder_of(app_dir: &str) -> PathBuf {
    Path::new(app_dir).parent().map(Path::to_path_buf).unwrap_or_default()
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .set_title("Pick the folder that holds your band apps")
        .pick_folder(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx.recv().ok().flatten()?;
    picked.into_path().ok().map(|p| p.display().to_string())
}

/// The folder as the toolbar shows it, with the home folder written as `~`,
/// which is how a person reads it.
#[tauri::command]
fn shown_folder(folder: String) -> String {
    output::shown(Path::new(&folder))
}

#[tauri::command]
fn list_apps(folder: String) -> Vec<AppEntry> {
    let mut apps: Vec<AppEntry> = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("layout.json").is_file())
        .map(|path| {
            let name = layout::load(&path.join("layout.json"))
                .map(|l| l.name)
                .unwrap_or_else(|_| path.file_name().unwrap_or_default().to_string_lossy().into_owned());
            AppEntry { dir: path.display().to_string(), name }
        })
        .collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

#[tauri::command]
fn create_app(folder: String, name: String) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the app a name.".to_string());
    }
    let base = layout::slug(name);
    let mut dir = Path::new(&folder).join(&base);
    let mut n = 2;
    while dir.exists() {
        dir = Path::new(&folder).join(format!("{base}-{n}"));
        n += 1;
    }
    let used = signals::in_use(Path::new(&folder));
    let mut new = Layout::new(name);
    let signal = signals::next_free(&used).unwrap_or_default();
    new.buttons.push(Button::new(&signal, &new.color));
    std::fs::create_dir_all(dir.join("images")).map_err(text)?;
    layout::save(&dir.join("layout.json"), &new).map_err(text)?;
    Ok(dir.display().to_string())
}

#[tauri::command]
fn load_layout(app_dir: String) -> Result<Layout, String> {
    layout::load(&layout_file(&app_dir)).map_err(text)
}

#[tauri::command]
fn save_layout(app_dir: String, mut layout: Layout) -> Result<(), String> {
    // Store every signal in lower case, as the spec requires.
    for button in &mut layout.buttons {
        if let Some(signal) = layout::normalize_signal(&button.signal) {
            button.signal = signal;
        }
    }
    layout::save(&layout_file(&app_dir), &layout).map_err(text)
}

#[tauri::command]
async fn check_layout(app_dir: String, layout: Layout) -> Result<checks::Report, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let used = signals::in_use(&folder_of(&app_dir));
        checks::check(&layout, Path::new(&app_dir), &used)
    })
    .await
    .map_err(text)
}

#[tauri::command]
fn next_signal(app_dir: String, layout: Layout) -> Option<String> {
    let mut used = signals::in_use(&folder_of(&app_dir));
    // The buttons on screen may not be saved yet.
    for button in &layout.buttons {
        if let Some(signal) = layout::normalize_signal(&button.signal) {
            used.push(signals::Use { signal, app: layout.name.clone(), dir: None });
        }
    }
    signals::next_free(&used)
}

#[tauri::command]
async fn pick_image(app: AppHandle, app_dir: String) -> Result<Option<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .add_filter("Image", &band_editor::images::FORMATS)
        .set_title("Pick a PNG or JPEG picture")
        .pick_file(move |picked| {
            let _ = tx.send(picked);
        });
    let Some(picked) = rx.recv().ok().flatten() else {
        return Ok(None);
    };
    let source = picked.into_path().map_err(text)?;
    generate::copy_image(Path::new(&app_dir), &source)
        .map(Some)
        .map_err(|e| format!("Could not copy {}: {e}", source.display()))
}

#[tauri::command]
async fn preview(app_dir: String, layout: Layout) -> Result<PreviewData, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let made = generate::generate(Path::new(&app_dir), &layout).map_err(text)?;
        let mut images = HashMap::new();
        for (name, path) in &made.assets {
            images.insert(name.clone(), std::fs::read(path).map_err(text)?);
        }
        Ok(PreviewData { page: made.page, images })
    })
    .await
    .map_err(text)?
}

/// The user's picture, scaled for the form. PNG, whatever the file's own type.
#[tauri::command]
async fn image_thumbnail(app_dir: String, rel: String) -> Result<Vec<u8>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = layout::image_path(Path::new(&app_dir), &rel)
            .ok_or_else(|| "the picture must be inside the app folder".to_string())?;
        let source = images::load(&path)?;
        images::thumbnail(&source, THUMBNAIL).encode_png().map_err(text)
    })
    .await
    .map_err(text)?
}

#[tauri::command]
fn tools_status() -> zmake::ToolsStatus {
    zmake::status_in(&zmake::tools_dir())
}

/// Hand a line of a build or an install to the page as it happens, with its
/// time (decision E4). The Output drawer shows it.
fn tell(app: &AppHandle, said: Said) {
    let _ = app.emit("output", said);
}

/// Build the app, and tell the page each step as it happens.
#[tauri::command]
async fn build_app(
    app: AppHandle,
    app_dir: String,
    layout: Layout,
) -> Result<output::Built, String> {
    tauri::async_runtime::spawn_blocking(move || {
        output::build_app(
            Path::new(&app_dir),
            layout,
            &zmake::tools_dir(),
            &mut |said| tell(&app, said),
        )
    })
    .await
    .map_err(text)
}

/// Install the built app through BandBridge, and tell the page each step
/// and the upload's progress as they happen.
#[tauri::command]
async fn install_app(app: AppHandle, app_dir: String) -> Result<output::Installed, String> {
    tauri::async_runtime::spawn_blocking(move || {
        output::install_app(Path::new(&app_dir), &mut |said| tell(&app, said))
    })
    .await
    .map_err(text)
}

/// The version the About dialog shows: this build's own.
#[tauri::command]
fn version() -> &'static str {
    band_editor::about::VERSION
}

/// Open one of the About dialog's pages in the browser.
#[tauri::command]
fn open_about_page(name: String) -> Result<(), String> {
    let address = band_editor::about::page(&name)
        .ok_or_else(|| format!("the About dialog has no page called {name}"))?;
    open_in_browser(&address)
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

/// Quit the editor, from the build tools notice: it checks the tools when
/// it starts.
#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

/// Where BandBridge stands, for the title bar. The page asks every five
/// seconds while the window is shown. An ask can wait two seconds for its
/// answer, so it runs off the main thread.
#[tauri::command]
async fn bridge_status() -> Result<install::BridgeStatus, String> {
    tauri::async_runtime::spawn_blocking(install::status)
        .await
        .map_err(text)
}

/// Tell the page it runs on Windows, before its first script runs, so it
/// draws the Windows top bar from the first frame (W8). A page that hears
/// nothing from here is the Mac's. BandBridge's window does the same.
#[cfg(windows)]
fn tell_the_page_it_runs_on_windows<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("system")
        .js_init_script("window.__BANDBRIDGE_SYSTEM__ = \"windows\";".to_string())
        .build()
}

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(windows)]
    let builder = builder.plugin(tell_the_page_it_runs_on_windows());
    builder
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            pick_folder,
            shown_folder,
            list_apps,
            create_app,
            load_layout,
            save_layout,
            check_layout,
            next_signal,
            pick_image,
            image_thumbnail,
            preview,
            tools_status,
            build_app,
            install_app,
            bridge_status,
            version,
            open_about_page,
            quit
        ])
        .run(tauri::generate_context!())
        .expect("the editor failed to start");
}
