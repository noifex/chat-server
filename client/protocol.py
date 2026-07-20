import json

def build_line(frm, model, typ, text, reply_to=None, task_id=None, confidence=None,
               confidence_method=None, confidence_scale=None, domain=None, router=None)-> str:
    # 設計デルタ① (2026-07-16): confidence は出所を伴わないと跨model比較が無根拠
    # (metacognition 2607.11881 §4.2)。router は routing の meaningfulness を
    # auditable にする (routing 2607.09197)。全て optional=None で後方互換。
    d={"from":frm, "model":model,"type":typ,"text":text,"reply_to":reply_to,"task_id":task_id,
       "confidence":confidence,"confidence_method":confidence_method,
       "confidence_scale":confidence_scale,"domain":domain,"router":router}
    return json.dumps(d,ensure_ascii=False)

def split_treansport(raw:str):
    line=raw.rstrip("\n")
    head,sep,rest=line.partition(": ")
    if sep=="":
        return None,line
    return head,rest

def parse_line(raw:str)->dict:
    name,payload=split_treansport(raw)
    ev=None
    if name is not None:
        try:
            obj=json.loads(payload)
            if isinstance(obj,dict):
                ev=obj
        except(json.JSONDecodeError,ValueError):
            ev=None
    if ev is None:
        ev={"from":name,"type":"say","text":payload}
    ev.setdefault("from", name)
    ev.setdefault("model", None)
    ev.setdefault("type", "say")
    ev.setdefault("text", "")
    ev.setdefault("reply_to", None)
    ev.setdefault("task_id", None)
    ev.setdefault("confidence", None)
    ev.setdefault("confidence_method", None)   # self_report | logprob | sampled
    ev.setdefault("confidence_scale", None)    # 例 "0-20"（0-100は3値に潰れる）
    ev.setdefault("domain", None)              # 例 "rust"（校正はdomain依存）
    ev.setdefault("router", None)              # {policy_version, selected, rationale, perturbation_id}
    return ev

def render(ev)->str:
    """envelope dict → 人間向け1行。daemon の inbox 描画と human.py 表示で共用。"""
    if ev["from"] is None:                       # システム行（joined/left/history区切り）
        return ev["text"]
    if ev["type"]=="say":
        return f'{ev["from"]}: {ev["text"]}'
    return f'{ev["from"]} [{ev["type"]}]: {ev["text"]}'   # claim/done を可視化