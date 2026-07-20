from collections import deque
from protocol import parse_line, build_line
import socket,re,os,tempfile
PERSONAS={"Coffee","Cola","Tea"}
ROLE={"PROPOSE":"Coffee","CRITIQUE":"Cola","REBUT":"Coffee","SYNTHESIZE":"Tea"}
NEXT={"PROPOSE":"CRITIQUE","CRITIQUE":"REBUT","REBUT":"SYNTHESIZE","SYNTHESIZE":"CRITIQUE"}
TURN=os.path.join(os.path.dirname(__file__),"clients","turn")
LOG=os.path.join(os.path.dirname(__file__),".runtime","chat.log")
CLIENTS=os.path.join(os.path.dirname(__file__),"clients")
queue=deque()
turns_since_tea=0

POLICY_VERSION="sm-queue-v1"   # 現routing方針: 状態機械 + @address queue + Tea cadence
ROUTE_LOG=os.path.join(os.path.dirname(__file__),".runtime","route.log")

def write_turn(name):
    d=os.path.dirname(TURN) or "."
    fd,tmp =tempfile.mkstemp(dir=d)
    with os.fdopen(fd,"w") as f:f.write(name)
    os.rename(tmp,TURN)

def log_route(selected, rationale, perturbation_id=None):
    # 設計デルタ①: routing 決定を auditable に (routing 2607.09197 = meaningfulness の検証前提)
    line=build_line("orchestrator",None,"route",selected,
                    router={"policy_version":POLICY_VERSION,"selected":selected,
                            "rationale":rationale,"perturbation_id":perturbation_id})
    try:
        os.makedirs(os.path.dirname(ROUTE_LOG),exist_ok=True)
        with open(ROUTE_LOG,"a") as f:f.write(line+"\n")
    except OSError:
        pass
    
def parse_address(text):
    out =[]
    for m in re.finditer(r"@(\w+)|(\w+)、",text):
        name=m.group(1) or m.group(2)
        if name in PERSONAS and name not in out:
            out.append(name)
    return out

def wake(name):
    fifo=os.path.join(CLIENTS,name,f"{name}.notify")
    try:
        fd=os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
        os.write(fd,b"1")
        os.close(fd)
    except OSError:
        pass

def read_turn():
    try:
        with open(TURN) as f:
            return f.read().strip()
    except FileNotFoundError:
        return None
        
s= socket.create_connection(("127.0.0.1",8080))
s.sendall(b"orchestrator\n")
state,started="PROPOSE",False
for raw in s.makefile():
    ev=parse_line(raw)
    if ev["from"] is None:
        continue
    who,text,type=ev["from"],ev["text"],ev["type"]
    
    if who == "Tea" and type=="done":
        write_turn("STOP")
        for p in PERSONAS:wake(p)
        break
    for a in parse_address(text):
        if a!=who and a not in queue:
            queue.append(a)
            
    if who in PERSONAS:
        started=True
        if who !="Tea":
            turns_since_tea+=1
        else:
            turns_since_tea=0
        if turns_since_tea>=6 and "Tea" not in queue:
            queue.appendleft("Tea")
            turns_since_tea=0
        if queue:
            nxt=queue.popleft(); rationale="queue"
        else:
            state=NEXT[state]
            nxt=ROLE.get(state,"none"); rationale=f"state:{state}"
        write_turn(nxt); log_route(nxt,rationale); wake(nxt)

    else:
        nxt=None; rationale=None
        if queue:
            nxt=queue.popleft(); rationale="queue"
        elif not started:
            nxt="Coffee"; rationale="bootstrap"
        if nxt:
            started=True
            write_turn(nxt); log_route(nxt,rationale); wake(nxt)