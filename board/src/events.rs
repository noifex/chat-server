use serde::{Serialize, Deserialize};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u16 = 1;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Kind { TaskAdded, Claimed, Reclaimed, Working, Review, Approve, ChangesRequested }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum State { Proposed, Claimed, Working, Review, Done }

#[derive(Serialize, Deserialize, PartialEq)]
pub struct Event {
    schema_version: u16,
    pub seq: u64,
    pub ts: u64,
    pub kind: Kind,
    pub task_id: u64,
    pub by: String,
    pub fencing_token: Option<u64>,
    pub expected_state: Option<State>,
    pub desc: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct Task {
    pub id: u64,
    pub state: State,
    pub desc: String,
    pub owner: Option<String>,
    pub active_fencing_token: Option<u64>,
    pub claimed_at: Option<u64>,
}

impl Event {
    pub fn new(seq: u64, kind: Kind, task_id: u64, by: String) -> Self {
        Event {
            schema_version: SCHEMA_VERSION,
            seq,
            ts: now_ms(),
            kind,
            task_id,
            by,
            fencing_token: None,
            expected_state: None,
            desc: None,
        }
    }

    pub fn with_fencing(mut self, token: Option<u64>) -> Self {
        self.fencing_token = token;
        self
    }

    pub fn expected(mut self, state: State) -> Self {
        self.expected_state = Some(state);
        self
    }

    pub fn desc(mut self, d: String) -> Self {
        self.desc = Some(d);
        self
    }
}
