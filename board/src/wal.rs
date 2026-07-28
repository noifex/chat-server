use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::events::{Event, Task};
use crate::reducer;

#[derive(Debug)]
pub struct Poison { pub reason: String }

impl From<io::Error> for Poison {
    fn from(e: io::Error) -> Self { Poison { reason: e.to_string() } }
}
impl From<serde_json::Error> for Poison {
    fn from(e: serde_json::Error) -> Self { Poison { reason: e.to_string() } }
}

/// append の結果。Committed になって初めて「載った」。InDoubt は載ったか不明＝嘘の成功を返さない。
pub enum AppendOutcome { Committed(u64 /*seq*/), InDoubt(io::Error) }

/// recover の結果。TruncatedTail は末尾破損を切って回復、Poison は中間破損で手を出さない。
pub enum Recover { Clean, TruncatedTail { dropped_bytes: u64 }, Poison (Poison) } 

pub struct Wal {
    path: PathBuf,
    lock_path: PathBuf,
    poisoned: bool,
}

impl Wal {
    pub fn open(path: PathBuf) -> Wal {
        let lock_path = path.with_added_extension("lock");
        Wal { path, lock_path, poisoned: false }
    }

    /// flock LOCK_EX を取ってから f を走らせる。lockfile は drop で解放される。
    pub fn with_exclusive<T>(&mut self, f: impl FnOnce(&mut Locked) -> T) -> io::Result<T> {
        let lockfile = OpenOptions::new().create(true).write(true).open(&self.lock_path)?;
        lockfile.lock()?;
        let mut locked = Locked { wal: self };
        let result = f(&mut locked);
        Ok(result) // lockfile はここで drop → advisory lock 解放
    }
}

pub struct Locked<'a> { wal: &'a mut Wal }

impl Locked<'_> {
    /// 末尾破損なら切って回復、中間破損なら Poison。存在しない/正常なら Clean。
    pub fn recover(&mut self) -> Recover {
        let file = match File::open(&self.wal.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Recover::Clean,
            Err(e) => return Recover::Poison(Poison::from(e)) ,
        };
        let mut reader = BufReader::new(file);
        let mut last_good: u64 = 0; // 最後に完全 parse できた行の直後の byte offset
        let mut offset: u64 = 0;
        let mut buf = String::new();
        loop {
            buf.clear();
            let n = match reader.read_line(&mut buf) {
                Ok(0) => return Recover::Clean,
                Ok(n) => n,
                Err(e) => return Recover::Poison (Poison::from(e)),
            };
            offset += n as u64;
            let has_nl = buf.ends_with('\n');
            let content = buf.trim_end();

            if content.is_empty() && has_nl {
                last_good = offset; // 空行は許容
            } else if has_nl && serde_json::from_str::<Event>(content).is_ok() {
                last_good = offset; // 改行で閉じた正常な1行
            } else if has_nl {
                // 改行付きなのに parse 不能 = 書き込みは完了したが中身が壊れてる = 中間破損
                return Recover::Poison (Poison { reason: format!("corrupt event a byte {last_good}") });
            } else {
                // 改行が無い = 末尾で書き込みが千切れた = torn tail。切って回復。
                return self.truncate_to(last_good, offset - last_good);
            }
        }
    }

    fn truncate_to(&mut self, last_good: u64, dropped: u64) -> Recover {
        let file = match OpenOptions::new().write(true).open(&self.wal.path) {
            Ok(f) => f,
            Err(e) => return Recover::Poison(Poison::from(e)),
        };
        if let Err(e) = file.set_len(last_good) {
            return Recover::Poison(Poison::from(e));
        }
        if let Err(e) = file.sync_all() {
            return Recover::Poison(Poison::from(e));
        }
        // 親 dir を fsync（truncate をディレクトリエントリに焼く。best-effort）
        let parent = self.wal.path.parent().unwrap_or(Path::new(""));
        let dir = if parent.as_os_str().is_empty() { Path::new(".") } else { parent };
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
        Recover::TruncatedTail { dropped_bytes: dropped }
    }

    /// WAL を replay して projection と max_seq を作る。壊れた行に当たったら Poison。
    pub fn load(&self) -> Result<(BTreeMap<u64, Task>, u64), Poison> {
        let file = match File::open(&self.wal.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((BTreeMap::new(), 0)),
            Err(e) => return Err(Poison::from(e)),
        };
        let reader = BufReader::new(file);
        let mut map = BTreeMap::new();
        let mut prev=0u64;
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let ev: Event = serde_json::from_str(&line)?;
            let expected=prev+1;
            if ev.seq!=expected{
                return Err(Poison { reason: format!("seq gap: expected {expected}, got {}",ev.seq) });
            }
            reducer::apply(&mut map, &ev)?;
            prev=ev.seq;
        }
        Ok((map, prev))
    }

    /// 1行 append して fsync。sync_all まで抜けて初めて Committed。途中で落ちたら InDoubt + poison。
    pub fn append(&mut self, ev: &Event) -> AppendOutcome {
        let line = match serde_json::to_string(ev) {
            Ok(l) => l,
            Err(e) => {
                self.wal.poisoned = true;
                return AppendOutcome::InDoubt(io::Error::new(io::ErrorKind::InvalidData, e));
            }
        };
        let mut file = match OpenOptions::new().create(true).append(true).open(&self.wal.path) {
            Ok(f) => f,
            Err(e) => { self.wal.poisoned = true; return AppendOutcome::InDoubt(e); }
        };
        if let Err(e) = writeln!(file, "{line}") {
            self.wal.poisoned = true;
            return AppendOutcome::InDoubt(e);
        }
        //pause-hook for kill -9 test

        if std::env::var("BOARD_PAUSE_AFTER_WRITE").is_ok(){
            loop{std::thread::sleep(std::time::Duration::from_secs(1));}
        }
        if let Err(e) = file.sync_all() {
            self.wal.poisoned = true;
            return AppendOutcome::InDoubt(e);
        }
        AppendOutcome::Committed(ev.seq)
    }

    //pub fn contains(&self, task_id:u64,kind:Kind,fencing:Option<u64>)->bool{
    //}
}

