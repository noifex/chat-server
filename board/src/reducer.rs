use std::collections::BTreeMap;

use crate::wal::Poison;
use crate::events::{Event, Task, State, Kind};

/// WAL の1イベントを projection(map) に適用する状態機械。
/// ガード違反は Poison を返す（replay 中に落ちたら WAL が壊れてる証拠なので握り潰さない）。
pub fn apply(map: &mut BTreeMap<u64, Task>, ev: &Event) -> Result<(), Poison> {
    match ev.kind {
        Kind::TaskAdded => {
            if map.contains_key(&ev.task_id) {
                return Err(bad(format!("TaskAdded: task {} already exists", ev.task_id)));
            }
            map.insert(ev.task_id, Task {
                id: ev.task_id,
                state: State::Proposed,
                desc: ev.desc.clone().unwrap_or_default(),
                owner: None,
                author:None,
                active_fencing_token: None,
                claimed_at: None,
                commit_sha:None,
            });
            Ok(())
        }

        // grant: claim は Proposed からのみ。整理券(fencing) = 自分の seq を焼く。
        Kind::Claimed => {
            let task = get(map, ev.task_id)?;
            if task.state != State::Proposed {
                return Err(bad(format!("task {} is {:?}, cannot claim", ev.task_id, task.state)));
            }
            task.state = State::Claimed;
            task.owner = Some(ev.by.clone());
            task.active_fencing_token = Some(ev.seq);
            task.claimed_at = Some(ev.ts);
            Ok(())
        }

        // grant: reclaim は作業中の task を横取り。新しい整理券を焼く（旧番号は自動的に無効化）。
        Kind::Reclaimed => {
            let task = get(map, ev.task_id)?;
            match task.state { //自己承認バグを防ぐため、ここではauthorを書かない。
                State::Claimed | State::Working | State::Review => {
                    task.owner = Some(ev.by.clone());
                    task.active_fencing_token = Some(ev.seq);
                    task.claimed_at = Some(ev.ts);
                    Ok(())
                }
                other => Err(bad(format!("task {} is {:?}, cannot reclaim", ev.task_id, other))),
            }
        }

        // transition: state 一致 AND 持ってる整理券が現役と完全一致した時だけ進む。
        Kind::Working => advance(map, ev, State::Claimed, State::Working),
        Kind::Review  => {
            advance(map, ev, State::Working, State::Review)?;
            let task=get(map, ev.task_id)?;
            task.author=Some(ev.by.clone());
            Ok(())

        },
        Kind::RolledBack=>advance(map, ev, State::Compensating, State::RolledBack),
        Kind::NeedsHuman=> advance(map, ev, State::Compensating, State::NeedsHuman),

        // 独立レビュー: owner 以外だけが Review を裁ける。fencing は照合しない(reviewer は owner の整理券を持たない)。
        Kind::Approve => {
            let task = get(map, ev.task_id)?;
            if task.state != State::Review {
                return Err(bad(format!("task {} is {:?}, cannot approve", ev.task_id, task.state)));
            }
            match &task.author {
                Some(a) if a != &ev.by => {
                    task.state = State::Done;
                    task.commit_sha=ev.commit_sha.clone();
                    Ok(())
                }
                _ => Err(bad(format!("task {} cannot be approved by author or unowned", ev.task_id))), 
            }
        }

        Kind::Compensate=>{
            let task=get(map,ev.task_id)?;
            if task.state!=State::Done{
                return  Err(bad(format!("task {} is {:?}, cannot compensate",ev.task_id,task.state)));
            }
            match &task.author{
                Some(a) if a !=&ev.by=> {
                    task.state=State::Compensating;
                    task.active_fencing_token=Some(ev.seq);
                    Ok(())
                }
                _=>Err(bad(format!("task {} cannot be compensated by author or unowned",ev.task_id))),
            }
        }

        Kind::ChangesRequested => {
            let task = get(map, ev.task_id)?;
            if task.state != State::Review {
                return Err(bad(format!("task {} is {:?}, cannot request changes", ev.task_id, task.state)));
            }
            match &task.author {
                Some(a) if a != &ev.by => {
                    task.state = State::Working;
                    Ok(())
                }
                _ => Err(bad(format!("task {} cannot be sent back by author or unowned", ev.task_id))),
            }
        }
    }
}

