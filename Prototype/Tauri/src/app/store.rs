use crate::Result;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::PathBuf,
};
use tauri::Url;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub batch: Option<Batch>,
    #[serde(default)]
    pub pending_cleanup: Vec<Uuid>,
    #[serde(default)]
    pub sort: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub url: Url,
    pub log_id: Uuid,
    pub next_number: u32,
    pub selected: Option<u32>,
    pub sessions: Vec<Session>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub profile: Uuid,
    pub number: u32,
    pub ready: bool,
    pub waiting_room_url: Option<Url>,
}

impl Batch {
    pub fn new(url: Url, count: u32) -> Self {
        Self {
            url,
            log_id: Uuid::new_v4(),
            next_number: count + 1,
            selected: Some(1),
            sessions: (1..=count).map(Session::new).collect(),
        }
    }

    pub fn add(&mut self) -> Result<Session> {
        let number = self.next_number;
        self.next_number = number
            .checked_add(1)
            .ok_or("Session numbering limit reached.")?;
        let session = Session::new(number);
        self.selected = Some(number);
        self.sessions.push(session.clone());
        Ok(session)
    }
}

impl Session {
    fn new(number: u32) -> Self {
        Self {
            profile: Uuid::new_v4(),
            number,
            ready: false,
            waiting_room_url: None,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Link {
    pub batch: Uuid,
    pub session: u32,
    pub url: Url,
    pub seen: String,
    pub ahead: Option<i32>,
}

impl Link {
    pub fn new(batch: Uuid, session: u32, url: Url, ahead: Option<i32>) -> Self {
        Self {
            batch,
            session,
            url,
            seen: timestamp(),
            ahead,
        }
    }
}

pub struct Store {
    pub root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn read_json<T: Default + DeserializeOwned>(&self, name: &str) -> Result<T> {
        match fs::read(self.root.join(name)) {
            Ok(contents) => Ok(serde_json::from_slice(&contents)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
            Err(error) => Err(error.into()),
        }
    }

    fn write_json(&self, name: &str, value: &(impl Serialize + ?Sized)) -> Result<()> {
        let destination = self.root.join(name);
        let temporary = destination.with_extension("tmp");
        let mut file = BufWriter::new(fs::File::create(&temporary)?);
        serde_json::to_writer(&mut file, value)?;
        file.flush()?;
        file.get_ref().sync_all()?;
        drop(file);
        fs::rename(temporary, destination)?;
        Ok(())
    }

    pub fn read(&self) -> Result<Saved> {
        self.read_json("active-batch.json")
    }
    pub fn save(&self, saved: &Saved) -> Result<()> {
        self.write_json("active-batch.json", saved)
    }
    pub fn links(&self) -> Result<Vec<Link>> {
        self.read_json("queue-links.json")
    }
    pub fn write_links(&self, links: &[Link]) -> Result<()> {
        self.write_json("queue-links.json", links)
    }
    pub fn log_dir(&self) -> PathBuf {
        self.root.join("Drop Logs")
    }

    pub fn log(
        &self,
        batch: Uuid,
        event: &str,
        session: Option<u32>,
        code: Option<i32>,
        ahead: Option<i32>,
    ) -> Result<()> {
        let directory = self.log_dir();
        fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{batch}.jsonl"));
        let mut entry = serde_json::to_vec(&serde_json::json!({
            "timestamp": timestamp(), "event": event, "session": session, "code": code, "membersAhead": ahead,
        }))?;
        entry.push(b'\n');
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(&entry)?;
        Ok(())
    }
}

fn timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("a current UTC timestamp fits RFC3339")
}

pub fn position_key(number: u32, ahead: Option<i32>) -> (bool, Option<i32>, u32) {
    (ahead.is_none(), ahead, number)
}

pub fn remaining_selection(ordered: &[u32], selected: Option<u32>, removed: &[u32]) -> Option<u32> {
    let Some(index) = selected.and_then(|number| ordered.iter().position(|row| *row == number))
    else {
        return ordered
            .iter()
            .copied()
            .find(|number| !removed.contains(number));
    };
    if !removed.contains(&ordered[index]) {
        return selected;
    }
    ordered[index + 1..]
        .iter()
        .chain(ordered[..index].iter().rev())
        .copied()
        .find(|number| !removed.contains(number))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_and_adding_assign_stable_numbers_and_profiles() {
        let mut batch = Batch::new("https://example.test/".parse().unwrap(), 2);
        let original = batch.sessions[0].profile;
        batch.add().unwrap();
        assert_eq!(batch.selected, Some(3));
        assert_eq!(batch.sessions[0].profile, original);
        assert_ne!(batch.sessions[1].profile, batch.sessions[2].profile);
        batch.sessions.remove(1);
        batch.add().unwrap();
        assert_eq!(
            batch
                .sessions
                .iter()
                .map(|item| item.number)
                .collect::<Vec<_>>(),
            [1, 3, 4]
        );
    }

    #[test]
    fn saving_batch_and_cleanup_is_atomic_and_history_is_separate() {
        let dir = std::env::temp_dir().join(format!("queue-launcher-test-{}", Uuid::new_v4()));
        let files = Store::new(dir.clone()).unwrap();
        let batch = Batch::new("https://example.test/".parse().unwrap(), 1);
        let id = batch.sessions[0].profile;
        let link = Link::new(
            batch.log_id,
            1,
            "https://example.test/sample".parse().unwrap(),
            Some(8),
        );
        files.write_links(&[link]).unwrap();
        files
            .save(&Saved {
                batch: Some(batch),
                ..Default::default()
            })
            .unwrap();
        files
            .save(&Saved {
                pending_cleanup: vec![id],
                ..Default::default()
            })
            .unwrap();
        let loaded = files.read().unwrap();
        assert!(loaded.batch.is_none());
        assert_eq!(loaded.pending_cleanup, [id]);
        assert_eq!(files.links().unwrap().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn position_order_and_close_selection_follow_the_display_order() {
        let mut rows = [(1, None), (2, Some(8)), (3, Some(2)), (4, Some(8))];
        rows.sort_by_key(|&(number, ahead)| position_key(number, ahead));
        let ordered = rows.map(|(number, _)| number);
        assert_eq!(ordered, [3, 2, 4, 1]);
        assert_eq!(remaining_selection(&ordered, Some(2), &[2, 4]), Some(1));
        assert_eq!(remaining_selection(&ordered, Some(1), &[1]), Some(4));
        assert_eq!(remaining_selection(&ordered, Some(3), &ordered), None);
    }
}
