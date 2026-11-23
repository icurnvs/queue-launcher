use crate::{
    browser::{self, Browser, BrowserEvents, Event, Layout},
    queue_status::{is_waiting_room, QueueObservation, QueueStatus},
    store::{position_key, remaining_selection, Batch, Link, Saved, Session, Store},
    Result, Window, MAX_SESSIONS,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
use tauri::Url;
use uuid::Uuid;

#[cfg(all(debug_assertions, target_os = "macos"))]
pub mod mac_smoke;
#[cfg(all(debug_assertions, target_os = "windows"))]
pub mod smoke;

const LIVE: usize = 0;
const DEMO: usize = 1;
const DEMO_PORT: u16 = 43193;
const DEMO_LINK: &str = "https://queue-launcher.example/local-waiting-room.html";
const FIXTURE: &str = include_str!("../../../Scripts/local-waiting-room.html");

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Status,
    Open { url: String, count: u32 },
    Add,
    End,
    Select { number: u32 },
    Next,
    Mark,
    Copy { number: u32 },
    CopyClose { number: u32 },
    Close { number: u32 },
    Back,
    Forward,
    Stop,
    Reload,
    Retry,
    ToggleDemo,
    Sort { enabled: bool },
    CopyHistory { index: usize },
    OpenLogs,
    Layout(Layout),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    demo: bool,
    url: String,
    current_url: String,
    selected: Option<u32>,
    sessions: Vec<Row>,
    sort: bool,
    history: Vec<Link>,
    notice: Option<String>,
    feedback: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    number: u32,
    ready: bool,
    status: Option<QueueStatus>,
    shareable: bool,
    error: Option<String>,
    can_go_back: bool,
    can_go_forward: bool,
    loading: bool,
    browser_ready: bool,
}

struct View {
    instance: Uuid,
    browser: Option<Browser>,
    error: Option<String>,
    status: Option<QueueStatus>,
    share: Option<Url>,
    observation: Option<QueueObservation>,
    navigating: Option<u64>,
    document: u64,
}

struct Mode {
    store: Store,
    saved: Saved,
    links: Vec<Link>,
    views: HashMap<Uuid, View>,
    feedback: Option<String>,
    notice: Option<String>,
    cleanup_error: bool,
    dirty: bool,
    log_error: bool,
}

impl Mode {
    fn new(path: PathBuf) -> Result<Self> {
        let store = Store::new(path)?;
        let saved = store.read()?;
        let links = store.links()?;
        Ok(Self {
            store,
            saved,
            links,
            views: HashMap::new(),
            feedback: None,
            notice: None,
            cleanup_error: false,
            dirty: false,
            log_error: false,
        })
    }

    fn batch(&self) -> Result<&Batch> {
        self.saved
            .batch
            .as_ref()
            .ok_or("Open a batch first.".into())
    }

    fn selected(&self) -> Option<u32> {
        self.saved.batch.as_ref().and_then(|batch| batch.selected)
    }

    fn current_url(&self) -> Option<Url> {
        let batch = self.saved.batch.as_ref()?;
        let session = batch
            .sessions
            .iter()
            .find(|session| Some(session.number) == batch.selected)?;
        let fallback = session.waiting_room_url.as_ref().unwrap_or(&batch.url);
        Some(
            self.views
                .get(&session.profile)
                .and_then(|view| view.browser.as_ref())
                .and_then(|browser| browser.url().ok())
                .filter(|url| matches!(url.scheme(), "http" | "https" | "file"))
                .unwrap_or_else(|| fallback.clone()),
        )
    }

    fn commit(&mut self, saved: Saved) -> Result<()> {
        self.store
            .save(&saved)
            .map_err(|_| "Could not save changes. Check that the app data folder is writable.")?;
        self.saved = saved;
        self.dirty = false;
        Ok(())
    }

    fn record(
        &mut self,
        batch: Uuid,
        event: &str,
        session: Option<u32>,
        code: Option<i32>,
        ahead: Option<i32>,
    ) {
        if self.store.log(batch, event, session, code, ahead).is_err() {
            self.log_error = true;
        }
    }

