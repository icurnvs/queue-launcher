use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{mpsc, Mutex},
    time::Instant,
};
use tauri::{
    webview::{NewWindowResponse, WebviewBuilder},
    AppHandle, LogicalPosition, LogicalSize, Manager, Webview, WebviewUrl, Window, WindowBuilder,
};
use uuid::Uuid;

mod fixture;
mod verify;

const WIDTH: f64 = 960.0;
const HEIGHT: f64 = 640.0;
const TOOLBAR: f64 = 120.0;
const MAX: usize = 50;
const SAMPLE_LINK: &str = "https://queue-launcher.example/sample";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Reply<T> = std::result::Result<T, String>;

#[derive(Clone, Default, Deserialize, Serialize)]
struct Saved {
    sessions: Vec<Uuid>,
    pending_removal: Vec<Uuid>,
}

struct Launcher {
    root: PathBuf,
    saved: Saved,
    links: Vec<String>,
    views: Vec<(Uuid, Webview)>,
    selected: Option<Uuid>,
    fixture_url: tauri::Url,
    verifying: bool,
    cleanup_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    count: usize,
    selected: Option<usize>,
    links: Vec<String>,
    pending_removal: usize,
    cleanup_error: Option<String>,
    open_ms: Option<u128>,
}

fn read_json<T: Default + serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error.into()),
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let staging = path.with_extension("tmp");
    fs::write(&staging, serde_json::to_vec_pretty(value)?)?;
    fs::rename(staging, path)?;
    Ok(())
}

impl Launcher {
    fn save(&self, saved: Saved) -> Result<()> {
        write_json(&self.root.join("sessions.json"), &saved)
    }

    fn status(&self, open_ms: Option<u128>) -> Status {
        Status {
            count: self.views.len(),
            selected: self
                .views
                .iter()
                .position(|(id, _)| Some(*id) == self.selected),
            links: self.links.clone(),
            pending_removal: self.saved.pending_removal.len(),
            cleanup_error: self.cleanup_error.clone(),
            open_ms,
        }
    }

    fn create_view(&self, window: &Window, id: Uuid) -> Result<Webview> {
        let url = self.fixture_url.clone();
        let mut builder =
            WebviewBuilder::new(format!("session-{id}"), WebviewUrl::External(url.clone()))
                .on_navigation(move |target| *target == url)
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .devtools(false);
        if self.verifying {
            builder = builder.initialization_script(format!("window.fixtureSession = '{id}';"));
        }
        #[cfg(target_os = "windows")]
        {
            builder = builder.data_directory(self.root.join("profiles").join(id.to_string()));
        }
        #[cfg(target_os = "macos")]
        {
            builder = builder.data_store_identifier(*id.as_bytes());
        }
        let view = window.add_child(
            builder,
            LogicalPosition::new(0.0, TOOLBAR),
            LogicalSize::new(WIDTH, HEIGHT - TOOLBAR),
        )?;
        view.hide()?;
        Ok(view)
    }

    fn select(&mut self, id: Uuid) -> Result<()> {
        if self.selected == Some(id) {
            return Ok(());
        }
        let (_, target) = self
            .views
            .iter()
            .find(|(other, _)| *other == id)
            .ok_or("unknown session")?;
        if let Some((_, previous)) = self
            .views
            .iter()
            .find(|(other, _)| Some(*other) == self.selected)
        {
            previous.hide()?;
        }
        target.show()?;
        self.selected = Some(id);
        Ok(())
    }

    fn add(&mut self, window: &Window, count: usize) -> Result<()> {
        if count > MAX - self.views.len() {
            return Err("limit is 50 open sessions".into());
        }
        for _ in 0..count {
            let id = Uuid::new_v4();
            let view = self.create_view(window, id)?;
            let mut saved = self.saved.clone();
            saved.sessions.push(id);
            if let Err(error) = self.save(saved.clone()) {
                view.close()?;
                return Err(error);
            }
            self.saved = saved;
            self.views.push((id, view));
        }
        if let Some((id, _)) = self.views.first() {
            if self.selected.is_none() {
                self.select(*id)?;
            }
        }
        Ok(())
    }

    fn close(&mut self, window: &Window, id: Uuid) -> Result<()> {
        let index = self
            .views
            .iter()
            .position(|(other, _)| *other == id)
            .ok_or("unknown session")?;
        let mut saved = self.saved.clone();
        saved.sessions.retain(|other| *other != id);
        saved.pending_removal.push(id);
        self.save(saved.clone())?;
        if let Err(error) = self.views[index].1.close() {
            self.save(self.saved.clone())?;
            return Err(error.into());
        }
        self.saved = saved;
        self.views.remove(index);
        if self.selected == Some(id) {
            self.selected = None;
            if let Some((next, _)) = self.views.get(index).or_else(|| self.views.last()) {
                self.select(*next)?;
            }
        }
        remove_store(window.app_handle().clone(), self.root.clone(), id);
        Ok(())
    }

    fn save_link(&mut self) -> Result<()> {
        if !self.links.iter().any(|link| link == SAMPLE_LINK) {
            let mut links = self.links.clone();
            links.push(SAMPLE_LINK.into());
            write_json(&self.root.join("links.json"), &links)?;
            self.links = links;
        }
        Ok(())
    }
}

