use std::process::Command;
use std::path::PathBuf;
use std::{env::temp_dir, sync::atomic::{AtomicU64, Ordering}, process};
  fn board(wal: &PathBuf) -> Command {
      let mut c = Command::new(env!("CARGO_BIN_EXE_board"));  // ← cargo が渡すパス。"board"=bin名
      c.env("BOARD_WAL", wal);
      c
  }
    fn tmp_wal()-> PathBuf{
        static COUNTER:AtomicU64=AtomicU64::new(0);
        let n =COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid=process::id();
        let name=format!("board_test_{pid}_{n}.wal");
        temp_dir().join(name)

    }

  #[test]
  fn cold_start_replay() {
  let wal = tmp_wal();
    //add
    let out = board(&wal).args(["add", "first task"]).output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(),"1");

    //claim
    let out = board(&wal).args(["claim", "1", "Coffee"]).output().unwrap();
    assert!(out.status.success());
    //crash recovery
    let out = board(&wal).args(["project"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("first task"));
    assert!(stdout.contains("claimed"));
    assert!(stdout.contains("Coffee"));
    let _ = std::fs::remove_file(&wal);
    let _ = std::fs::remove_file(wal.with_added_extension("lock"));
  }
  #[test]
  fn kill_releases_lock_and_recovers(){
    let wal=tmp_wal();

    let mut child=board(&wal)
        .env("BOARD_PAUSE_AFTER_WRITE","1")
        .args(["add","victim task"])
        .spawn().unwrap();
    //for testing. 500'ms' is ok 
    std::thread::sleep(std::time::Duration::from_millis(500));
    // kill -9
    child.kill().unwrap();
    let _=child.wait();

    let out=board(&wal).args(["project"]).output().unwrap();
    assert!(out.status.success()); 
    let stdout=String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("victim task")); // if ok = data is live = safety.
    //durability永続性 は証明不能。理由：電源断つされても残れるかの領域問題。
    let _ = std::fs::remove_file(&wal);
    let _ = std::fs::remove_file(wal.with_added_extension("lock"));


  }