use std::env;
use std::path::PathBuf;

mod events;
mod git;
mod reducer;
mod wal;
use events::{Event, Kind, Task};
use wal::{AppendOutcome, Recover, Wal};
mod error;
use error::{BoardError, BoardResult};
const WAL: &str = "board.wal";

fn main() {
    let code = match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            error.exit_code()
        }
    };
    std::process::exit(code);
}

fn run() -> BoardResult<()> {
    let args: Vec<String> = env::args().collect();
    let wal_path = env::var("BOARD_WAL").unwrap_or_else(|_| WAL.to_string());
    let mut wal = Wal::open(PathBuf::from(wal_path));

    match args.get(1).map(String::as_str) {
        Some("add")     => cmd_add(&mut wal, args.get(2).cloned().unwrap_or_default()),
        Some("claim")   => mutate(&mut wal, Kind::Claimed, &args),
        Some("reclaim") => mutate(&mut wal, Kind::Reclaimed, &args),
        Some("start")   => mutate(&mut wal, Kind::Working, &args),
        Some("review")  => mutate(&mut wal, Kind::Review, &args),
        Some("approve") => mutate(&mut wal, Kind::Approve, &args),
        Some("changes") => mutate(&mut wal, Kind::ChangesRequested, &args),
        Some("recover") => cmd_recover(&mut wal),
        Some("project") => cmd_project(&mut wal),
        Some("compensate")=> mutate(&mut wal, Kind::Compensate, &args),
        Some("revert")=>cmd_revert(&mut wal, &args),
        _ => Err(BoardError::Usage(
                "board <add|claim|reclaim|start|review|approve|changes|recover|project|compensate|revert>".into(),
            )),
        }
}
fn cmd_add(wal: &mut Wal, desc: String) -> BoardResult<()> {
    with_lock(wal, |w| {
        if let Recover::Poison(p) = w.recover() {
            return Err(BoardError::WalPoison(p.reason));
        }
        let (tasks, max_seq) = w.load().map_err(|p| BoardError::WalPoison(p.reason))?;
        let max_id = tasks.keys().next_back().copied().unwrap_or(0);
        let id = next_u64(max_id, "task_id")?;
        let seq = next_u64(max_seq, "sequence")?;
        let ev = Event::new(seq, Kind::TaskAdded, id, "cli".into())?.desc(desc);
        match w.append(&ev) {
            AppendOutcome::Committed(_) => {
                println!("{id}");
                Ok(())
            }
            AppendOutcome::InDoubt(e) => Err(BoardError::WalInDoubt(e)),
        }
    })
}

/// 全書き込みの心臓。lock 区間の中で recover → load → guard 試し打ち → append。
fn mutate(wal: &mut Wal, kind: Kind, args: &[String]) -> BoardResult<()> {
    let id: u64 = match args.get(2).and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => {
            return Err(BoardError::Usage("bad or missing <id>".into()));
        }
    };
    let peer = match args.get(3) {
        Some(p) => p.clone(),
        None => {
            return Err(BoardError::Usage("missing <peer>".into()));
        }
    };
    let is_grant = matches!(kind, Kind::Claimed | Kind::Reclaimed | Kind::Compensate);
    let fencing_arg: Option<u64> = args.get(4).and_then(|s| s.parse().ok());

    with_lock(wal, |w| {
        if let Recover::Poison(p) = w.recover() {
            return Err(BoardError::WalPoison(p.reason));
        }
        let (tasks, max_seq) = match w.load() {
            Ok(v) => v,
            Err(p) => {
                return Err(BoardError::WalPoison(p.reason));
            }
        };

        let seq = next_u64(max_seq, "sequence")?;
        let token = if is_grant { Some(seq) } else { fencing_arg };
        let mut ev = Event::new(seq, kind, id, peer)?.with_fencing(token);
        if let Some(t) = tasks.get(&id) {
            ev = ev.expected(t.state);
        }

        // 本番 map に触る前に clone で試し打ち → 弾かれるなら WAL を汚さず終わる
        let mut probe = tasks.clone();
        if let Err(p) = reducer::apply(&mut probe, &ev) {
            return Err(BoardError::Rejected(format!("task {id}: {}", p.reason)));
        }

        // approve ok -> done -> git commit-> sha
        if matches!(kind, Kind::Approve) {
            let ws = env::var("BOARD_WORKSPACE").unwrap_or_else(|_| "workspace".into());
            match git::commit(std::path::Path::new(&ws), id) {
                Ok(sha) => {
                    ev = ev.with_commit_sha(sha);
                }
                Err(e) => {
                    return Err(BoardError::Git(format!(
                        "can't commit task {id} failed: {e}"
                    )));
                }
            }
        }

        match w.append(&ev) {
            AppendOutcome::Committed(s) => {
                if is_grant {
                    println!("{{\"task_id\":{id},\"fencing_token\":{s}}}");
                } else {
                    println!("ok");
                }
                Ok(())
            }
            AppendOutcome::InDoubt(e) => Err(BoardError::WalInDoubt(e)),
        }
    })
}

