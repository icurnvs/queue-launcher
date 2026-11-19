use super::{Action, Launcher, DEMO, LIVE};
use crate::{on_main, Result, Window};
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Url};

pub fn run(app: AppHandle) {
    thread::spawn(move || {
        let result = check(&app);
        if let Err(error) = result {
            eprintln!("FAIL: {error}");
            app.exit(1);
        } else {
            app.exit(0);
        }
    });
}

fn invoke<T: Send + 'static>(
    app: &AppHandle,
    work: impl FnOnce(&Window, &mut Launcher) -> Result<T> + Send + 'static,
) -> Result<T> {
    Ok(on_main(app, move |window, launcher| {
        launcher.update();
        work(window, launcher)
    })?)
}

fn check(app: &AppHandle) -> Result<()> {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../Scripts/local-waiting-room.html");
    let url = Url::from_file_path(fixture)
        .map_err(|_| "fixture path is invalid")?
        .to_string();
    invoke(app, move |window, launcher| {
        launcher.act(window, Action::Open { url, count: 2 })?;
        launcher.act(window, Action::Mark)?;
        launcher.act(window, Action::Add)?;
        let live = launcher.snapshot();
        ensure(
            live.sessions.len() == 3 && live.sessions[0].ready && live.selected == Some(3),
            "live add/mark did not preserve the batch",
        )?;
        launcher.act(window, Action::ToggleDemo)
    })?;

    let start = Instant::now();
    loop {
        let count = invoke(app, |_, launcher| {
            let mode = launcher.active();
            Ok(mode.links.len())
        })?;
        if count == 12 {
            break;
        }
        ensure(
            start.elapsed() < Duration::from_secs(25),
            "Demo did not collect twelve sample links",
        )?;
        thread::sleep(Duration::from_millis(300));
    }

    invoke(app, |window, launcher| {
        let mode = &launcher.modes[DEMO];
        let batch = mode.batch()?;
        let first = batch.sessions[0].profile;
        let surviving = batch.sessions[1].profile;
        let survivor_status = mode.views[&surviving].status.clone();
        let history_count = mode.links.len();
        let profile_path = crate::named_profiles::profile_path(first)?;
        ensure(
            profile_path.is_dir(),
            "selected Demo profile has no on-disk directory",
        )?;
        std::fs::write(
            mode.store.root.join("closed-profile-path.txt"),
            &profile_path.to_string_lossy().as_bytes(),
        )?;
        launcher.act(window, Action::Sort { enabled: true })?;
        launcher.act(window, Action::Close { number: 1 })?;
        let mode = &launcher.modes[DEMO];
        ensure(
            mode.saved.pending_cleanup.is_empty(),
            "profile deletion did not complete",
        )?;
        ensure(
            mode.batch()?.sessions.len() == 11
                && !mode.views.contains_key(&first)
                && mode.views[&surviving].status == survivor_status
                && mode.links.len() == history_count,
            "selective close changed another session or lost history",
        )?;
        launcher.act(window, Action::End)?;
        let mode = &launcher.modes[DEMO];
        ensure(
            mode.saved.batch.is_none()
                && mode.saved.pending_cleanup.is_empty()
                && mode.links.len() == history_count,
            "ending the Demo did not cleanly close all sessions",
        )?;
        launcher.act(window, Action::ToggleDemo)?;
        ensure(
            launcher.current == LIVE
                && launcher.snapshot().sessions.len() == 3
                && launcher.snapshot().sessions[0].ready,
            "ending Demo changed the live batch",
        )?;
        let live = &launcher.modes[LIVE];
        let log = std::fs::read_to_string(
            live.store
                .log_dir()
                .join(format!("{}.jsonl", live.batch()?.log_id)),
        )?;
        ensure(
            log.contains("marked_ready")
                && !log.contains("file:")
                && !log.contains("local-waiting-room.html"),
            "drop log was missing its event or exposed the fixture URL",
        )?;
        println!("PASS live batch actions, Demo isolation, history capture, selected cleanup, End batch, and drop log redaction");
        Ok(())
    })?;
    check_navigation(app)
}

