use std::path::{Path};
use std::process::Command;
use std::io;
pub enum RevertOutcome {
    Reverted,
    Conflict,
}


pub fn commit(workspace:&Path, task_id:u64)->io::Result<String>{
    // git add -A
    let add= Command::new("git") //which program commands
        .current_dir(workspace) // path
        .arg("add") // one word one arg/ use [""]
        .arg("-A")
        .output()?;
    if !add.status.success(){return  Err(io::Error::other("git add failed"));}
    // git commit -m "task {task_id}"
    let commit= Command::new("git")
        .current_dir(workspace)
        .arg("commit")
        .arg("-m")
        .arg(format!("task {task_id}"))
        .output()?;
    if !commit.status.success(){
        return Err(io::Error::other(
            format!("git commit failed: {}", String::from_utf8_lossy(&commit.stderr))
        ));
    }
    
    // git rev-parse HEAD 
    let rev_parse=Command::new("git")
        .current_dir(workspace)
        .arg("rev-parse").arg("HEAD")
        .output()?;
    if !rev_parse.status.success(){
        return  Err(io::Error::other(
            format!("git rev_parse failed:{}",
            String::from_utf8_lossy(&rev_parse.stderr))
        ));
    }
    let sha=String::from_utf8_lossy(&rev_parse.stdout)
        .trim().to_string();
    Ok(sha)
}
  pub fn revert(workspace: &Path, sha: &str) -> io::Result<RevertOutcome> {
      let out = Command::new("git")
          .current_dir(workspace)
          .args(["revert", "--no-edit", sha])
          .output()?;
    let text=format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
      if out.status.success() {
          Ok(RevertOutcome::Reverted)
      } else if text.contains("CONFLICT"){
        let _ = Command::new("git")
              .current_dir(workspace)
              .args(["revert", "--abort"])
              .output();
          Ok(RevertOutcome::Conflict)
      }else {
          Err(io::Error::other(format!("git revert <sha> failed:{}",text)))
      }
      //普通の再実行はここまで来れない。cmd_revert probeがstateで弾く。
      //(RolledBack-> RolledBackは不正遷移 -> exit code 1)can't roll: task 1 is RolledBack, cannot -> RolledBack
      //1
      //ここにこれるのはWAL appendが失敗か中断した窓だけ。(テスト後の補註)↓テストする前のコメント
      // えーとrevert済みの失敗はErrを返すだけです
      // 例：git revert ok -> wal appendの前に失敗 ->再実行
      // workspaceは巻き戻り済みけどboardはCompensatingのまま。
      //exit code 75になるが、それは後で再試行　の意味。でも何度やってもなおらないのでここの75は信用できない。
      // この処理についてはstep 13c: dual write復旧の実装が担当するが、またのちほど。

      //else if text.contains("CONFLICT")
      //なぜ抜けられるかというと
      //gitが返したものにCONFLICTが含まれてないからerrになった。
  }


#[cfg(test)]
mod test{
    use super::*;
    use std::path::{Path,PathBuf};
    use std::sync::atomic::{Ordering,AtomicU64};
    use std::{process,process::Command};
    use std::env::temp_dir;
    

    fn tmp_repo()-> PathBuf{
        static COUNTER:AtomicU64=AtomicU64::new(0);
        let n =COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid=process::id();
        let dir=temp_dir().join(format!("board_test_{pid}_{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        run_git(&dir, &["init"]);
        run_git(&dir, &["config", "user.email", "t@t"]);
        run_git(&dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("README"), "init").unwrap();
        run_git(&dir, &["add", "-A"]);
        run_git(&dir, &["commit", "-m", "init"]);
        dir
}
    fn run_git(dir:&Path,args:&[&str]){
        let out= Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(),"git {:?} failed: {}",args, String::from_utf8_lossy(&out.stderr));
    }

#[test]
fn commit_then_revert(){
let repo = tmp_repo();
    std::fs::write(repo.join("a.txt"), "hello").unwrap();
    let sha = commit(&repo, 1).unwrap();
    assert_eq!(sha.len(), 40);
    let outcome = revert(&repo, &sha).unwrap();
    assert!(matches!(outcome, RevertOutcome::Reverted)); 
    assert!(!repo.join("a.txt").exists());
    let _ = std::fs::remove_dir_all(&repo);
}
}