    fn ordered(&self) -> Vec<&Session> {
        let Some(batch) = &self.saved.batch else {
            return vec![];
        };
        let mut sessions: Vec<_> = batch.sessions.iter().collect();
        if self.saved.sort {
            sessions.sort_by_key(|session| {
                let status = self
                    .views
                    .get(&session.profile)
                    .and_then(|view| view.status.as_ref());
                position_key(
                    session.number,
                    status.and_then(|status| status.ahead),
                    status.and_then(|status| status.estimated_wait_seconds),
                )
            });
        }
        sessions
    }

    fn capture_links(&mut self) -> Result<()> {
        let Some(batch) = self.saved.batch.as_ref() else {
            return Ok(());
        };
        let mut links = self.links.clone();
        for session in &batch.sessions {
            let Some(view) = self.views.get(&session.profile) else {
                continue;
            };
            let Some(url) = &view.share else { continue };
            if links.iter().any(|link| {
                link.batch == batch.log_id && link.session == session.number && &link.url == url
            }) {
                continue;
            }
            links.push(Link::new(
                batch.log_id,
                session.number,
                url.clone(),
                view.status.as_ref().and_then(|s| s.ahead),
            ));
        }
        if links.len() != self.links.len() {
            self.store
                .write_links(&links)
                .map_err(|_| "Could not save queue links. Check the app data folder.")?;
            self.links = links;
        }
        Ok(())
    }

    fn share_link(&self, session: &Session) -> Option<&Url> {
        self.views
            .get(&session.profile)
            .and_then(|view| view.share.as_ref())
            .or_else(|| {
                let batch = self.saved.batch.as_ref()?;
                self.links
                    .iter()
                    .rev()
                    .find(|link| link.batch == batch.log_id && link.session == session.number)
                    .map(|link| &link.url)
            })
    }

