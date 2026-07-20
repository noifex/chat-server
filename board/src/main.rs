use std::env;
use std::path::PathBuf;

mod events;
mod wal;
mod reducer;

use events::{Event, Kind, Task};
use wal::{AppendOutcome, Recover, Wal};

const WAL: &str = "board.wal";

fn main() {
    let args: Vec<String> = env::args().collect();
    let wal_path = env::var("BOARD_WAL").unwrap_or_else(|_| WAL.to_string());
    let mut wal = Wal::open(PathBuf::from(wal_path));

    let code = match args.get(1).map(String::as_str) {
        Some("add")     => cmd_add(&mut wal, args.get(2).cloned().unwrap_or_default()),
        Some("claim")   => mutate(&mut wal, Kind::Claimed, &args),
        Some("reclaim") => mutate(&mut wal, Kind::Reclaimed, &args),
        Some("start")   => mutate(&mut wal, Kind::Working, &args),
        Some("review")  => mutate(&mut wal, Kind::Review, &args),
        Some("approve") => mutate(&mut wal, Kind::Approve, &args),
        Some("changes") => mutate(&mut wal, Kind::ChangesRequested, &args),
        Some("recover") => cmd_recover(&mut wal),
        Some("project") => cmd_project(&mut wal),
        _ => {
            eprintln!("usage: board <add|claim|reclaim|start|review|approve|changes|recover|project> ...");
            2
        }
    };
    std::process::exit(code);
}

fn cmd_add(wal: &mut Wal, desc: String) -> i32 {
    with_lock(wal, |w| {
        if let Recover::Poison (p) = w.recover() {
            eprintln!("poison: {}",p.reason);
            return 75;
        }
        let (tasks, max_seq) = match w.load() {
            Ok(v) => v,
            Err(p) => { eprintln!("poison: {}", p.reason); return 75; }
        };
        let id = tasks.keys().max().copied().unwrap_or(0) + 1;
        let ev = Event::new(max_seq + 1, Kind::TaskAdded, id, "cli".into()).desc(desc);
        match w.append(&ev) {
            AppendOutcome::Committed(_) => { println!("{id}"); 0 }
            AppendOutcome::InDoubt(e)   => { eprintln!("indoubt: {e}"); 75 }
        }
    })
}

/// 全書き込みの心臓。lock 区間の中で recover → load → guard 試し打ち → append。
fn mutate(wal: &mut Wal, kind: Kind, args: &[String]) -> i32 {
    let id: u64 = match args.get(2).and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => { eprintln!("bad or missing <id>"); return 2; }
    };
    let peer = match args.get(3) {
        Some(p) => p.clone(),
        None => { eprintln!("missing <peer>"); return 2; }
    };
    let is_grant = matches!(kind, Kind::Claimed | Kind::Reclaimed);
    // transition(start/review) は第4引数に整理券を持ち歩く。approve/changes は取らない(reviewer は owner の整理券を持たない)。
    let fencing_arg: Option<u64> = args.get(4).and_then(|s| s.parse().ok());

    with_lock(wal, |w| {
        if let Recover::Poison (p) = w.recover() {
            eprintln!("poison: {}",p.reason);
            return 75;
        }
        let (tasks, max_seq) = match w.load() {
            Ok(v) => v,
            Err(p) => { eprintln!("poison: {}", p.reason); return 75; }
        };

        let seq = max_seq + 1;
        let token = if is_grant { Some(seq) } else { fencing_arg };
        let mut ev = Event::new(seq, kind, id, peer).with_fencing(token);
        if let Some(t) = tasks.get(&id) {
            ev = ev.expected(t.state);
        }

        // 本番 map に触る前に clone で試し打ち → 弾かれるなら WAL を汚さず終わる
        let mut probe = tasks.clone();
        if let Err(p) = reducer::apply(&mut probe, &ev) {
            eprintln!("rejected: {}", p.reason);
            return 1;
        }

        match w.append(&ev) {
            AppendOutcome::Committed(s) => {
                if is_grant {
                    println!("{{\"task_id\":{id},\"fencing_token\":{s}}}");
                } else {
                    println!("ok");
                }
                0
            }
            AppendOutcome::InDoubt(e) => { eprintln!("indoubt: {e}"); 75 }
        }
    })
}

fn cmd_recover(wal: &mut Wal) -> i32 {
    with_lock(wal, |w| match w.recover() {
        Recover::Clean => { println!("clean"); 0 }
        Recover::TruncatedTail { dropped_bytes } => {
            println!("truncated_tail dropped={dropped_bytes}");
            0
        }
        Recover::Poison (p)=> { eprintln!("poison: {}",p.reason); 75 }
    })
}

fn cmd_project(wal: &mut Wal) -> i32 {
    with_lock(wal, |w| {
        if let Recover::Poison(p) = w.recover() {
            eprintln!("poison: {}",p.reason);
            return 75;
        }
        let (tasks, _) = match w.load() {
            Ok(v) => v,
            Err(p) => { eprintln!("poison: {}", p.reason); return 75; }
        };
        let list: Vec<&Task> = tasks.values().collect();
        println!("{}", serde_json::to_string_pretty(&list).unwrap());
        0
    })
}

/// with_exclusive のラッパ。lock 自体が取れなければ 74。
fn with_lock(wal: &mut Wal, f: impl FnOnce(&mut wal::Locked) -> i32) -> i32 {
    wal.with_exclusive(f).unwrap_or_else(|e| {
        eprintln!("lock: {e}");
        74
    })
}