#[cfg(test)]
mod tests{
    use std::{env::temp_dir, sync::atomic::{AtomicU64, Ordering}, process};

use super::*;
    use crate::{events::{Event,Kind}};

    fn tmp_wal()-> PathBuf{
        static COUNTER:AtomicU64=AtomicU64::new(0);
        let n =COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid=process::id();
        let name=format!("board_test_{pid}_{n}.wal");
        temp_dir().join(name)

    }

    fn good_line(seq:u64,kind:Kind,task_id:u64 )->String{
        let ev=Event::new(seq, kind, task_id, "tester".to_string());
        let mut s=serde_json::to_string(&ev).unwrap();
        s.push('\n');
        s // this is ok 
    }
    fn write_wal(line:&[&str])->PathBuf{
        let path=tmp_wal();
        let contents=line.concat();
        std::fs::write(&path, &contents).unwrap();
        path
    }
struct TempWal { path: PathBuf }
impl Drop for TempWal {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_added_extension("lock"));
    }
  }

  
#[test]
fn torn_tail_recovers(){
    
    let torn=serde_json::to_string(&Event::new(3,Kind::TaskAdded,3,"tester".to_string())).unwrap();
    let path=write_wal(&[
        &good_line(1, Kind::TaskAdded, 1),
        &good_line(2, Kind::TaskAdded, 2),
        &torn,
    ]);
    let _tmp=TempWal{path:path.clone()};
    let mut wal=Wal::open(path.clone());
    let rec=wal.with_exclusive(|w| w.recover()).unwrap();
    match rec {
        Recover::TruncatedTail { dropped_bytes }=>{
            assert_eq!(dropped_bytes,torn.len() as u64);
        }
        _=> panic!("expected TruncatedTail"),
    }
    let (map,max_seq)=wal.with_exclusive(|w| w.load()).unwrap().unwrap();
    assert_eq!(max_seq,2);
    assert_eq!(map.len(),2);
    }
#[test]
fn clean_wal_loads_all(){
    let path=write_wal(&[
        &good_line(1, Kind::TaskAdded, 1),
        &good_line(2, Kind::TaskAdded, 2),
        &good_line(3, Kind::TaskAdded, 3),

    ]);
    let _tmp=TempWal{path:path.clone()};
    let mut wal=Wal::open(path.clone());
    let rec=wal.with_exclusive(|w| w.recover()).unwrap();
    assert!(matches!(rec,Recover::Clean));
    let (map,max_seq)=wal.with_exclusive(|w| w.load()).unwrap().unwrap();
    assert_eq!(max_seq,3);
    assert_eq!(map.len(),3);
}
#[test]
fn mid_corrupt_is_poison(){
    let path=write_wal(&[
        &good_line(1, Kind::TaskAdded, 1),
        "this is not json\n",
        &good_line(3, Kind::TaskAdded, 3),
    ]);
    let _tmp=TempWal{path:path.clone()};
    let mut wal=Wal::open(path.clone());
    let rec=wal.with_exclusive(|w| w.recover()).unwrap();
    assert!(matches!(rec,Recover::Poison(_)));
    match rec{
        Recover::Poison(p)=>{
            assert!(p.reason.contains("corrupt"));
        }
        _=>panic!("expected Poison"),// can delete this line because line 245 is checking same thing
    }
}
#[test]
fn seq_gap(){
    let path=write_wal(&[
        &good_line(1, Kind::TaskAdded, 1),
        &good_line(2, Kind::TaskAdded, 2),
        &good_line(4, Kind::TaskAdded, 4),
    ]);
    let _tmp=TempWal{path:path.clone()};
    let mut wal=Wal::open(path.clone());
    let rec=wal.with_exclusive(|w| w.recover()).unwrap();
    assert!(matches!(rec,Recover::Clean));

    let result =wal.with_exclusive(|w| w.load()).unwrap();
    assert!(result.is_err());
}
#[test]
fn seq_dup(){
        let path=write_wal(&[
        &good_line(1, Kind::TaskAdded, 1),
        &good_line(2, Kind::TaskAdded, 2),
        &good_line(2, Kind::TaskAdded, 2),
    ]);
    let _tmp=TempWal{path:path.clone()};
    let mut wal=Wal::open(path.clone());
    let rec=wal.with_exclusive(|w| w.recover()).unwrap();
    assert!(matches!(rec,Recover::Clean));

    let result =wal.with_exclusive(|w| w.load()).unwrap();
    assert!(result.is_err());
}
}