    fn row(&self, session: &Session) -> Row {
        let view = self.views.get(&session.profile);
        let error = view.and_then(|view| view.error.clone());
        let browser = view.and_then(|view| view.browser.as_ref());
        Row {
            number: session.number,
            ready: session.ready,
            status: view.and_then(|view| view.status.clone()),
            shareable: self.share_link(session).is_some(),
            can_go_back: self.selected() == Some(session.number)
                && error.is_none()
                && browser.is_some_and(Browser::can_go_back),
            can_go_forward: self.selected() == Some(session.number)
                && error.is_none()
                && browser.is_some_and(Browser::can_go_forward),
            loading: error.is_none() && view.is_some_and(|view| view.navigating.is_some()),
            browser_ready: error.is_none() && browser.is_some(),
            error,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    url: Url,
    text: String,
    error_id: String,
    challenge_visible: bool,
}

pub struct Launcher {
    modes: [Mode; 2],
    current: usize,
    demo_url: Option<Url>,
    demo_loaded: bool,
    layout: Layout,
    shown: Option<(usize, Uuid)>,
    events: BrowserEvents,
    messages: mpsc::Receiver<(Uuid, Event)>,
    last_sample: Instant,
}

impl Launcher {
    pub fn new(root: PathBuf) -> Result<Self> {
        let (events, messages) = mpsc::channel();
        Ok(Self {
            modes: [Mode::new(root.join("Live"))?, Mode::new(root.join("Demo"))?],
            current: LIVE,
            demo_url: None,
            demo_loaded: false,
            layout: Layout::default(),
            shown: None,
            events,
            messages,
            last_sample: Instant::now(),
        })
    }

    fn active(&self) -> &Mode {
        &self.modes[self.current]
    }
    fn active_mut(&mut self) -> &mut Mode {
        &mut self.modes[self.current]
    }

    pub fn snapshot(&self) -> Snapshot {
        let mode = self.active();
        let notices = [
            mode.notice.as_deref(),
            mode.cleanup_error.then_some("Some browser profiles could not be cleared. Cleanup will retry when the app reopens."),
            mode.log_error.then_some("Some drop log entries could not be written."),
        ].into_iter().flatten().collect::<Vec<_>>();
        Snapshot {
            demo: self.current == DEMO,
            current_url: mode
                .current_url()
                .map(|url| url.to_string())
                .unwrap_or_default(),
            url: mode
                .saved
                .batch
                .as_ref()
                .map(|batch| batch.url.to_string())
                .or_else(|| (self.current == DEMO).then(|| DEMO_LINK.to_owned()))
                .unwrap_or_default(),
            selected: mode.selected(),
            sessions: mode
                .ordered()
                .into_iter()
                .map(|session| mode.row(session))
                .collect(),
            sort: mode.saved.sort,
            history: mode.links.clone(),
            notice: (!notices.is_empty()).then(|| notices.join(" ")),
            feedback: mode.feedback.clone(),
        }
    }

    pub fn restore(&mut self, window: &Window) {
        self.restore_mode(window, LIVE);
        #[cfg(target_os = "macos")]
        {
            self.retry_cleanup(window, LIVE);
            self.retry_cleanup(window, DEMO);
        }
    }

    fn restore_mode(&mut self, window: &Window, mode: usize) {
        if let Some(batch) = self.modes[mode].saved.batch.clone() {
            self.modes[mode].record(batch.log_id, "batch_restored", None, None, None);
            for session in &batch.sessions {
                self.modes[mode].record(
                    batch.log_id,
                    "session_restored",
                    Some(session.number),
                    None,
                    None,
                );
                self.create(window, mode, &batch, session);
            }
        }
        #[cfg(target_os = "windows")]
        self.retry_cleanup(window, mode);
    }

    fn create(&mut self, window: &Window, mode: usize, batch: &Batch, session: &Session) {
        let instance = Uuid::new_v4();
        let root = &self.modes[mode].store.root;
        let start = session.waiting_room_url.as_ref().unwrap_or(&batch.url);
        let result = Browser::create(
            window,
            root,
            session.profile,
            instance,
            &batch.url,
            start,
            mode == DEMO,
            &self.events,
        );
        let (browser, error) = match result {
            Ok(browser) => (Some(browser), None),
            Err(_) => {
                self.modes[mode].record(
                    batch.log_id,
                    "webview_initialization_failed",
                    Some(session.number),
                    None,
                    None,
                );
                (None, Some("Could not open this browser session. Check the browser runtime, then try again.".into()))
            }
        };
        self.modes[mode].views.insert(
            session.profile,
            View {
                instance,
                browser,
                error,
                status: None,
                share: None,
                observation: None,
                navigating: None,
                document: 0,
            },
        );
    }

    fn show_selected(&mut self) -> Result<()> {
        let mode = self.active();
        let selected = mode
            .saved
            .batch
            .as_ref()
            .and_then(|batch| {
                batch
                    .sessions
                    .iter()
                    .find(|session| Some(session.number) == batch.selected)
            })
            .map(|session| session.profile);
        let next = selected
            .filter(|id| {
                self.layout.visible
                    && mode
                        .views
                        .get(id)
                        .is_some_and(|view| view.error.is_none() && view.browser.is_some())
            })
            .map(|id| (self.current, id));
        if self.shown != next {
            if let Some((index, id)) = self.shown.take() {
                if let Some(browser) = self.modes[index]
                    .views
                    .get(&id)
                    .and_then(|view| view.browser.as_ref())
                {
                    browser.visible(false)?;
                }
            }
        }
        if let Some((index, id)) = next {
            if let Some(browser) = self.modes[index]
                .views
                .get(&id)
                .and_then(|view| view.browser.as_ref())
            {
                browser.bounds(self.layout)?;
                if self.shown != next {
                    browser.visible(true)?;
                }
            }
        }
        self.shown = next;
        Ok(())
    }

    fn open(&mut self, window: &Window, url: Url, count: u32) -> Result<()> {
        if !(1..=MAX_SESSIONS).contains(&count) {
            return Err("Choose between 1 and 50 sessions.".into());
        }
        let mode = self.active_mut();
        if mode.saved.batch.is_some() {
            return Err("End the current batch before opening another.".into());
        }
        let batch = Batch::new(url, count);
        let mut saved = mode.saved.clone();
        saved.batch = Some(batch.clone());
        mode.commit(saved)?;
        mode.feedback = None;
        mode.record(batch.log_id, "batch_started", None, None, None);
        for session in &batch.sessions {
            self.active_mut().record(
                batch.log_id,
                "session_opened",
                Some(session.number),
                None,
                None,
            );
            self.create(window, self.current, &batch, session);
        }
        self.show_selected()
    }

    fn add(&mut self, window: &Window) -> Result<()> {
        let mode = self.active_mut();
        let mut batch = mode.batch()?.clone();
        if batch.sessions.len() >= MAX_SESSIONS as usize {
            return Err("The batch already has 50 sessions.".into());
        }
        let session = batch.add()?;
        let mut saved = mode.saved.clone();
        saved.batch = Some(batch.clone());
        mode.commit(saved)?;
        mode.record(
            batch.log_id,
            "session_opened",
            Some(session.number),
            None,
            None,
        );
        self.create(window, self.current, &batch, &session);
        self.show_selected()
    }

    fn toggle_demo(&mut self, window: &Window) -> Result<()> {
        self.active_mut().capture_links()?;
        if self.current == LIVE && !self.demo_loaded {
            let url = start_demo()?;
            self.demo_url = Some(url);
            self.current = DEMO;
            self.restore_mode(window, DEMO);
            self.demo_loaded = true;
            if self.active().saved.batch.is_none() {
                return self.open(window, self.demo_url.clone().unwrap(), 12);
            }
        } else {
            self.current = if self.current == LIVE { DEMO } else { LIVE };
        }
        self.show_selected()
    }

    fn select(&mut self, number: u32) -> Result<()> {
        let mode = self.active_mut();
        let mut saved = mode.saved.clone();
        let batch = saved.batch.as_mut().ok_or("Open a batch first.")?;
        if !batch
            .sessions
            .iter()
            .any(|session| session.number == number)
        {
            return Err("Session is no longer open.".into());
        }
        if batch.selected != Some(number) {
            batch.selected = Some(number);
            mode.commit(saved)?;
            self.show_selected()?;
        }
        Ok(())
    }

    fn mark(&mut self) -> Result<()> {
        let mode = self.active_mut();
        let selected = mode.selected().ok_or("Select a session first.")?;
        let mut saved = mode.saved.clone();
        let batch = saved.batch.as_mut().ok_or("Open a batch first.")?;
        let session = batch
            .sessions
            .iter_mut()
            .find(|session| session.number == selected)
            .ok_or("Session is no longer open.")?;
        session.ready = !session.ready;
        let event = if session.ready {
            "marked_ready"
        } else {
            "marked_pending"
        };
        let batch_id = batch.log_id;
        mode.commit(saved)?;
        mode.record(batch_id, event, Some(selected), None, None);
        Ok(())
    }

    fn remove(&mut self, window: &Window, removed: &[u32]) -> Result<()> {
        let index = self.current;
        let mode = self.active_mut();
        mode.capture_links()?;
        let batch = mode.batch()?.clone();
        let closing: Vec<_> = batch
            .sessions
            .iter()
            .filter(|session| removed.contains(&session.number))
            .cloned()
            .collect();
        if closing.is_empty() {
            return Err("Session is no longer open.".into());
        }
        let ordered: Vec<_> = mode
            .ordered()
            .into_iter()
            .map(|session| session.number)
            .collect();
        let mut next = batch.clone();
        next.selected = remaining_selection(&ordered, batch.selected, removed);
        next.sessions
            .retain(|session| !removed.contains(&session.number));
        let ended = next.sessions.is_empty();
        let mut saved = mode.saved.clone();
        saved
            .pending_cleanup
            .extend(closing.iter().map(|session| session.profile));
        saved.batch = (!ended).then_some(next);
        mode.commit(saved)?;
        for session in closing {
            let mode = &mut self.modes[index];
            mode.record(
                batch.log_id,
                "session_closed",
                Some(session.number),
                None,
                None,
            );
            let view = mode
                .views
                .remove(&session.profile)
                .and_then(|view| view.browser);
            self.delete_profile(window, index, session.profile, view);
        }
        if ended {
            self.active_mut()
                .record(batch.log_id, "batch_ended", None, None, None);
        }
        self.show_selected()
    }

    fn retry_cleanup(&mut self, window: &Window, index: usize) {
        for id in self.modes[index].saved.pending_cleanup.clone() {
            self.delete_profile(window, index, id, None);
        }
    }

    #[cfg(target_os = "windows")]
    fn delete_profile(
        &mut self,
        window: &Window,
        index: usize,
        id: Uuid,
        browser: Option<Browser>,
    ) {
        let outcome = match browser {
            Some(browser) => browser.delete(),
            None => crate::named_profiles::retry_delete(window, &self.modes[index].store.root, id),
        };
        self.finish_cleanup(index, id, outcome.is_ok());
    }

    #[cfg(target_os = "macos")]
    fn delete_profile(
        &mut self,
        _window: &Window,
        index: usize,
        id: Uuid,
        browser: Option<Browser>,
    ) {
        drop(browser);
        let events = self.events.clone();
        browser::remove_store(id, move |ok| {
            let _ = events.send((id, Event::Removed(index, ok)));
        });
    }

    fn finish_cleanup(&mut self, index: usize, id: Uuid, ok: bool) {
        let mode = &mut self.modes[index];
        if ok {
            let mut saved = mode.saved.clone();
            saved.pending_cleanup.retain(|other| *other != id);
            if mode.commit(saved).is_err() {
                mode.cleanup_error = true;
            } else if mode.saved.pending_cleanup.is_empty() {
                mode.cleanup_error = false;
            }
        } else {
            mode.cleanup_error = true;
        }
    }

    fn selected_browser(&self) -> Result<&Browser> {
        let mode = self.active();
        let batch = mode.batch()?;
        let session = batch
            .sessions
            .iter()
            .find(|session| Some(session.number) == batch.selected)
            .ok_or("Select a session first.")?;
        mode.views
            .get(&session.profile)
            .and_then(|view| view.browser.as_ref())
            .ok_or("The session browser is unavailable.".into())
    }

    fn retry(&mut self, window: &Window) -> Result<()> {
        let batch = self.active().batch()?.clone();
        let session = batch
            .sessions
            .iter()
            .find(|session| Some(session.number) == batch.selected)
            .ok_or("Select a session first.")?;
        if let Some(browser) = self
            .active_mut()
            .views
            .remove(&session.profile)
            .and_then(|view| view.browser)
        {
            browser.discard()?;
        }
        self.create(window, self.current, &batch, session);
        self.shown = None;
        self.show_selected()
    }

    fn copy(&mut self, number: u32) -> Result<()> {
        let mode = self.active_mut();
        let session = mode
            .batch()?
            .sessions
            .iter()
            .find(|session| session.number == number)
            .ok_or("Session is no longer open.")?;
        let url = mode
            .share_link(session)
            .ok_or("No share link is available for this session.")?;
        clipboard(url)?;
        mode.feedback = Some(format!("Copied Session {number} link."));
        Ok(())
    }

    pub fn session_menu(
        &self,
        window: &Window,
        number: u32,
    ) -> Result<tauri::menu::Menu<tauri::Wry>> {
        use tauri::menu::{Menu, MenuItem};
        let mode = self.active();
        let session = mode
            .batch()?
            .sessions
            .iter()
            .find(|session| session.number == number)
            .ok_or("Session is no longer open.")?;
        let shareable = mode.share_link(session).is_some();
        let item = |action: &str, label: &str, enabled| {
            MenuItem::with_id(
                window,
                format!("session/{}/{action}", session.profile),
                label,
                enabled,
                None::<&str>,
            )
        };
        Ok(Menu::with_items(
            window,
            &[
                &item("copy", "Copy link", shareable)?,
                &item("copy_close", "Copy and close", shareable)?,
                &item("close", "Close session", true)?,
            ],
        )?)
    }

    pub fn menu_action(&mut self, window: &Window, id: &str) -> Result<()> {
        let mut parts = id.split('/');
        if parts.next() != Some("session") {
            return Ok(());
        }
        let profile = parts.next().and_then(|id| Uuid::parse_str(id).ok());
        let action = parts.next();
        if parts.next().is_some() {
            return Ok(());
        }
        let number = self
            .active()
            .saved
            .batch
            .as_ref()
            .and_then(|batch| {
                batch
                    .sessions
                    .iter()
                    .find(|session| Some(session.profile) == profile)
            })
            .map(|session| session.number);
        let Some(number) = number else {
            return Ok(());
        };
        let action = match action {
            Some("copy") => Action::Copy { number },
            Some("copy_close") => Action::CopyClose { number },
            Some("close") => Action::Close { number },
            _ => return Ok(()),
        };
        self.update();
        if let Err(error) = self.act(window, action) {
            self.active_mut().notice = Some(error.to_string());
        }
        Ok(())
    }

    pub fn act(&mut self, window: &Window, action: Action) -> Result<()> {
        match action {
            Action::Status => Ok(()),
            Action::Open { url, count } => {
                let url = if self.current == DEMO {
                    self.demo_url.clone().ok_or("Demo mode is unavailable.")?
                } else {
                    browser::destination(&url)?
                };
                self.open(window, url, count)
            }
            Action::Add => self.add(window),
            Action::End => {
                let numbers: Vec<_> = self
                    .active()
                    .ordered()
                    .iter()
                    .map(|session| session.number)
                    .collect();
                self.remove(window, &numbers)
            }
            Action::Select { number } => self.select(number),
            Action::Next => {
                let mode = self.active();
                let ordered = mode.ordered();
                if ordered.is_empty() {
                    return Ok(());
                }
                let next = mode
                    .selected()
                    .and_then(|current| {
                        ordered.iter().position(|session| session.number == current)
                    })
                    .map_or(0, |index| (index + 1) % ordered.len());
                self.select(ordered[next].number)
            }
            Action::Mark => self.mark(),
            Action::Copy { number } => self.copy(number),
            Action::CopyClose { number } => {
                self.copy(number)?;
                self.remove(window, &[number])?;
                self.active_mut().feedback =
                    Some(format!("Copied Session {number} link. Session closed."));
                Ok(())
            }
            Action::Close { number } => self.remove(window, &[number]),
            Action::Back => self.selected_browser()?.back(),
            Action::Forward => self.selected_browser()?.forward(),
            Action::Stop => {
                self.selected_browser()?.stop()?;
                let mode = self.active_mut();
                let selected = mode.selected();
                let profile = mode
                    .batch()?
                    .sessions
                    .iter()
                    .find(|session| Some(session.number) == selected)
                    .map(|session| session.profile);
                if let Some(view) = profile.and_then(|profile| mode.views.get_mut(&profile)) {
                    // Ignore the cancelled load's eventual completion, including
                    // WebView2's connection-aborted result. New loads get a new ID.
                    view.navigating = None;
                }
                Ok(())
            }
            Action::Reload => {
                self.selected_browser()?.reload()?;
                let mode = self.active_mut();
                let batch = mode.batch()?;
                mode.record(batch.log_id, "manual_reload", batch.selected, None, None);
                Ok(())
            }
            Action::Retry => self.retry(window),
            Action::ToggleDemo => self.toggle_demo(window),
            Action::Sort { enabled } => {
                let mode = self.active_mut();
                let mut saved = mode.saved.clone();
                saved.sort = enabled;
                mode.commit(saved)
            }
            Action::CopyHistory { index } => {
                let mode = self.active_mut();
                let link = mode
                    .links
                    .get(index)
                    .ok_or("That saved link is unavailable.")?;
                clipboard(&link.url)?;
                mode.feedback = Some(format!("Copied Session {} link.", link.session));
                Ok(())
            }
            Action::OpenLogs => open_logs(self.active().store.log_dir()),
            Action::Layout(mut layout) => {
                let size = window
                    .inner_size()?
                    .to_logical::<f64>(window.scale_factor()?);
                layout.x = layout.x.clamp(0., size.width);
                layout.y = layout.y.clamp(0., size.height);
                layout.width = layout.width.clamp(0., size.width - layout.x);
                layout.height = layout.height.clamp(0., size.height - layout.y);
                layout.visible &= layout.width >= 1. && layout.height >= 1.;
                self.layout = layout;
                self.show_selected()
            }
        }
    }

    pub fn update(&mut self) {
        let mut redraw = false;
        while let Ok((instance, event)) = self.messages.try_recv() {
            #[cfg(target_os = "macos")]
            if let Event::Removed(index, ok) = event {
                self.finish_cleanup(index, instance, ok);
                continue;
            }
            let Some((index, profile)) = self.modes.iter().enumerate().find_map(|(index, mode)| {
                mode.views
                    .iter()
                    .find(|(_, view)| view.instance == instance)
                    .map(|(&id, _)| (index, id))
            }) else {
                continue;
            };
            let mode = &mut self.modes[index];
            let Some(batch) = mode.saved.batch.as_mut() else {
                continue;
            };
            let Some(session) = batch
                .sessions
                .iter_mut()
                .find(|session| session.profile == profile)
            else {
                continue;
            };
            let view = mode.views.get_mut(&profile).expect("matching session view");
            let batch_id = batch.log_id;
            let session_number = Some(session.number);
            let original = &batch.url;

            match event {
                Event::Starting(id) => {
                    view.navigating = Some(id);
                    view.document = view.document.wrapping_add(1);
                    view.status = None;
                    view.share = None;
                    view.observation = None;
                    redraw |= view.error.take().is_some();
                }
                Event::Finished(id, error) => {
                    if view.navigating != Some(id) {
                        continue;
                    }
                    view.navigating = None;
                    if let Some(code) = error {
                        view.error = Some(format!("Page could not load (error {code})."));
                        redraw = true;
                        mode.record(
                            batch_id,
                            "url_load_failed",
                            session_number,
                            Some(code),
                            None,
                        );
                    } else if let Some(url) =
                        view.browser.as_ref().and_then(|browser| browser.url().ok())
                    {
                        if is_waiting_room(&url) {
                            mode.record(
                                batch_id,
                                "waiting_room_page_loaded",
                                session_number,
                                None,
                                None,
                            );
                        } else if session.waiting_room_url.is_some() {
                            mode.record(batch_id, "left_waiting_room", session_number, None, None);
                        }
                    }
                }
                Event::Terminated(code) => {
                    view.error = Some("This session's browser process closed.".into());
                    view.status = None;
                    view.share = None;
                    view.observation = None;
                    view.navigating = None;
                    view.document = view.document.wrapping_add(1);
                    redraw = true;
                    mode.record(
                        batch_id,
                        "web_process_terminated",
                        session_number,
                        code,
                        None,
                    );
                }
                Event::Popup(url) => {
                    if browser::allows(&url, original, index == DEMO) {
                        if let Some(browser) = view.browser.as_ref() {
                            let _ = browser.navigate(url);
                        }
                    }
                }
                Event::Sample(document, json) => {
                    if view.document != document
                        || view.navigating.is_some()
                        || view.error.is_some()
                    {
                        continue;
                    }
                    let Ok(page) = serde_json::from_str::<Page>(&json) else {
                        continue;
                    };
                    let current = view.browser.as_ref().and_then(|browser| browser.url().ok());
                    if current.as_ref() != Some(&page.url)
                        || !observed_url(&page.url, original, index == DEMO)
                    {
                        continue;
                    }
                    let status = QueueStatus::parse(&page.text, page.error_id.trim().parse().ok());
                    if let Some(resume) = status.resume_url(&page.url) {
                        if session.waiting_room_url.as_ref() != Some(&resume) {
                            session.waiting_room_url = Some(resume);
                            mode.dirty = true;
                        }
                    }
                    view.share = if index == DEMO {
                        status
                            .queue_id
                            .filter(|_| status.ahead.is_some())
                            .and_then(|id| Url::parse(&format!("{DEMO_LINK}?q={id}")).ok())
                    } else {
                        status.share_url(&page.url)
                    };
                    let observation = status.observation(page.challenge_visible);
                    view.status = Some(status);
                    if observation != view.observation {
                        view.observation = observation.clone();
                        if let Some(obs) = observation {
                            mode.record(
                                batch_id,
                                obs.event,
                                session_number,
                                obs.code,
                                obs.members_ahead,
                            );
                        }
                    }
                }
                #[cfg(target_os = "macos")]
                Event::Removed(..) => unreachable!(),
            }
        }
        if redraw && self.show_selected().is_err() {
            self.active_mut().notice = Some("Could not update the session view.".into());
        }
        if self.last_sample.elapsed() < Duration::from_secs(3) {
            return;
        }

        self.last_sample = Instant::now();
        for mode in &mut self.modes {
            let outcome = mode.capture_links().and_then(|_| {
                if mode.dirty {
                    mode.commit(mode.saved.clone())?;
                }
                Ok(())
            });
            if let Err(error) = outcome {
                mode.notice = Some(error.to_string());
            } else if mode
                .notice
                .as_deref()
                .is_some_and(|notice| notice.starts_with("Could not save"))
            {
                mode.notice = None;
            }
        }
        let mode = self.active();
        let Some(batch) = mode.saved.batch.as_ref() else {
            return;
        };
        for view in mode.views.values() {
            if view.navigating.is_some() || view.error.is_some() {
                continue;
            }
            let Some(browser) = &view.browser else {
                continue;
            };
            if browser
                .url()
                .is_ok_and(|url| observed_url(&url, &batch.url, self.current == DEMO))
            {
                let instance = view.instance;
                let document = view.document;
                let events = self.events.clone();
                let _ = browser.sample(move |json| {
                    let _ = events.send((instance, Event::Sample(document, json)));
                });
            }
        }
    }
}

fn observed_url(url: &Url, original: &Url, demo: bool) -> bool {
    is_waiting_room(url) || (url == original && (demo || url.scheme() == "file"))
}

fn start_demo() -> Result<Url> {
    let server = tiny_http::Server::http(("127.0.0.1", DEMO_PORT)).map_err(|_| {
        "Demo mode could not start its local waiting room. Check that port 43193 is free."
    })?;
    let page = FIXTURE.replace("location.hostname === 'queue-launcher.example'", "true");
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let response = if request.method() == &tiny_http::Method::Get
                && request.url() == "/local-waiting-room.html"
            {
                tiny_http::Response::from_string(page.clone())
                    .with_header(tiny_http::Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap())
                    .with_header(tiny_http::Header::from_bytes("Cache-Control", "no-store").unwrap())
                    .with_header(tiny_http::Header::from_bytes("Content-Security-Policy",
                        "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'").unwrap())
            } else {
                tiny_http::Response::from_string("").with_status_code(404)
            };
            let _ = request.respond(response);
        }
    });
    Ok(format!("http://127.0.0.1:{DEMO_PORT}/local-waiting-room.html").parse()?)
}

