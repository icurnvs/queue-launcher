use percent_encoding::percent_decode_str;
use regex::Regex;
use serde::Serialize;
use std::{borrow::Cow, sync::LazyLock};
use tauri::Url;
use uuid::Uuid;

static NO_ATTEMPTS: LazyLock<Regex> = LazyLock::new(|| regex(r"no more attempts"));
static EVENT_ENDED: LazyLock<Regex> =
    LazyLock::new(|| regex(r"the event has ended|this product has sold out"));
static SESSION_EXPIRED: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(your|this) (queue session|session|queue number|turn) has expired"));
static QUEUE_ID: LazyLock<Regex> = LazyLock::new(|| regex(r"queue id\s*:\s*([0-9a-f-]{36})\b"));
static AHEAD: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"(?:members ahead of you|number of users in line ahead of you)\s*:\s*([0-9]+(?:,[0-9]{3})*)\b",
    )
});
static ESTIMATED_WAIT: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"(?:estimated time to enter|your estimated wait time is)\s*:\s*((?:(?:more|less) than |about |over |under )?(?:\d+\s*(?:hours?|minutes?|seconds?)(?:\s+(?:and\s+)?\d+\s*(?:hours?|minutes?|seconds?))*|an?\s+(?:hour|minute|second))|calculating)",
    )
});
static WAIT_DURATION: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\b(\d+|an?)\s+(hours?|minutes?|seconds?)\b"));
static PURCHASE_WINDOW: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(\d+\s+minutes?)\s+to complete your purchase"));
static TIME_REMAINING: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"(?:time remaining|time left)\s*:\s*(\d{1,2}:\d{2}(?::\d{2})?|\d+\s+(?:minutes?|seconds?))",
    )
});
static PRE_QUEUE: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"you[’']re early!?.*the queue for this product will begin soon.*(?:the event will begin at|\d+\s+minutes?\s+\d+\s+seconds?)",
    )
});
static IN_WAITING_ROOM: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"you are (?:now )?in (?:the |a )?(?:waiting room|(?:virtual )?queue)|in the waiting room",
    )
});

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueStatus {
    pub queue_id: Option<Uuid>,
    pub ahead: Option<i32>,
    pub estimated_wait: Option<String>,
    pub estimated_wait_seconds: Option<u64>,
    pub purchase_window: Option<String>,
    pub time_remaining: Option<String>,
    pub message: Option<String>,
    pub error_id: Option<i32>,
    pub in_waiting_room: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueObservation {
    pub event: &'static str,
    pub code: Option<i32>,
    pub members_ahead: Option<i32>,
}

fn wait_seconds(text: &str) -> Option<u64> {
    let lower = text.to_ascii_lowercase();
    let mut total = None;
    for part in WAIT_DURATION.captures_iter(&lower) {
        let amount = match &part[1] {
            "a" | "an" => 1,
            value => value.parse::<u64>().ok()?,
        };
        let unit = match &part[2] {
            "hour" | "hours" => 3600,
            "minute" | "minutes" => 60,
            _ => 1,
        };
        total = Some(
            total
                .unwrap_or(0u64)
                .checked_add(amount.checked_mul(unit)?)?,
        );
    }
    let total = total?;
    if lower.starts_with("more than ") || lower.starts_with("over ") {
        total.checked_add(1)
    } else if lower.starts_with("less than ") || lower.starts_with("under ") {
        Some(total.saturating_sub(1))
    } else {
        Some(total)
    }
}

impl QueueStatus {
    pub fn parse(text: &str, error_id: Option<i32>) -> Self {
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let message = [
            (&*NO_ATTEMPTS, "No more attempts"),
            (&*EVENT_ENDED, "Event ended"),
            (&*SESSION_EXPIRED, "Session expired"),
        ]
        .into_iter()
        .find_map(|(pattern, message)| pattern.is_match(&flat).then(|| message.to_owned()));
        if message.is_some() {
            return Self {
                message,
                error_id,
                ..Self::default()
            };
        }

        let estimated_wait = capture(&ESTIMATED_WAIT, &flat).map(ToOwned::to_owned);
        let estimated_wait_seconds = estimated_wait.as_deref().and_then(wait_seconds);
        Self {
            queue_id: capture(&QUEUE_ID, &flat).and_then(queue_id),
            ahead: capture(&AHEAD, &flat).and_then(|ahead| ahead.replace(',', "").parse().ok()),
            estimated_wait,
            estimated_wait_seconds,
            purchase_window: capture(&PURCHASE_WINDOW, &flat).map(ToOwned::to_owned),
            time_remaining: capture(&TIME_REMAINING, &flat).map(ToOwned::to_owned),
            message: None,
            error_id,
            in_waiting_room: IN_WAITING_ROOM.is_match(&flat) || PRE_QUEUE.is_match(&flat),
        }
    }

    pub fn observation(&self, challenge_visible: bool) -> Option<QueueObservation> {
        let event = match self.message.as_deref() {
            Some("No more attempts") => "no_more_attempts",
            Some("Event ended") => "event_ended",
            Some("Session expired") => "session_expired",
            _ if self.error_id.is_some() => "queue_error",
            _ if self.ahead.is_some() => "queue_position",
            _ if self.in_waiting_room => "waiting_room",
            _ if challenge_visible => "captcha_visible",
            _ => return None,
        };
        Some(QueueObservation {
            event,
            code: self.error_id,
            members_ahead: self.ahead,
        })
    }

    pub fn resume_url(&self, url: &Url) -> Option<Url> {
        if self.message.is_some() || !is_waiting_room(url) {
            return None;
        }
        if parameters(url)
            .any(|parameter| parameter.name() == "q" && queue_id(&parameter.value()).is_some())
        {
            return Some(url.clone());
        }

        let queue_id = format!("q={}", self.queue_id?);
        let query = parameters(url)
            .filter(|parameter| parameter.name() != "q")
            .map(|parameter| parameter.0)
            .chain(std::iter::once(queue_id.as_str()))
            .collect::<Vec<_>>()
            .join("&");
        let mut resume = url.clone();
        resume.set_query(Some(&query));
        Some(resume)
    }

    pub fn share_url(&self, url: &Url) -> Option<Url> {
        if self.error_id.is_some() || (self.ahead.is_none() && !self.in_waiting_room) {
            return None;
        }
        let mut resume = self.resume_url(url)?;
        for name in ["c", "e", "t", "q"] {
            let mut matches = parameters(&resume).filter(|parameter| parameter.name() == name);
            if matches.next()?.value().is_empty() || matches.next().is_some() {
                return None;
            }
        }

        // Preserve escaping in the original target and other share parameters.
        let query = parameters(&resume)
            .filter(|parameter| matches!(parameter.name().as_ref(), "c" | "e" | "t" | "cid" | "q"))
            .map(|parameter| parameter.0)
            .collect::<Vec<_>>()
            .join("&");
        resume.set_username("").ok()?;
        resume.set_password(None).ok()?;
        resume.set_path("/");
        resume.set_fragment(None);
        resume.set_query(Some(&query));
        Some(resume)
    }
}

pub fn is_waiting_room(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some("wr.costco.com")
}

fn regex(pattern: &str) -> Regex {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .expect("valid queue-status pattern")
}

fn capture<'a>(pattern: &Regex, text: &'a str) -> Option<&'a str> {
    Some(pattern.captures(text)?.get(1)?.as_str())
}

