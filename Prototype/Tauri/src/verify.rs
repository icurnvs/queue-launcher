use crate::{fixture::Report, on_main, Result, SAMPLE_LINK};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::mpsc::Receiver,
    thread,
    time::{Duration, Instant},
};
use tauri::AppHandle;
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_secs(90);

struct Check {
    app: AppHandle,
    reports: Receiver<Report>,
}

impl Check {
    fn ids(&self) -> Result<Vec<Uuid>> {
        Ok(on_main(&self.app, |_, launcher| {
            Ok(launcher.views.iter().map(|(id, _)| *id).collect())
        })?)
    }

    fn add(&self, count: usize) -> Result<()> {
        Ok(on_main(&self.app, move |window, launcher| {
            launcher.add(window, count)
        })?)
    }

    fn close(&self, id: Uuid) -> Result<()> {
        on_main(&self.app, move |window, launcher| {
            launcher.close(window, id)
        })?;
        let start = Instant::now();
        loop {
            if on_main(&self.app, move |_, launcher| {
                if let Some(error) = &launcher.cleanup_error {
                    return Err(error.clone().into());
                }
                Ok(!launcher.saved.pending_removal.contains(&id))
            })? {
                return Ok(());
            }
            ensure(start.elapsed() < TIMEOUT, "store cleanup timed out")?;
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn select(&self, id: Uuid) -> Result<()> {
        Ok(on_main(&self.app, move |_, launcher| launcher.select(id))?)
    }

    fn report_after(&self, id: Uuid, javascript: &'static str) -> Result<Report> {
        on_main(&self.app, move |_, launcher| {
            let (_, view) = launcher
                .views
                .iter()
                .find(|(other, _)| *other == id)
                .ok_or("missing view")?;
            view.eval(javascript)?;
            Ok(())
        })?;
        self.collect(&[id])?
            .remove(&id)
            .ok_or("missing fixture report".into())
    }

    fn collect(&self, ids: &[Uuid]) -> Result<HashMap<Uuid, Report>> {
        let start = Instant::now();
        let waiting: HashSet<_> = ids.iter().copied().collect();
        let mut reports = HashMap::new();
        while reports.len() < waiting.len() {
            let time_left = TIMEOUT.saturating_sub(start.elapsed());
            let report = self.reports.recv_timeout(time_left).map_err(|_| {
                format!(
                    "readiness timed out after {}/{} fixture reports",
                    reports.len(),
                    waiting.len()
                )
            })?;
            if waiting.contains(&report.session) {
                ensure(
                    report.bridge_denied,
                    "fixture could invoke the launcher command",
                )?;
                reports.insert(report.session, report);
            }
        }
        Ok(reports)
    }

    fn initial(&self) -> Result<()> {
        ensure(
            self.ids()?.is_empty(),
            "use a fresh scratch data directory for initial verification",
        )?;
        self.add(2)?;
        let ids = self.ids()?;
        let reports = self.collect(&ids)?;
        ensure_isolated(&reports)?;
        ensure(
            reports.values().all(|r| !r.had_cookie && !r.had_storage),
            "new stores contained old state",
        )?;
        println!("PASS two same-origin views have isolated cookies and local storage; launcher command denied");

        self.select(ids[0])?;
        let change = self.report_after(
            ids[0],
            "document.getElementById('increment').click(); window.fixtureReport()",
        )?;
        ensure(
            change.counter == 1,
            "increment did not update in-memory state",
        )?;
        self.select(ids[1])?;
        self.select(ids[0])?;
        let switched = self.report_after(ids[0], "window.fixtureReport()")?;
        ensure(
            switched.counter == 1 && switched.page == reports[&ids[0]].page,
            "switch changed the in-memory page",
        )?;
        println!("PASS switching preserves the page-load marker and in-memory counter");

        on_main(&self.app, move |_, launcher| {
            launcher.save_link()?;
            let observations = ids
                .iter()
                .map(|id| (*id, (reports[id].storage, reports[id].cookie)))
                .collect::<HashMap<_, _>>();
            crate::write_json(&launcher.root.join("verification.json"), &observations)?;
            Ok(())
        })?;
        Ok(())
    }

    fn reopen(&self) -> Result<()> {
        let ids = self.ids()?;
        ensure(
            ids.len() == 2,
            "reopen verification requires two saved sessions",
        )?;
        let old = on_main(&self.app, |_, launcher| {
            ensure(
                launcher.links.iter().any(|link| link == SAMPLE_LINK),
                "sample link did not survive relaunch",
            )?;
            read_observations(&launcher.root)
        })?;
        let restored = self.collect(&ids)?;
        for id in &ids {
            let report = &restored[id];
            ensure(
                old.get(id) == Some(&(report.storage, report.cookie))
                    && report.had_storage
                    && report.had_cookie,
                "saved UUID did not reopen its cookie and local storage",
            )?;
        }
        println!("PASS relaunch restores both UUID-backed stores and the saved link");

        #[cfg(target_os = "windows")]
        let deleted = ids[0];
        #[cfg(target_os = "windows")]
        let named = on_main(&self.app, move |_, launcher| {
            if launcher.named_profiles {
                let path = crate::named_profiles::profile_path(deleted)?;
                ensure(
                    path.is_dir(),
                    "named profile directory did not exist before close",
                )?;
                crate::write_json(
                    &launcher.root.join("deleted-profile.json"),
                    &(deleted, path),
                )?;
            }
            Ok(launcher.named_profiles)
        })?;
        self.close(ids[0])?;
        let remaining = self.report_after(ids[1], "window.fixtureReport()")?;
        ensure(
            old.get(&ids[1]) == Some(&(remaining.storage, remaining.cookie))
                && remaining.page == restored[&ids[1]].page,
            "closing the first session changed the second",
        )?;
        #[cfg(target_os = "windows")]
        if named {
            println!("PASS close clears and marks the named profile for deletion; the other page and link survive");
            return Ok(());
        }
        self.probe_deleted(ids[0])?;
        println!("PASS closing removes the selected store; the other page and link survive");
        Ok(())
    }

    fn probe_deleted(&self, deleted: Uuid) -> Result<()> {
        on_main(&self.app, move |window, launcher| {
            let view = launcher.create_view(window, deleted)?;
            // Track this probe separately so normal close can journal its removal too.
            launcher.views.push((deleted, view));
            Ok(())
        })?;
        self.check_clean_store(deleted)
    }

    fn check_clean_store(&self, deleted: Uuid) -> Result<()> {
        let fresh = self.collect(&[deleted])?.remove(&deleted).unwrap();
        ensure(
            !fresh.had_cookie && !fresh.had_storage,
            "deleted store still has browser data",
        )?;
        self.close(deleted)?;
        on_main(&self.app, |_, launcher| {
            ensure(
                launcher.links.iter().any(|link| link == SAMPLE_LINK),
                "closing lost the saved sample link",
            )
        })?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn verify_named_deletion(&self) -> Result<()> {
        let (deleted, path) = on_main(&self.app, |_, launcher| {
            ensure(
                launcher.named_profiles,
                "named profile verification requires named mode",
            )?;
            crate::read_json::<Option<(Uuid, std::path::PathBuf)>>(
                &launcher.root.join("deleted-profile.json"),
            )?
            .ok_or("missing deleted profile observation".into())
        })?;
        let start = Instant::now();
        while path.try_exists()? {
            ensure(
                start.elapsed() < Duration::from_secs(10),
                "named profile directory still exists after relaunch",
            )?;
            thread::sleep(Duration::from_millis(100));
        }
        let remaining = self.ids()?;
        ensure(
            remaining.len() == 1,
            "relaunch should restore only the surviving session",
        )?;
        let reports = self.collect(&remaining)?;
        let old = on_main(&self.app, |_, launcher| read_observations(&launcher.root))?;
        let survivor = &reports[&remaining[0]];
        ensure(
            old.get(&remaining[0]) == Some(&(survivor.storage, survivor.cookie)),
            "surviving named profile changed after relaunch",
        )?;
        let rejected = on_main(&self.app, move |window, launcher| {
            match launcher.create_view(window, deleted) {
                Ok(view) => {
                    launcher.views.push((deleted, view));
                    Ok(false)
                }
                Err(error) if crate::named_profiles::deletion_pending(error.as_ref()) => Ok(true),
                Err(error) => Err(error),
            }
        })?;
        println!("PASS the named profile directory is gone after browser exit and the surviving profile persisted");
        let probe = if rejected {
            println!("OBSERVED WebView2 still rejects reuse of the deleted name with ERROR_DELETE_PENDING");
            self.add(1)?;
            *self.ids()?.last().ok_or("missing fresh profile")?
        } else {
            deleted
        };
        self.check_clean_store(probe)?;
        Ok(())
    }

    fn scale(&self, total: usize, start: Instant) -> Result<()> {
        ensure(
            self.ids()?.is_empty(),
            "scale verification requires a fresh scratch directory",
        )?;
        self.add(total)?;
        let created = start.elapsed();
        let ids = self.ids()?;
        let reports = self.collect(&ids)?;
        ensure_isolated(&reports)?;
        let ready = start.elapsed();
        let mut switches = Vec::new();
        for id in [ids[total - 1], ids[total / 2], ids[0]] {
            let switching = Instant::now();
            self.select(id)?;
            let report = self.report_after(id, "window.fixtureReport()")?;
            ensure(
                report.page == reports[&id].page,
                "scale switch reloaded the page",
            )?;
            switches.push(switching.elapsed().as_millis());
        }
        println!(
            "PASS {total} views: {} ms to create, {} ms to ready, switch + JS round trip {:?} ms",
            created.as_millis(),
            ready.as_millis(),
            switches
        );
        println!("SNAPSHOT {total}");
        thread::sleep(Duration::from_secs(6));
        Ok(())
    }
}

fn ensure(condition: bool, message: &str) -> Result<()> {
    if !condition {
        return Err(message.into());
    }
    Ok(())
}

fn ensure_isolated(reports: &HashMap<Uuid, Report>) -> Result<()> {
    let storage: HashSet<_> = reports.values().map(|r| r.storage).collect();
    let cookies: HashSet<_> = reports.values().map(|r| r.cookie).collect();
    ensure(storage.len() == reports.len(), "local storage was shared")?;
    ensure(cookies.len() == reports.len(), "cookies were shared")
}

fn read_observations(root: &Path) -> Result<HashMap<Uuid, (Uuid, Uuid)>> {
    crate::read_json(&root.join("verification.json"))
}

pub fn run(app: AppHandle, reports: Receiver<Report>, mode: String, start: Instant) {
    thread::spawn(move || {
        let check = Check { app, reports };
        let result = match mode.as_str() {
            "--verify-initial" => check.initial(),
            "--verify-reopen" => check.reopen(),
            #[cfg(target_os = "windows")]
            "--verify-named-deletion" => check.verify_named_deletion(),
            "--verify-12" => check.scale(12, start),
            "--verify-50" => check.scale(50, start),
            _ => Err("unknown verification mode".into()),
        };
        if let Err(error) = result {
            eprintln!("FAIL: {error}");
            check.app.exit(1);
        } else {
            check.app.exit(0);
        }
    });
}