fn on_main<T: Send + 'static>(
    app: &AppHandle,
    work: impl FnOnce(&Window, &mut Launcher) -> Result<T> + Send + 'static,
) -> Reply<T> {
    let (send, receive) = mpsc::sync_channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            let window = handle.get_window("main").ok_or("launcher window closed")?;
            let state = handle.state::<Mutex<Launcher>>();
            let mut launcher = state.try_lock().map_err(|_| "launcher is busy")?;
            work(&window, &mut launcher)
        })();
        let _ = send.send(result.map_err(|error| error.to_string()));
    })
    .map_err(|error| error.to_string())?;
    receive.recv().map_err(|error| error.to_string())?
}

fn record_cleanup(app: &AppHandle, id: Uuid, outcome: Reply<()>) {
    let _ = on_main(app, move |_, launcher| {
        match outcome {
            Ok(()) => {
                let mut saved = launcher.saved.clone();
                saved.pending_removal.retain(|other| *other != id);
                if let Err(error) = launcher.save(saved.clone()) {
                    launcher.cleanup_error = Some(error.to_string());
                } else {
                    launcher.saved = saved;
                    if launcher.saved.pending_removal.is_empty() {
                        launcher.cleanup_error = None;
                    }
                }
            }
            Err(error) => launcher.cleanup_error = Some(error),
        }
        Ok(())
    });
}

#[cfg(target_os = "windows")]
fn remove_store(app: AppHandle, root: PathBuf, id: Uuid) {
    std::thread::spawn(move || {
        let path = root.join("profiles").join(id.to_string());
        let mut outcome = Ok(());
        for _ in 0..60 {
            match fs::remove_dir_all(&path) {
                Ok(()) => {
                    outcome = Ok(());
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && !path.exists() => {
                    outcome = Ok(());
                    break;
                }
                Err(error) => outcome = Err(format!("Windows store cleanup: {error}")),
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        record_cleanup(&app, id, outcome);
    });
}

#[cfg(target_os = "macos")]
fn remove_store(app: AppHandle, _root: PathBuf, id: Uuid) {
    use wry::WebViewExtDarwin;

    // Run after the current Tauri close event has released the WKWebView.
    std::thread::spawn(move || {
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            let (send, receive) = mpsc::channel();
            let dispatched = app.run_on_main_thread(move || {
                wry::WebView::remove_data_store(id.as_bytes(), move |result| {
                    let _ = send.send(result.map_err(|error| error.to_string()));
                });
            });
            if dispatched.is_err() {
                return;
            }
            match receive.recv_timeout(std::time::Duration::from_secs(3)) {
                Ok(Ok(())) => {
                    record_cleanup(&app, id, Ok(()));
                    return;
                }
                Ok(Err(_)) => {}
                Err(_) => break,
            }
        }
        record_cleanup(
            &app,
            id,
            Err("WebKit store removal did not complete".into()),
        );
    });
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Action {
    Status,
    Add { count: usize },
    Select { index: usize },
    Close { index: usize },
    SaveLink,
}

#[tauri::command]
async fn launcher(app: AppHandle, action: Action) -> Reply<Status> {
    on_main(&app, move |window, launcher| {
        let mut open_ms = None;
        match action {
            Action::Status => {}
            Action::Add { count } => {
                let start = Instant::now();
                launcher.add(window, count)?;
                open_ms = Some(start.elapsed().as_millis());
            }
            Action::Select { index } => {
                let (id, _) = launcher.views.get(index).ok_or("unknown session")?;
                launcher.select(*id)?;
            }
            Action::Close { index } => {
                let (id, _) = launcher.views.get(index).ok_or("unknown session")?;
                launcher.close(window, *id)?;
            }
            Action::SaveLink => launcher.save_link()?,
        }
        Ok(launcher.status(open_ms))
    })
}

fn main() {
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    compile_error!("This prototype targets Windows and macOS.");

    let verification = std::env::args().nth(1);
    let start = Instant::now();
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![launcher])
        .setup(move |app| {
            let root = match std::env::var_os("QUEUE_TAURI_PROTOTYPE_DATA") {
                Some(root) => PathBuf::from(root),
                None => app.path().app_local_data_dir()?.join("prototype"),
            };
            fs::create_dir_all(&root)?;
            let saved: Saved = read_json(&root.join("sessions.json"))?;
            let links = read_json(&root.join("links.json"))?;
            let (fixture_url, reports) = fixture::start(verification.is_some())?;
            let window = WindowBuilder::new(app, "main")
                .title("Queue Launcher | Tauri prototype")
                .inner_size(WIDTH, HEIGHT)
                .resizable(false)
                .build()?;
            window.add_child(
                WebviewBuilder::new("launcher", WebviewUrl::App("index.html".into()))
                    .on_navigation(|url| {
                        matches!(url.scheme(), "tauri") || url.host_str() == Some("tauri.localhost")
                    })
                    .on_new_window(|_, _| NewWindowResponse::Deny)
                    .devtools(false),
                LogicalPosition::new(0., 0.),
                LogicalSize::new(WIDTH, TOOLBAR),
            )?;
            let mut launcher = Launcher {
                root,
                saved,
                links,
                views: vec![],
                selected: None,
                fixture_url,
                verifying: verification.is_some(),
                cleanup_error: None,
            };
            for id in &launcher.saved.sessions {
                let view = launcher.create_view(&window, *id)?;
                launcher.views.push((*id, view));
            }
            if let Some((id, _)) = launcher.views.first() {
                launcher.select(*id)?;
            }
            let pending = launcher.saved.pending_removal.clone();
            let root = launcher.root.clone();
            app.manage(Mutex::new(launcher));
            for id in pending {
                remove_store(app.handle().clone(), root.clone(), id);
            }
            if let Some(mode) = verification {
                verify::run(app.handle().clone(), reports, mode, start);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run Queue Launcher prototype");
}
