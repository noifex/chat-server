use std::collections::VecDeque;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::net::TcpListener;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::thread::spawn;
// nc 127.0.0.1 8080
enum Event{
    Join {id:u64,name:String , stream:TcpStream}, // this stream is for write_stream ==broadcaster
    Move {id:u64, text:String},
    Leave {id:u64},
}
struct Client{id:u64,name:String,stream:TcpStream}

fn main() -> std::io::Result<()>{
    let addr:SocketAddr="127.0.0.1:8080".parse().unwrap();
    let listener=TcpListener::bind(addr).expect("failed");
    println!("listening on {}",addr);
    let (tx,rx)=mpsc::channel::<Event>();
    spawn(move || broadcaster(rx));
    let mut next_id:u64=0;
    for stream in listener.incoming(){
        let id=next_id;
        next_id+=1;
        
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

fn handle(id:u64,stream:TcpStream,tx:Sender<Event>)->std::io::Result<()>{
    let mut reader=BufReader::new(stream.try_clone()?);
    
    let name=read_name(&mut  reader)?;
    tx.send(Event::Join { id,name, stream }).map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "client failed to join"))?;
    for line in reader.lines(){
        let text=line?;
        tx.send(Event::Move { id, text  }).unwrap();
    }
    tx.send(Event::Leave { id }).unwrap();
    Ok(())
}

fn broadcaster(rx:Receiver<Event>){
    let mut clients: Vec<(u64,String, TcpStream)>=Vec::new();
    let mut history:VecDeque<String>=VecDeque::new();
    for ev in rx{
        match ev{ 
            Event::Join {id,name,mut stream}=>{
                
                if let Err(e) =  greet(&mut stream, & history){
                    eprintln!("greet failed for {name}(id={id}): {e}");
                    continue;
                }
                clients.push((id,name.clone(),stream)); 
                broadcast(&mut clients, &format!("{name} joined"));
                //println!("{}: joined", name);
            },
            Event::Move { id, text }=>{
                let name={let Some((_,n,_))=clients.iter().find(|(cid,_,_)|*cid== id)else{
                    continue};
                    n.clone()
                };
                broadcast(&mut clients, &format!("{name}: {text}"));
                history.push_back(format!("{}: {}",name, text));
                if history.len()>30{history.pop_front();} // how many history should keep.  
            },
            Event::Leave { id }=>{
                let Some(pos)=clients.iter().position(|(cid,_,_)|*cid==id) else {continue;}; 
                let (_,name,_)=clients.remove(pos);
                broadcast(&mut clients, &format!("{name} left"));
            }
            
        }
       
    }
}
fn broadcast(clients: &mut Vec<(u64,String, TcpStream)>, msg: &str) {
      clients.retain_mut(|(_,_, s)| writeln!(s, "{msg}").is_ok());
  }
fn read_name(reader: &mut impl BufRead) -> std::io::Result<String> {
      let mut name = String::new();
      let n =reader.read_line(&mut name)?;
      if n==0 {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "no name"));
      }
      Ok(name.trim().to_string())
  }

fn greet(stream:&mut TcpStream,history:& VecDeque<String>)->io::Result<()>{
    if history.is_empty(){
        writeln!(stream," new conversation")?;
    }else {
        writeln!(stream,"---- history - start ----")?;
        for line in history{
        writeln!(stream,"{line}")?;
                    }
        writeln!(stream,"---- history - end ----")?;
    }
    Ok(())
}