use std::{env};
use std::path::PathBuf;

mod events;
mod wal;
mod reducer;
mod git;
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
        Some("compensate")=> mutate(&mut wal, Kind::Compensate, &args),
        Some("revert")=>cmd_revert(&mut wal, &args),
        _ => {
            eprintln!("usage: board <add|claim|reclaim|start|review|approve|changes|recover|project|compensate|revert> ...");
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
    let is_grant = matches!(kind, Kind::Claimed | Kind::Reclaimed|Kind::Compensate);
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

        // approve ok -> done -> git commit-> sha 
        if matches!(kind,Kind::Approve){
            let ws =env::var("BOARD_WORKSPACE").unwrap_or_else(|_| "workspace".into());
            match git::commit(std::path::Path::new(&ws), id) {
                Ok(sha)=>{ev=ev.with_commit_sha(sha);}
                Err(e)=>{eprintln!(" {e}"); return 75;}
            }
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


fn cmd_revert(wal:&mut Wal,args: &[String])->i32{
    let task_id: u64 = match args.get(2).and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => { eprintln!("bad or missing <id>"); return 2; }
    };
    let peer = match args.get(3) {
        Some(p) => p.clone(),
        None => { eprintln!("missing <peer>"); return 2; }
    };
    let token:u64=match args.get(4).and_then(|s| s.parse().ok()){
        Some(v)=>v,
        None=>{eprintln!("missing <token>"); return  2;}
    };
    let ws =env::var("BOARD_WORKSPACE").unwrap_or_else(|_| "workspace".into());

    with_lock(wal, |w| {
        if let Recover::Poison(p) =w.recover()  {
            eprintln!("poison:{}",p.reason);
            return 75;
        }
        let (tasks,max_seq)=match w.load(){
            Ok(v)=>v,
            Err(p)=>{eprintln!("poison:{}",p.reason); return 75;}
        };
        let new_peer=peer.clone();
        let task= match tasks.get(&task_id){
            Some(t)=>t,
            None=>{eprintln!("None task id"); return 1;}
        };
        let commit_sha= match &task.commit_sha {
            Some(s)=>s,
            None=>{eprintln!("None commit sha"); return  37;}
        };
        let next_seq:u64=max_seq+1;
        let ev_rolled=Event::new(next_seq, Kind::RolledBack, task_id, peer).with_fencing(Some(token));
        let ev_needs=Event::new(next_seq,Kind::NeedsHuman,task_id,new_peer).with_fencing(Some(token)); // ev_needs need 'new_peer(peer.clone())' because by:String. moved.
        
        let mut probe_rolled = tasks.clone();
        let mut probe_needs = tasks.clone();
        if let Err(p) =  reducer::apply(&mut probe_rolled, &ev_rolled){
            eprintln!("can't roll: {}",p.reason);
            return 1;
        }
        if let Err(p) =  reducer::apply(&mut probe_needs, &ev_needs){
            eprintln!("can't call human: {}",p.reason);
            return 1;
        }
        let ev=match git::revert(std::path::Path::new(&ws), commit_sha) {
            Ok(git::RevertOutcome::Reverted)=>{
                ev_rolled                
            },
            Ok(git::RevertOutcome::Conflict)=>{
                ev_needs
            },
            Err(e)=>{
                eprintln!("can't revert {}",e);
                return 75;
            }
        };
        match w.append(&ev){
            AppendOutcome::Committed(_)=>{
                if matches!(ev.kind,Kind::RolledBack) {
                    println!("rolled_back");
                }else{
                    println!("needs_human");
                }
                0
            }
            AppendOutcome::InDoubt(e) => { eprintln!(" {e}"); 75 }
        }
    })


}