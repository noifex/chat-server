from protocol import build_line, parse_line

# transport 剥がし + jq エスケープの合流点（`: ` と `"` 混在）
ev = parse_line(f'Cola: {build_line("Cola","claude-opus-4-8","say",chr(97)+": "+chr(34)+"b"+chr(34))}')
assert ev["text"] == 'a: "b"', ev
assert parse_line("Cola joined\n")["from"] is None

# 設計デルタ①: 新フィールド round-trip
line = build_line("Cola", "claude-sonnet-5", "say", "hi", confidence=0.62,
                  confidence_method="self_report", confidence_scale="0-20", domain="rust",
                  router={"policy_version": "sm-queue-v1", "selected": "Coffee",
                          "rationale": "queue", "perturbation_id": None})
ev = parse_line(f"Cola: {line}")
assert ev["confidence"] == 0.62, ev
assert ev["confidence_method"] == "self_report", ev
assert ev["confidence_scale"] == "0-20", ev
assert ev["domain"] == "rust", ev
assert ev["router"]["selected"] == "Coffee", ev

# 後方互換: 旧envelope（新フィールドなし）は None に default
old = '{"from":"Tea","model":null,"type":"say","text":"yo"}'
ev = parse_line(f"Tea: {old}")
assert ev["confidence_method"] is None and ev["domain"] is None and ev["router"] is None, ev

# 平文 fallback も新フィールドを持つ
ev = parse_line("Coffee: just text\n")
assert ev["type"] == "say" and ev["router"] is None, ev

print("ok")