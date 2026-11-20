use super::{Action, Launcher, Snapshot, LIVE};
use crate::{browser, on_main, Result, Window};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tiny_http::{Header, Response, Server};
use uuid::Uuid;

const ORIGIN: &str = "http://127.0.0.1:43194";
const TIMEOUT: Duration = Duration::from_secs(45);
const PAGE: &str = r#"<!doctype html>
<meta charset="utf-8">
<title>Queue Launcher offline check</title>
<input aria-label="Fixture text">
<a href="/second" target="_blank">Open in same session</a>
<script>
const storage = localStorage.fixture ||= crypto.randomUUID();
const cookie = document.cookie.split('fixture=')[1] || crypto.randomUUID();
document.cookie = 'fixture=' + cookie + '; Max-Age=86400; Path=/';
window.fixture = { storage, cookie, page: crypto.randomUUID() };
</script>"#;

#[derive(Clone, Copy)]
pub enum Phase {
    Initial,
    Reopen,
    Cleanup,
    Fifty,
}

pub fn phase() -> Result<Option<Phase>> {
    let mut args = std::env::args();
    if !args.any(|arg| arg == "--verify-macos-offline") {
        return Ok(None);
    }
    Ok(Some(match args.next().as_deref() {
        Some("initial") => Phase::Initial,
        Some("reopen") => Phase::Reopen,
        Some("cleanup") => Phase::Cleanup,
        Some("50") => Phase::Fifty,
        _ => return Err("Choose initial, reopen, cleanup, or 50 for offline verification.".into()),
    }))
}

impl Phase {
    pub fn prepare(self, root: &Path) -> Result<()> {
        ensure(
            std::env::var_os("QUEUE_LAUNCHER_DATA").is_some(),
            "Offline verification requires QUEUE_LAUNCHER_DATA.",
        )?;
        let fresh = matches!(self, Self::Initial | Self::Fifty);
        ensure(
            if fresh { !root.try_exists()? } else { root.join("offline-evidence.json").is_file() },
            "Use a new scratch directory for initial/50, or reuse the initial scratch for reopen/cleanup.",
        )?;
        start_fixture()
    }
}

#[derive(Deserialize, Serialize)]
struct Markers {
    storage: Uuid,
    cookie: Uuid,
    page: Uuid,
}

#[derive(Deserialize, Serialize)]
struct Evidence {
    ids: Vec<Uuid>,
    markers: Vec<Markers>,
}

pub fn run(app: AppHandle, phase: Phase) {
    thread::spawn(move || {
        let check = Check(app);
        let code = crate::verification::exit_code(|| match phase {
            Phase::Initial => check.initial(),
            Phase::Reopen => check.reopen(),
            Phase::Cleanup => check.cleanup(),
            Phase::Fifty => check.scale(50),
        });
        check.0.exit(code);
    });
}

fn start_fixture() -> Result<()> {
    let server = Server::http("127.0.0.1:43194").map_err(|error| error.to_string())?;
    thread::spawn(move || {
        let mut first_retry = true;
        for request in server.incoming_requests() {
            let location = match request.url() {
                "/redirect" => Some("/home"),
                "/retry" if first_retry => {
                    first_retry = false;
                    Some("http://127.0.0.1:1/unavailable")
                }
                _ => None,
            };
            if let Some(location) = location {
                let _ =
                    request.respond(Response::empty(302).with_header(header("Location", location)));
            } else if request.url() == "/slow" {
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(2));
                    let _ = request.respond(
                        Response::from_string(PAGE)
                            .with_header(header("Content-Type", "text/html")),
                    );
                });
            } else {
                let _ = request.respond(
                    Response::from_string(PAGE)
                        .with_header(header("Content-Type", "text/html"))
                        .with_header(header("Cache-Control", "no-store")),
                );
            }
        }
    });
    Ok(())
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name, value).expect("valid fixture header")
}

struct Check(AppHandle);