fn cmd_recover(wal: &mut Wal) -> BoardResult<()> {
    with_lock(wal, |w| match w.recover() {
        Recover::Clean => {
            println!("clean");
            Ok(())
        }
        Recover::TruncatedTail { dropped_bytes } => {
            println!("truncated_tail dropped={dropped_bytes}");
            Ok(())
        }
        Recover::Poison(p) => Err(BoardError::WalPoison(p.reason)),
    })
}

fn cmd_project(wal: &mut Wal) -> BoardResult<()> {
    with_lock(wal, |w| {
        if let Recover::Poison(p) = w.recover() {
            return Err(BoardError::WalPoison(p.reason));
        }
        let (tasks, _) = w.load().map_err(|p| BoardError::WalPoison(p.reason))?;
        let list: Vec<&Task> = tasks.values().collect();
        let json = serde_json::to_string_pretty(&list).map_err(BoardError::Json)?;
        println!("{json}");
        Ok(())
    })
}

/// with_exclusive のラッパ。lock 自体が取れなければ 74。
fn with_lock<T>(
    wal: &mut Wal,
    f: impl FnOnce(&mut wal::Locked) -> BoardResult<T>,
) -> BoardResult<T> {
    wal.with_exclusive(f).map_err(BoardError::Lock)?
}

fn cmd_revert(wal: &mut Wal, args: &[String]) -> BoardResult<()> {
    let task_id: u64 = match args.get(2).and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => {
            return Err(BoardError::Usage("bad or missing <id>".into()));
        }
    };
    let peer = match args.get(3) {
        Some(p) => p.clone(),
        None => return Err(BoardError::Usage("missing <peer>".into())),
    };
    let token: u64 = match args.get(4).and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => {
            return Err(BoardError::Usage("missing <token>".into()));
        }
    };
    let ws = env::var("BOARD_WORKSPACE").unwrap_or_else(|_| "workspace".into());

    with_lock(wal, |w| {
        if let Recover::Poison(p) = w.recover() {
            return Err(BoardError::WalPoison(p.reason));
        }
        let (tasks, max_seq) = w.load().map_err(|p| BoardError::WalPoison(p.reason))?;
        let new_peer = peer.clone();

        let task = tasks
            .get(&task_id)
            .ok_or_else(|| BoardError::Rejected(format!("task {task_id} not found")))?;
        let commit_sha = task
            .commit_sha
            .as_deref()
            .ok_or_else(|| BoardError::Invariant(format!("task {task_id} has no commit_sha")))?;
        let next_seq = next_u64(max_seq, "sequence")?;
        let ev_rolled =
            Event::new(next_seq, Kind::RolledBack, task_id, peer)?.with_fencing(Some(token));
        let ev_needs =
            Event::new(next_seq, Kind::NeedsHuman, task_id, new_peer)?
                .with_fencing(Some(token)); // ev_needs need 'new_peer(peer.clone())' because by:String. moved.

        let mut probe_rolled = tasks.clone();
        let mut probe_needs = tasks.clone();
        if let Err(p) = reducer::apply(&mut probe_rolled, &ev_rolled) {
            return Err(BoardError::Rejected(format!(
                "cannot roll back: {}",
                p.reason
            )));
        }
        if let Err(p) = reducer::apply(&mut probe_needs, &ev_needs) {
            return Err(BoardError::Rejected(format!(
                "cannot mark needs_human: {}",
                p.reason
            )));
        }
        let ev = match git::revert(std::path::Path::new(&ws), commit_sha) {
            Ok(git::RevertOutcome::Reverted) => ev_rolled,
            Ok(git::RevertOutcome::Conflict) => ev_needs,
            Err(e) => {
                return Err(BoardError::Git(format!("revert failed: {e}")));
            }
        };
        match w.append(&ev) {
            AppendOutcome::Committed(_) => {
                if matches!(ev.kind, Kind::RolledBack) {
                    println!("rolled_back");
                } else {
                    println!("needs_human");
                }
                Ok(())
            }
            AppendOutcome::InDoubt(e) => Err(BoardError::WalInDoubt(e)),
        }
    })
}

fn next_u64(value: u64, what: &'static str) -> BoardResult<u64> {
    value.checked_add(1).ok_or(BoardError::Exhausted(what))
}
