#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod browser;
mod launcher;
#[cfg(target_os = "windows")]
#[allow(dead_code)]
#[path = "../named_profiles.rs"]
mod named_profiles;
#[path = "../queue_status.rs"]
mod queue_status;
mod store;
#[cfg(any(debug_assertions, test))]
mod verification;

use launcher::{Action, Launcher, Snapshot};
use std::{
    fs::{self, OpenOptions},
    sync::{mpsc, Mutex},
    time::Duration,
};
use tauri::{
    webview::{NewWindowResponse, WebviewBuilder},
    AppHandle, LogicalPosition, Manager, WebviewUrl, Window, WindowBuilder,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Reply<T> = std::result::Result<T, String>;

const WIDTH: f64 = 1180.0;
const HEIGHT: f64 = 800.0;
#[cfg(target_os = "windows")]
const TOOLBAR: f64 = 120.0;
const MAX_SESSIONS: u32 = 50;
const READ_PAGE: &str = include_str!("../read_queue_page.js");

fn on_main<T: Send + 'static>(
    app: &AppHandle,
    work: impl FnOnce(&Window, &mut Launcher) -> Result<T> + Send + 'static,
) -> Reply<T> {
    let (send, receive) = mpsc::sync_channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            let window = handle
                .get_window("main")
                .ok_or("Queue Launcher is closing.")?;
            let state = handle
                .try_state::<Mutex<Launcher>>()
                .ok_or("Queue Launcher is busy.")?;
            let mut launcher = state.try_lock().map_err(|_| "Queue Launcher is busy.")?;
            work(&window, &mut launcher)
        })();
        let _ = send.send(result.map_err(|error| error.to_string()));
    })
    .map_err(|error| error.to_string())?;
    receive.recv().map_err(|error| error.to_string())?
}

#[tauri::command]
async fn port(app: AppHandle, action: Action) -> Reply<Snapshot> {
    on_main(&app, move |window, launcher| {
        launcher.update();
        launcher.act(window, action)?;
        Ok(launcher.snapshot())
    })
}

#[tauri::command]
async fn session_menu(app: AppHandle, number: u32, x: f64, y: f64) -> Reply<()> {
    use tauri::menu::ContextMenu;
    if !x.is_finite() || !y.is_finite() {
        return Err("The menu position is invalid.".into());
    }
    let (menu, position) = on_main(&app, move |window, launcher| {
        launcher.update();
        #[cfg(target_os = "macos")]
        let position = browser::menu_position(window, x.max(0.), y.max(0.))?;
        #[cfg(not(target_os = "macos"))]
        let position = LogicalPosition::new(x.max(0.), y.max(0.));
        Ok((launcher.session_menu(window, number)?, position))
    })?;
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Some(window) = handle.get_window("main") {
            let _ = menu.popup_at(window, position);
        }
    })
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn configure_webview_runtime() -> Result<()> {
    let executable = std::env::current_exe()?;
    let root = executable
        .parent()
        .ok_or("Queue Launcher executable directory is unavailable.")?;
    let bundled = root.join("WebView2Runtime");
    if bundled.join("msedgewebview2.exe").is_file() {
        // Set before either Tauri or Wry creates a WebView2 environment.
        std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", bundled);
    }
    Ok(())
}

fn run() -> Result<()> {
    #[cfg(target_os = "windows")]
    configure_webview_runtime()?;
    #[cfg(all(debug_assertions, target_os = "macos"))]
    let smoke = launcher::mac_smoke::phase()?;
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![port, session_menu])
        .on_menu_event(|app, event| {
            if !event.id().as_ref().starts_with("session/") {
                return;
            }
            let app = app.clone();
            let id = event.id().as_ref().to_owned();
            // Popup menus run a nested native event loop. Dispatch the action
            // after the callback, without holding the launcher lock in that loop.
            tauri::async_runtime::spawn(async move {
                let _ = on_main(&app, move |window, launcher| {
                    launcher.menu_action(window, &id)
                });
            });
        })
        .setup(move |app| {
            let root = std::env::var_os("QUEUE_LAUNCHER_DATA")
                .map(std::path::PathBuf::from)
                .map(Ok)
                .unwrap_or_else(|| app.path().app_local_data_dir())?;
            #[cfg(all(debug_assertions, target_os = "windows"))]
            let smoke = std::env::args().any(|arg| arg == "--verify-offline");
            #[cfg(all(debug_assertions, target_os = "windows"))]
            if smoke && (std::env::var_os("QUEUE_LAUNCHER_DATA").is_none() || root.try_exists()?) {
                return Err(
                    "Offline verification requires a new QUEUE_LAUNCHER_DATA directory.".into(),
                );
            }
            #[cfg(all(debug_assertions, target_os = "macos"))]
            if let Some(phase) = smoke {
                phase.prepare(&root)?;
            }
            fs::create_dir_all(&root)?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(root.join("instance.lock"))?;
            lock.try_lock()
                .map_err(|_| "Queue Launcher is already using this data directory.")?;
            app.manage(lock);

            let window = WindowBuilder::new(app, "main")
                .title("Queue Launcher")
                .inner_size(WIDTH, HEIGHT)
                .min_inner_size(840., 560.)
                .build()?;
            #[allow(unused_mut)]
            let mut chrome = WebviewBuilder::new("app", WebviewUrl::App("app.html".into()))
                .auto_resize()
                .on_navigation(|url| {
                    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
                        || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost"))
                })
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .devtools(false);
            #[cfg(target_os = "windows")]
            {
                chrome = chrome.data_directory(root.join("Launcher Browser Data"));
            }
            let size = window
                .inner_size()?
                .to_logical::<f64>(window.scale_factor()?);
            let _chrome = window.add_child(chrome, LogicalPosition::new(0., 0.), size)?;
            #[cfg(target_os = "macos")]
            browser::attach_launcher(&_chrome)?;

            let mut launcher = Launcher::new(root)?;
            launcher.restore(&window);
            app.manage(Mutex::new(launcher));

            #[cfg(all(debug_assertions, target_os = "windows"))]
            if smoke {
                launcher::smoke::run(app.handle().clone());
            }

            #[cfg(all(debug_assertions, target_os = "macos"))]
            if let Some(phase) = smoke {
                launcher::mac_smoke::run(app.handle().clone(), phase);
            }

            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(500));
                if let Err(error) = on_main(&handle, |_, launcher| {
                    launcher.update();
                    Ok(())
                }) {
                    if error != "Queue Launcher is busy." {
                        break;
                    }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Queue Launcher: {error}");
        std::process::exit(1);
    }
}
