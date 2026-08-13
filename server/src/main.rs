use std::collections::VecDeque;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::net::Shutdown;
use std::net::TcpListener;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::SyncSender;// tx: Sender -> SyncSender because senderなら高速出力する個体がいたらbroadcasterが遅れ、全体の配信が遅延とメモリ消費アップ。 so tx:SyncSender
use std::sync::mpsc::TrySendError;
use std::thread::spawn;
// nc 127.0.0.1 8080
enum Event{
    Join {id:u64,name:String , out:SyncSender<String>,kill:TcpStream}, // this stream is for write_stream ==broadcaster
    Move {id:u64, text:String},
    Leave {id:u64},
}
struct Client{
    id:u64,
    name:String,
    //stream:TcpStream,
    out:SyncSender<String>,
    kill:TcpStream,
    }
struct LeaveGuard{id:u64,tx:SyncSender<Event>}
impl Drop for LeaveGuard{
    fn drop(&mut self) {
        let _= self.tx.send(Event::Leave { id: self.id });
    }
}

fn main() -> std::io::Result<()>{
    let addr:SocketAddr="127.0.0.1:8080".parse().unwrap();
    let listener=TcpListener::bind(addr).expect("failed");
    println!("listening on {}",addr);
    let (tx,rx)=mpsc::sync_channel::<Event>(1024);
    spawn(move || broadcaster(rx));
    for (id,stream) in (0_u64..).zip(listener.incoming()){
        match stream {
            Ok(s)=>{
                let tx=tx.clone();
                spawn(move ||{
                  if let Err(e)=  handle(id, s,tx){
                    eprintln!("client {id}:{e}");
                  }
                });
            }
            Err(e)=>{
                eprintln!("Connection failed: {}",e);
            }
        }
    }
    Ok(())
}

fn handle(id:u64,stream:TcpStream,tx:SyncSender<Event>)->std::io::Result<()>{
    let mut reader=BufReader::new(stream.try_clone()?);
    let kill=stream.try_clone()?;
    let (out,rx)=mpsc::sync_channel::<String>(256);
    let name=read_name(&mut  reader)?;
    spawn(move || writer(stream,rx));
    tx.send(Event::Join { id,name, out,kill }).map_err(|_| 
            io::Error::new(io::ErrorKind::BrokenPipe, "client failed to join"))?;
    let _guard=LeaveGuard{id,tx:tx.clone()};
    for line in reader.lines(){
        let text=line?;
        tx.send(Event::Move { id, text  }).unwrap();
    }
    Ok(())
}

fn writer(mut stream:TcpStream,rx:Receiver<String>){
    for line in rx{
        if stream.write_all(line.as_bytes()).is_err(){break;}
    }
    let _=stream.shutdown(Shutdown::Both);
}

fn broadcaster(rx:Receiver<Event>){
    let mut clients: Vec<Client>=Vec::new();
    let mut history:VecDeque<String>=VecDeque::new();
    for ev in rx{
        match ev{ 
            Event::Join {id,name,out,kill}=>{
                if let Err(e) =  greet(&out, &history){
                    eprintln!("greet failed for {name}(id={id}): {e}");
                    let _=kill.shutdown(Shutdown::Both);
                    continue;
                }
                clients.push(Client { id, name:name.clone(), out, kill }); 
                broadcast(&mut clients, &format!("{name} joined"));
            },
            Event::Move { id, text }=>{
                let Some(c)=clients.iter().find(|c| c.id==id) else {
                    continue;
                };
                let name=c.name.clone();
                broadcast(&mut clients, &format!("{name}: {text}"));
                history.push_back(format!("{}: {}",name, text));
                if history.len()>30{history.pop_front();} // how many history should keep.  
            },
            Event::Leave { id }=>{
                let Some(pos)=clients.iter().position(|c| c.id==id)else {
                    continue;
                };
                let name=clients.remove(pos).name;
                broadcast(&mut clients, &format!("{name} left"));
            }
            
        }
       
    }
}
fn broadcast(clients: &mut Vec<Client>, msg: &str) {
      let line=format!("{msg}\n");
      for c in clients {
        if c.out.try_send(line.clone()).is_err(){
            let _=c.kill.shutdown(Shutdown::Both);
        }
      }
  }
fn read_name(reader: &mut impl BufRead) -> std::io::Result<String> {
      let mut name = String::new();
      let n =reader.read_line(&mut name)?;
      if n==0 {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "no name"));
      }
      Ok(name.trim().to_string())
  }

fn greet(out:&SyncSender<String>,history:& VecDeque<String>)->Result<(), TrySendError<String>>{
    let mut s =String::new();
    s.push_str("---- history - start ---\n");
    for line in history{
        s.push_str(line);
        s.push('\n');
    }
    s.push_str("---- history - end ---\n");
    out.try_send(s)
}