fn queue_id(value: &str) -> Option<Uuid> {
    let value = value.trim();
    if value.len() != 36
        || ![8, 13, 18, 23]
            .iter()
            .all(|&index| value.as_bytes()[index] == b'-')
    {
        return None;
    }
    Uuid::parse_str(value).ok()
}

struct Parameter<'a>(&'a str);

impl Parameter<'_> {
    fn name(&self) -> Cow<'_, str> {
        let name = self.0.split_once('=').map_or(self.0, |(name, _)| name);
        percent_decode_str(name).decode_utf8_lossy()
    }

    fn value(&self) -> Cow<'_, str> {
        let value = self.0.split_once('=').map_or("", |(_, value)| value);
        percent_decode_str(value).decode_utf8_lossy()
    }
}

fn parameters(url: &Url) -> impl Iterator<Item = Parameter<'_>> {
    url.query()
        .unwrap_or_default()
        .split('&')
        .filter(|part| !part.is_empty())
        .map(Parameter)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "274eb8c4-773b-47d2-b68a-a430522318b6";
    const TARGET: &str = "https%3A%2F%2Fwww.costco.com%2Fa%3Fx%3D1%252B2";

    fn waiting_room() -> QueueStatus {
        QueueStatus::parse(
            &format!("Queue ID: {ID}\nYou are now in the waiting room\nMembers ahead of you: 1,234\nEstimated time to enter: about 1 hour and 12 minutes\nYou have 10 minutes to complete your purchase\nTime remaining: 09:41"),
            None,
        )
    }

    #[test]
    fn reads_numeric_and_worded_wait_estimates_without_a_member_count() {
        for (text, seconds) in [
            ("22 minutes", Some(1320)),
            ("more than an hour", Some(3601)),
            ("over an hour", Some(3601)),
            ("less than a minute", Some(59)),
            ("under a minute", Some(59)),
            ("about 1 hour and 12 minutes", Some(4320)),
            ("2 hours 3 minutes 4 seconds", Some(7384)),
            ("calculating", None),
        ] {
            let status = QueueStatus::parse(&format!("Estimated time to enter: {text}"), None);
            assert_eq!(status.estimated_wait.as_deref(), Some(text));
            assert_eq!(status.estimated_wait_seconds, seconds, "{text}");
            assert_eq!(status.ahead, None);
        }
        assert_eq!(wait_seconds("99999999999999999999 hours"), None);
    }

    #[test]
    fn recognizes_the_pre_queue_countdown_as_a_waiting_room_state() {
        let text = format!("Queue ID: {ID}\nYou’re early!\nThe queue for this product will begin soon.\nThe event will begin at: 12:32 PM PST\n20 Minutes 34 Seconds");
        let status = QueueStatus::parse(&text, None);
        assert!(status.in_waiting_room);
        assert!(status.queue_id.is_some());
        assert!(QueueStatus::parse(&text.replace('’', "'"), None).in_waiting_room);
        assert!(!QueueStatus::parse("You're early!", None).in_waiting_room);
    }

    #[test]
    fn reads_waiting_room_details() {
        let status = waiting_room();
        assert_eq!(status.queue_id, Uuid::parse_str(ID).ok());
        assert_eq!(status.ahead, Some(1234));
        assert_eq!(
            status.estimated_wait.as_deref(),
            Some("about 1 hour and 12 minutes")
        );
        assert_eq!(status.purchase_window.as_deref(), Some("10 minutes"));
        assert_eq!(status.time_remaining.as_deref(), Some("09:41"));
        assert!(status.in_waiting_room);
    }

    #[test]
    fn resume_appends_queue_id_and_share_strips_private_fields() {
        let url = Url::parse(&format!(
            "https://wr.costco.com/wait?c=costco&e=drop&t={TARGET}&cid=en-US&token=private#fragment"
        ))
        .unwrap();
        let status = waiting_room();
        let resume = status.resume_url(&url).unwrap();
        assert!(resume.query().unwrap().ends_with(&format!("&q={ID}")));
        assert_eq!(
            status.share_url(&url).unwrap().as_str(),
            format!("https://wr.costco.com/?c=costco&e=drop&t={TARGET}&cid=en-US&q={ID}")
        );
    }

    #[test]
    fn accepts_encoded_parameter_names_without_reencoding_query() {
        let url = Url::parse(&format!(
            "https://wr.costco.com/w?%63=costco&e=drop&t=a+b&%71={ID}"
        ))
        .unwrap();
        let status = waiting_room();
        assert_eq!(status.resume_url(&url), Some(url.clone()));
        assert_eq!(
            status.share_url(&url).unwrap().as_str(),
            format!("https://wr.costco.com/?%63=costco&e=drop&t=a+b&%71={ID}")
        );
    }

    #[test]
    fn filters_other_origins_and_rejects_ambiguous_or_incomplete_links() {
        let status = waiting_room();
        for url in [
            format!("https://other.example/?q={ID}"),
            format!("http://wr.costco.com/?q={ID}"),
            format!("https://wr.costco.com.other.example/?q={ID}"),
        ] {
            assert!(status.resume_url(&Url::parse(&url).unwrap()).is_none());
        }
        for query in ["c=costco&e=drop&e=duplicate&t=target", "c=costco&e=drop&t="] {
            let url = Url::parse(&format!("https://wr.costco.com/?{query}")).unwrap();
            assert!(status.share_url(&url).is_none());
        }
    }

    #[test]
    fn requires_waiting_room_evidence_to_share_and_ignores_invalid_queue_ids() {
        let id_without_hyphens = ID.replace('-', "");
        let url = Url::parse(&format!(
            "https://wr.costco.com/?c=costco&e=drop&t=target&q={id_without_hyphens}"
        ))
        .unwrap();
        let status = QueueStatus::parse(&format!("Queue ID: {ID}"), None);
        assert_eq!(
            status.resume_url(&url).unwrap().query(),
            Some(format!("c=costco&e=drop&t=target&q={ID}").as_str())
        );
        assert!(status.share_url(&url).is_none());
    }

    #[test]
    fn shares_confirmed_waiting_room_without_a_member_count() {
        let url = Url::parse(&format!(
            "https://wr.costco.com/?c=costco&e=drop&t={TARGET}&token=private"
        ))
        .unwrap();
        let status = QueueStatus::parse(&format!("Queue ID: {ID} Welcome! You Are Now in The Virtual Queue. Estimated time to enter: 22 minutes"), None);
        assert_eq!(status.ahead, None);
        let shared = status
            .share_url(&url)
            .expect("Confirmed queue must be shareable without members ahead");
        assert_eq!(
            shared.query(),
            Some(format!("c=costco&e=drop&t={TARGET}&q={ID}").as_str())
        );
        assert!(
            QueueStatus::parse(&format!("Queue ID: {ID} You are now in the queue"), Some(7))
                .share_url(&url)
                .is_none()
        );
        assert!(
            QueueStatus::parse("Queue ID: invalid You are now in the queue", None)
                .share_url(&url)
                .is_none()
        );
    }

    #[test]
    fn prioritizes_queue_errors_and_records_visible_manual_challenge() {
        let error = QueueStatus::parse("No more attempts left while in the waiting room", Some(42));
        assert_eq!(
            error.observation(true),
            Some(QueueObservation {
                event: "no_more_attempts",
                code: Some(42),
                members_ahead: None
            })
        );
        assert_eq!(
            QueueStatus::parse("Waiting for verification", None)
                .observation(true)
                .unwrap()
                .event,
            "captcha_visible"
        );
    }
}