fn clipboard(url: &Url) -> Result<()> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(url.to_string()))
        .map_err(|_| "Could not copy the link. Check the clipboard and try again.".into())
}

fn open_logs(path: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&path)?;
    let command = if cfg!(target_os = "windows") {
        "explorer.exe"
    } else {
        "open"
    };
    std::process::Command::new(command).arg(path).spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_url_follows_selection_before_browser_views_are_restored() {
        let root = std::env::temp_dir().join(format!("queue-current-url-{}", Uuid::new_v4()));
        let mut mode = Mode::new(root.clone()).unwrap();
        assert_eq!(mode.current_url(), None);

        let original: Url = "https://queue-launcher.example/release".parse().unwrap();
        let resume: Url = "https://queue-launcher.example/waiting?session=one"
            .parse()
            .unwrap();
        let mut batch = Batch::new(original.clone(), 2);
        batch.sessions[0].waiting_room_url = Some(resume.clone());
        batch.selected = Some(1);
        mode.saved.batch = Some(batch);
        assert_eq!(mode.current_url(), Some(resume));

        mode.saved.batch.as_mut().unwrap().selected = Some(2);
        assert_eq!(mode.current_url(), Some(original));
        mode.saved.batch.as_mut().unwrap().selected = None;
        assert_eq!(mode.current_url(), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restored_links_are_copyable_before_any_session_is_selected_or_sampled() {
        let root = std::env::temp_dir().join(format!("queue-restored-links-{}", Uuid::new_v4()));
        let batch = Batch::new("https://queue-launcher.example/release".parse().unwrap(), 2);
        let link = Link::new(
            batch.log_id,
            1,
            "https://queue-launcher.example/saved".parse().unwrap(),
            Some(12),
        );
        let store = Store::new(root.clone()).unwrap();
        store
            .save(&Saved {
                batch: Some(batch.clone()),
                ..Saved::default()
            })
            .unwrap();
        store.write_links(&[link.clone()]).unwrap();

        let mut mode = Mode::new(root.clone()).unwrap();
        assert!(mode.views.is_empty());
        assert!(mode.row(&batch.sessions[0]).shareable);
        assert_eq!(mode.share_link(&batch.sessions[0]), Some(&link.url));
        assert!(!mode.row(&batch.sessions[1]).shareable);

        // Session numbers are reused in new batches. Earlier links must never
        // become the copy target for a new session with the same number.
        mode.saved.batch = Some(Batch::new(batch.url.clone(), 2));
        let new_session = &mode.saved.batch.as_ref().unwrap().sessions[0];
        assert!(!mode.row(new_session).shareable);
        std::fs::remove_dir_all(root).unwrap();
    }
}