fn check_navigation(app: &AppHandle) -> Result<()> {
    let server = tiny_http::Server::http(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
    let origin = format!("http://{}", server.server_addr());
    thread::spawn(move || {
        for request in server.incoming_requests() {
            thread::spawn(move || {
                if request.url() == "/start" {
                    let _ =
                        request.respond(tiny_http::Response::empty(302).with_header(
                            tiny_http::Header::from_bytes("Location", "/first").unwrap(),
                        ));
                } else {
                    if request.url() == "/slow" {
                        thread::sleep(Duration::from_secs(10));
                    }
                    let _ = request.respond(tiny_http::Response::from_string("Navigation fixture"));
                }
            });
        }
    });
    let start = format!("{origin}/start");
    invoke(app, move |window, launcher| {
        launcher.act(window, Action::End)?;
        launcher.act(
            window,
            Action::Open {
                url: start,
                count: 2,
            },
        )
    })?;
    wait_page(app, "/first", false)?;
    let second = format!("{origin}/second").parse()?;
    invoke(app, move |_, launcher| {
        launcher.selected_browser()?.navigate(second)
    })?;
    wait_page(app, "/second", false)?;
    invoke(app, |window, launcher| launcher.act(window, Action::Back))?;
    wait_page(app, "/first", false)?;
    invoke(app, |_, launcher| {
        let snapshot = launcher.snapshot();
        ensure(
            !snapshot.sessions[0].can_go_back && snapshot.sessions[0].can_go_forward,
            "Back did not enable Forward at the first page",
        )
    })?;
    invoke(app, |window, launcher| {
        launcher.act(window, Action::Forward)
    })?;
    wait_page(app, "/second", false)?;
    invoke(app, |window, launcher| {
        launcher.act(window, Action::Select { number: 2 })
    })?;
    wait_page(app, "/first", false)?;
    invoke(app, |window, launcher| {
        ensure(
            !launcher.snapshot().sessions[1].can_go_forward,
            "Forward state leaked between sessions",
        )?;
        launcher.act(window, Action::Select { number: 1 })
    })?;
    let slow = format!("{origin}/slow").parse()?;
    invoke(app, move |_, launcher| {
        launcher.selected_browser()?.navigate(slow)
    })?;
    wait_page(app, "", true)?;
    invoke(app, |window, launcher| launcher.act(window, Action::Stop))?;
    wait_page(app, "", false)?;
    let third = format!("{origin}/third").parse()?;
    invoke(app, move |_, launcher| {
        launcher.selected_browser()?.navigate(third)
    })?;
    wait_page(app, "/third", false)?;
    invoke(app, |window, launcher| launcher.act(window, Action::End))?;
    println!("PASS redirected URL, per-session Back/Forward history, loading state, and Stop without a page error");
    Ok(())
}

fn wait_page(app: &AppHandle, path: &'static str, loading: bool) -> Result<()> {
    let start = Instant::now();
    loop {
        let ready = invoke(app, move |_, launcher| {
            let snapshot = launcher.snapshot();
            Ok(snapshot.sessions.iter().any(|row| {
                Some(row.number) == snapshot.selected
                    && row.error.is_none()
                    && row.loading == loading
                    && snapshot.current_url.ends_with(path)
            }))
        })?;
        if ready {
            return Ok(());
        }
        if start.elapsed() >= Duration::from_secs(15) {
            let details = invoke(app, |_, launcher| {
                let snapshot = launcher.snapshot();
                let row = snapshot
                    .sessions
                    .iter()
                    .find(|row| Some(row.number) == snapshot.selected);
                Ok(format!(
                    "url={}, loading={:?}, error={:?}",
                    snapshot.current_url,
                    row.map(|row| row.loading),
                    row.and_then(|row| row.error.as_deref())
                ))
            })?;
            return Err(format!(
                "Navigation did not settle at path={path}, loading={loading}: {details}"
            )
            .into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}