impl Check {
    fn main<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Window, &mut Launcher) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        Ok(on_main(&self.0, |window, launcher| {
            launcher.update();
            work(window, launcher)
        })?)
    }

    fn act(&self, action: Action) -> Result<Snapshot> {
        self.main(move |window, launcher| {
            launcher.act(window, action)?;
            Ok(launcher.snapshot())
        })
    }

    fn open(&self, path: &str, count: u32) -> Result<()> {
        self.act(Action::Open {
            url: format!("{ORIGIN}{path}"),
            count,
        })?;
        Ok(())
    }

    fn ready(&self, count: usize, path: &str) -> Result<()> {
        let start = Instant::now();
        let path = path.to_string();
        loop {
            let path = path.clone();
            let (ready, loading, failed) = self.main(move |_, launcher| {
                let mut ready = 0;
                let mut loading = 0;
                let mut failed = 0;
                for view in launcher.active().views.values() {
                    if view.error.is_some() {
                        failed += 1;
                    } else if view.document > 0
                        && view.navigating.is_none()
                        && view.browser.as_ref().is_some_and(|browser| {
                            browser.url().is_ok_and(|url| url.path() == path)
                        })
                    {
                        ready += 1;
                    } else {
                        loading += 1;
                    }
                }
                Ok((ready, loading, failed))
            })?;
            if ready == count {
                return Ok(());
            }
            if failed > 0 || start.elapsed() >= Duration::from_secs(120) {
                return Err(format!(
                    "fixture navigation: {ready}/{count} ready, {loading} loading, {failed} failed"
                )
                .into());
            }
            thread::sleep(Duration::from_millis(75));
        }
    }

    fn eval(&self, number: u32, script: &'static str) -> Result<String> {
        let (send, receive) = mpsc::channel();
        self.main(move |_, launcher| {
            let mode = launcher.active();
            let profile = mode
                .batch()?
                .sessions
                .iter()
                .find(|session| session.number == number)
                .ok_or("fixture session missing")?
                .profile;
            let browser = mode.views[&profile]
                .browser
                .as_ref()
                .ok_or("fixture browser missing")?;
            browser.evaluate(script, move |text| {
                let _ = send.send(text);
            })
        })?;
        Ok(receive
            .recv_timeout(TIMEOUT)
            .map_err(|_| "fixture JavaScript did not return")?)
    }

    fn chrome(&self, script: &'static str) -> Result<String> {
        let (send, receive) = mpsc::channel();
        let script = script.to_owned();
        self.main(move |window, _| {
            window
                .get_webview("app")
                .ok_or("launcher webview missing")?
                .eval_with_callback(script, move |value| {
                    let _ = send.send(value);
                })?;
            Ok(())
        })?;
        Ok(receive.recv_timeout(TIMEOUT)?)
    }

    fn controls_visible(&self) -> Result<()> {
        wait("launcher row actions and layout", || {
            let value = self.chrome(
                r#"(() => {
                const slot = controls["browser-slot"].getBoundingClientRect();
                const actions = controls["session-actions"].getBoundingClientRect();
                return !modal && actions.bottom <= slot.top &&
                    [...rows.values()].every(row =>
                        row.copy.isConnected && row.copyClose.isConnected &&
                        !row.copy.hidden && !row.copyClose.hidden &&
                        row.copy.getBoundingClientRect().top >= row.item.getBoundingClientRect().top &&
                        row.copyClose.getBoundingClientRect().bottom <= row.item.getBoundingClientRect().bottom) &&
                    layoutSignature?.split(",").slice(0, 2).join(",") ===
                        [Math.ceil(slot.left), Math.ceil(slot.top)].join(",");
            })()"#,
            )?;
            if value != "true" {
                return Ok(false);
            }
            let json: String =
                serde_json::from_str(&self.chrome("JSON.stringify([innerWidth, innerHeight])")?)?;
            let viewport: Vec<f64> = serde_json::from_str(&json)?;
            self.main(move |_, launcher| {
                let native_size = launcher.selected_browser()?.viewport_size()?;
                if (viewport[0] - native_size.0).abs() >= 1.
                    || (viewport[1] - native_size.1).abs() >= 1.
                {
                    return Ok(false);
                }
                let frame = launcher.selected_browser()?.frame_in_launcher()?;
                let layout = launcher.layout;
                Ok(frame.visible
                    && (frame.x - layout.x).abs() < 1.
                    && (frame.y - layout.y).abs() < 1.
                    && (frame.width - layout.width).abs() < 1.
                    && (frame.height - layout.height).abs() < 1.)
            })
        })
    }

    fn markers(&self, number: u32) -> Result<Markers> {
        Ok(serde_json::from_str(
            &self.eval(number, "JSON.stringify(window.fixture)")?,
        )?)
    }

    fn store_exists(&self, id: Uuid) -> Result<bool> {
        let (send, receive) = mpsc::channel();
        self.main(move |_, _| {
            browser::store_exists(id, move |exists| {
                let _ = send.send(exists);
            });
            Ok(())
        })?;
        Ok(receive.recv_timeout(TIMEOUT)?)
    }

    fn ids(&self) -> Result<Vec<Uuid>> {
        self.main(|_, launcher| {
            Ok(launcher
                .active()
                .batch()?
                .sessions
                .iter()
                .map(|s| s.profile)
                .collect())
        })
    }

    fn wait_for_cleanup(&self) -> Result<()> {
        wait("profile cleanup journal to clear", || {
            self.main(|_, launcher| {
                ensure(
                    !launcher.active().cleanup_error,
                    "WebKit reported profile cleanup failure",
                )?;
                Ok(launcher.active().saved.pending_cleanup.is_empty())
            })
        })
    }

    fn initial(&self) -> Result<()> {
        self.open("/redirect", 2)?;
        self.ready(2, "/home")?;
        let first = self.markers(1)?;
        let second = self.markers(2)?;
        ensure(
            first.storage != second.storage && first.cookie != second.cookie,
            "same-origin stores shared data",
        )?;
        ensure(
            self.eval(1, "String(typeof window.__TAURI_INTERNALS__)")? == "undefined",
            "site view has Tauri IPC",
        )?;
        self.eval(1, "document.querySelector('input').value = 'same page'")?;
        self.act(Action::Select { number: 2 })?;
        self.act(Action::Select { number: 1 })?;
        ensure(
            self.markers(1)?.page == first.page
                && self.eval(1, "document.querySelector('input').value")? == "same page",
            "switch reloaded or replaced the document",
        )?;

        self.eval(1, "document.querySelector('a').click(); 'clicked'")?;
        wait("popup to open in its original session", || {
            self.main(|_, launcher| {
                Ok(launcher.selected_browser()?.url()?.path() == "/second"
                    && launcher.snapshot().sessions[0].can_go_back)
            })
        })?;
        self.act(Action::Back)?;
        self.ready(2, "/home")?;
        ensure(
            !self.act(Action::Status)?.sessions[0].can_go_back,
            "Back stayed enabled at the first page",
        )?;
        ensure(
            self.act(Action::Status)?.sessions[0].can_go_forward,
            "Forward was not enabled after Back",
        )?;
        self.act(Action::Forward)?;
        wait("Forward to return to the second page", || {
            self.main(|_, launcher| {
                Ok(launcher.selected_browser()?.url()?.path() == "/second"
                    && !launcher.snapshot().sessions[0].loading)
            })
        })?;
        self.act(Action::Back)?;
        self.ready(2, "/home")?;
        let before = self.markers(1)?;
        self.act(Action::Reload)?;
        self.ready(2, "/home")?;
        let after = self.markers(1)?;
        ensure(
            before.page != after.page
                && before.storage == after.storage
                && before.cookie == after.cookie,
            "Reload did not replace the page while preserving storage",
        )?;

        self.main(|_, launcher| {
            launcher
                .selected_browser()?
                .navigate(format!("{ORIGIN}/slow").parse()?)
        })?;
        wait("slow page to start loading", || {
            Ok(self.act(Action::Status)?.sessions[0].loading)
        })?;
        self.act(Action::Stop)?;
        wait("Stop to finish without a page error", || {
            let snapshot = self.act(Action::Status)?;
            Ok(!snapshot.sessions[0].loading && snapshot.sessions[0].error.is_none())
        })?;
        self.main(|_, launcher| {
            let browser = launcher.selected_browser()?;
            browser.navigate(format!("{ORIGIN}/slow").parse()?)?;
            browser.navigate(format!("{ORIGIN}/home").parse()?)?;
            Ok(())
        })?;
        self.ready(2, "/home")?;
        thread::sleep(Duration::from_millis(2200));
        self.ready(2, "/home")?;
        self.act(Action::Select { number: 2 })?;
        self.act(Action::Mark)?;
        let evidence = Evidence {
            ids: self.ids()?,
            markers: vec![self.markers(1)?, self.markers(2)?],
        };
        self.main(move |_, launcher| {
            let root = launcher.active().store.root.parent().unwrap();
            std::fs::write(
                root.join("offline-evidence.json"),
                serde_json::to_vec(&evidence)?,
            )?;
            Ok(())
        })?;

        self.main(|_, launcher| {
            let mode = &mut launcher.modes[LIVE];
            let batch = mode.batch()?;
            let links: Vec<_> = batch
                .sessions
                .iter()
                .map(|session| {
                    super::Link::new(
                        batch.log_id,
                        session.number,
                        format!("https://queue-launcher.example/saved/{}", session.number)
                            .parse()
                            .unwrap(),
                        Some(session.number as i32),
                    )
                })
                .collect();
            mode.store.write_links(&links)?;
            mode.links = links;
            Ok(())
        })?;

        self.scale(12)?;
        println!("PASS redirect, same-session popup, native Back/Forward, Reload and Stop, overlapping navigation, same-origin cookie/storage isolation, no IPC, and switching without reload");
        Ok(())
    }

    fn read_evidence(&self) -> Result<Evidence> {
        self.main(|_, launcher| {
            let root = launcher.active().store.root.parent().unwrap();
            Ok(serde_json::from_slice(&std::fs::read(
                root.join("offline-evidence.json"),
            )?)?)
        })
    }

    fn reopen(&self) -> Result<()> {
        ensure(
            self.act(Action::Status)?
                .sessions
                .iter()
                .all(|session| session.shareable),
            "restored Copy actions required selecting or sampling sessions",
        )?;
        self.act(Action::Copy { number: 1 })?;
        ensure(
            arboard::Clipboard::new()?.get_text()? == "https://queue-launcher.example/saved/1",
            "restored Copy used the wrong saved link",
        )?;
        self.ready(2, "/home")?;
        let evidence = self.read_evidence()?;
        let snapshot = self.act(Action::Status)?;
        ensure(
            self.ids()? == evidence.ids
                && snapshot.selected == Some(2)
                && snapshot.sessions[1].ready,
            "batch IDs, selection or ready state changed on relaunch",
        )?;
        for (index, previous) in evidence.markers.iter().enumerate() {
            let restored = self.markers(index as u32 + 1)?;
            ensure(
                restored.storage == previous.storage
                    && restored.cookie == previous.cookie
                    && restored.page != previous.page,
                "persistent store did not reopen in a new document",
            )?;
        }
        ensure(
            self.store_exists(evidence.ids[0])?,
            "live store was absent before close",
        )?;
        let before = self.markers(2)?;
        self.act(Action::Close { number: 1 })?;
        self.wait_for_cleanup()?;
        let remaining = self.markers(2)?;
        ensure(
            !self.store_exists(evidence.ids[0])?
                && remaining.page == before.page
                && remaining.storage == before.storage
                && remaining.cookie == before.cookie,
            "selective deletion changed the surviving page or retained the closed store",
        )?;
        // Persist the state an abrupt exit leaves after committing End and
        // before requesting WebKit deletion. Keep the store live until exit.
        self.main(|_, launcher| {
            let mode = &mut launcher.modes[LIVE];
            let mut saved = mode.saved.clone();
            let batch = saved.batch.take().ok_or("live batch missing")?;
            saved
                .pending_cleanup
                .extend(batch.sessions.iter().map(|session| session.profile));
            mode.commit(saved)
        })?;
        ensure(
            self.store_exists(evidence.ids[1])?,
            "interrupted cleanup fixture has no store to recover",
        )?;
        println!("PASS live relaunch and selective deletion; left a persisted cleanup journal for the next launch");
        Ok(())
    }

    fn cleanup(&self) -> Result<()> {
        let evidence = self.read_evidence()?;
        self.wait_for_cleanup()?;
        ensure(
            !self.store_exists(evidence.ids[1])? && self.act(Action::Status)?.sessions.is_empty(),
            "startup failed to remove the journaled store",
        )?;
        self.open("/retry", 1)?;
        wait("provisional load error", || {
            let snapshot = self.act(Action::Status)?;
            Ok(snapshot.sessions[0].error.is_some() && !snapshot.sessions[0].browser_ready)
        })?;
        self.act(Action::Retry)?;
        self.ready(1, "/retry")?;
        let id = self.ids()?[0];
        self.act(Action::End)?;
        self.wait_for_cleanup()?;
        ensure(!self.store_exists(id)?, "End left a live store on disk")?;
        println!("PASS journal recovery on launch, provisional connection failure, Retry, and live End cleanup");
        Ok(())
    }

    fn scale(&self, count: u32) -> Result<()> {
        let start = Instant::now();
        if count == 12 {
            self.act(Action::ToggleDemo)?;
        } else {
            self.open("/home", count)?;
        }
        let created = start.elapsed();
        if count == 12 {
            wait("all 12 Demo queue statuses to be sampled", || {
                let snapshot = self.act(Action::Status)?;
                Ok(snapshot.sessions.len() == 12
                    && snapshot.sessions.iter().all(|row| row.shareable))
            })?;
        } else {
            self.ready(count as usize, "/home")?;
        }
        let script = if count == 12 {
            "window.switchMarker ||= crypto.randomUUID()"
        } else {
            "window.fixture.page"
        };
        let markers = (1..=count)
            .map(|number| self.eval(number, script))
            .collect::<Result<Vec<_>>>()?;
        ensure(
            markers.iter().collect::<HashSet<_>>().len() == count as usize,
            "fixture markers were missing or duplicated",
        )?;
        let ready = start.elapsed();
        let mut switches = Vec::new();
        for number in [count, count / 2, 1] {
            let switched = Instant::now();
            self.act(Action::Select { number })?;
            ensure(
                self.eval(number, script)? == markers[number as usize - 1],
                "scale switch reloaded a page",
            )?;
            switches.push(switched.elapsed().as_millis());
        }

        let ids = self.ids()?;
        if count == 12 {
            self.controls_visible()?;
            self.chrome("rows.get(2).item.querySelector('.row-actions').click(); true")?;
            wait(
                "clicking beside the row buttons to select its session",
                || Ok(self.act(Action::Status)?.selected == Some(2)),
            )?;
            ensure(
                self.eval(2, script)? == markers[1],
                "row background selection reloaded the session",
            )?;
            self.act(Action::Select { number: 1 })?;
            self.main(|window, _| {
                window.set_size(tauri::LogicalSize::new(850., 620.))?;
                Ok(())
            })?;
            self.controls_visible()?;
            self.main(|window, _| {
                window.set_size(tauri::LogicalSize::new(1180., 800.))?;
                Ok(())
            })?;
            self.controls_visible()?;
            self.main(|window, launcher| {
                use tauri::menu::MenuItemKind;
                let menu = launcher.session_menu(window, 1)?;
                let items = menu.items()?;
                ensure(
                    items.len() == 3,
                    "native context menu has the wrong actions",
                )?;
                for item in items {
                    let MenuItemKind::MenuItem(item) = item else {
                        return Err("unexpected native context menu item".into());
                    };
                    ensure(
                        item.is_enabled()?,
                        "available native menu action was disabled",
                    )?;
                }
                Ok(())
            })?;
            let profile = ids[0];
            let (instance, old_document, url) = self.main(move |_, launcher| {
                let view = &launcher.active().views[&profile];
                Ok((
                    view.instance,
                    view.document,
                    launcher.selected_browser()?.url()?,
                ))
            })?;
            let demo_path = url.path().to_string();
            self.act(Action::Reload)?;
            self.ready(count as usize, &demo_path)?;
            self.main(move |_, launcher| {
                ensure(
                    launcher.active().views[&profile].document != old_document,
                    "Reload did not advance the sampled document",
                )?;
                launcher.events.send((
                    instance,
                    browser::Event::Sample(
                        old_document,
                        serde_json::json!({
                            "url": url,
                            "text": "Members ahead of you: 999999999",
                            "errorId": "",
                            "challengeVisible": false,
                        })
                        .to_string(),
                    ),
                ))?;
                launcher.update();
                ensure(
                    launcher.active().views[&profile]
                        .status
                        .as_ref()
                        .and_then(|status| status.ahead)
                        != Some(999999999),
                    "same-URL reload accepted a stale page sample",
                )
            })?;
            self.act(Action::Sort { enabled: true })?;
            wait("fresh Demo queue positions after same-URL reload", || {
                let snapshot = self.act(Action::Status)?;
                let positions = snapshot
                    .sessions
                    .iter()
                    .map(|row| row.status.as_ref().and_then(|status| status.ahead))
                    .collect::<Option<Vec<_>>>();
                let Some(positions) = positions else {
                    return Ok(false);
                };
                if positions.len() != 12 {
                    return Ok(false);
                }
                ensure(
                    positions.windows(2).all(|pair| pair[0] <= pair[1]),
                    "queue position sort is out of order",
                )?;
                Ok(true)
            })?;
            ensure(
                self.store_exists(ids[0])?,
                "Demo store was absent before close",
            )?;
            let closing = ids[0];
            self.main(move |window, launcher| {
                launcher.menu_action(window, &format!("session/{closing}/copy_close"))
            })?;
            self.wait_for_cleanup()?;
            let snapshot = self.act(Action::Status)?;
            let link = snapshot
                .history
                .iter()
                .find(|link| link.session == 1)
                .ok_or("saved link missing")?;
            let copied = arboard::Clipboard::new()?.get_text()?;
            ensure(
                snapshot.sessions.len() == 11
                    && snapshot.history.len() == 12
                    && copied == link.url.as_str()
                    && !self.store_exists(ids[0])?
                    && self.eval(2, script)? == markers[1],
                "Demo copy-and-close did not preserve the survivor, clipboard, or saved link",
            )?;
            self.act(Action::End)?;
            self.wait_for_cleanup()?;
            ensure(
                self.act(Action::Status)?.history.len() == 12,
                "End lost link history",
            )?;
            let live = self.act(Action::ToggleDemo)?;
            ensure(
                !live.demo
                    && live.sessions.len() == 2
                    && live.selected == Some(2)
                    && live.sessions[1].ready,
                "Demo changed the live batch",
            )?;
        } else {
            self.act(Action::End)?;
            self.wait_for_cleanup()?;
        }
        for id in ids {
            ensure(
                !self.store_exists(id)?,
                "End batch retained a WebKit data store",
            )?;
        }
        println!("PASS {count} {} sessions: created {} ms; all fixtures ready by {} ms; switch + JS {:?} ms; End removed all stores",
            if count == 12 { "Demo" } else { "live fixture" }, created.as_millis(), ready.as_millis(), switches);
        if count == 12 {
            println!("PASS row controls, native context menu actions, stale sample rejection, queue observation and sorting, Demo copy-and-close, history and clipboard, and Live/Demo isolation");
        }
        Ok(())
    }
}

fn wait(message: &str, mut completed: impl FnMut() -> Result<bool>) -> Result<()> {
    let start = Instant::now();
    while !completed()? {
        ensure(
            start.elapsed() < TIMEOUT,
            &format!("timed out waiting for {message}"),
        )?;
        thread::sleep(Duration::from_millis(30));
    }
    Ok(())
}

fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}