fn advance(map: &mut BTreeMap<u64, Task>, ev: &Event, from: State, to: State) -> Result<(), Poison> {
    let task = get(map, ev.task_id)?;
    if task.state != from {
        return Err(bad(format!("task {} is {:?}, cannot -> {:?}", ev.task_id, task.state, to)));
    }
    if ev.fencing_token != task.active_fencing_token {
        return Err(bad(format!(
            "task {} fencing mismatch: presented={:?} active={:?}",
            ev.task_id, ev.fencing_token, task.active_fencing_token
        )));
    }
    task.state = to;
    Ok(())
}

fn get<'a>(map: &'a mut BTreeMap<u64, Task>, id: u64) -> Result<&'a mut Task, Poison> {
    map.get_mut(&id).ok_or_else(|| bad(format!("task {id} not found")))
}

fn bad(reason: String) -> Poison {
    Poison { reason }
}

#[cfg(test)]
mod tests{

    fn done_task()->BTreeMap<u64,Task>{
        let mut map=BTreeMap::new();
        apply(&mut map, &Event::new(1, Kind::TaskAdded, 1, "sys".into())).unwrap();
        apply(&mut map, &Event::new(2, Kind::Claimed,  1, "Coffee".into())).unwrap();
        apply(&mut map, &Event::new(3, Kind::Working,  1, "Coffee".into()).with_fencing(Some(2))).unwrap();
        apply(&mut map, &Event::new(4, Kind::Review,   1, "Coffee".into()).with_fencing(Some(2))).unwrap();
        apply(&mut map, &Event::new(5, Kind::Approve,  1, "Cola".into()).with_commit_sha("deadbeef".into())).unwrap();
        map
    }

    fn reviewed_task()->BTreeMap<u64,Task>{
        let mut map=BTreeMap::new();
        apply(&mut map, &Event::new(1, Kind::TaskAdded, 1, "sys".into())).unwrap();
        apply(&mut map, &Event::new(2, Kind::Claimed, 1, "Coffee".into())).unwrap();
        apply(&mut map, &Event::new(3, Kind::Working, 1, "Coffee".into()).with_fencing(Some(2))).unwrap();
        apply(&mut map, &Event::new(4, Kind::Review,   1, "Coffee".into()).with_fencing(Some(2))).unwrap();
        map

    }

use super::*;

    #[test]
    fn saga_done_to_rolled_back(){
        let mut map=done_task();
        assert_eq!(map[&1].state,State::Done);
        apply(&mut map, &Event::new(6, Kind::Compensate, 1, "Tea".into())).unwrap();
        assert_eq!(map[&1].state,State::Compensating);
        apply(&mut map, &Event::new(7, Kind::RolledBack, 1, "Coffee".into()).with_fencing(Some(6))).unwrap();
        assert_eq!(map[&1].state,State::RolledBack);
    }
    #[test]
    fn saga_owner_cannot_compensate(){
        let mut map=done_task();
        assert_eq!(map[&1].state,State::Done);
        let r =apply(&mut map, &Event::new(6, Kind::Compensate, 1, "Coffee".into()));
        assert!(r.is_err());
        assert_eq!(map[&1].state,State::Done);
    }
    #[test]
    fn approve_stores_commit_sha(){
        let map=done_task();
        assert_eq!(map[&1].state,State::Done);
        assert_eq!(map[&1].commit_sha.as_deref(),Some("deadbeef"));
    }
    #[test]
    fn reclaim_cannot_launder_self_approval(){
        let mut map=reviewed_task();
        assert_eq!(map[&1].state,State::Review);
        assert_eq!(map[&1].owner.as_deref(),Some("Coffee"));
        apply(&mut map, &Event::new(5, Kind::Reclaimed, 1, "Tea".into())).unwrap();
        assert_eq!(map[&1].state,State::Review);
        assert_eq!(map[&1].owner.as_deref(),Some("Tea"));
        let r =apply(&mut map, &Event::new(6, Kind::Approve, 1, "Coffee".into()));
        assert!(r.is_err());
        assert_eq!(map[&1].state,State::Review);
        assert_eq!(map[&1].author.as_deref(),Some("Coffee"));
        
    }
}