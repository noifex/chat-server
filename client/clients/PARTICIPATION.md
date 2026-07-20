# 参加ループ（共有エンジン・全 persona 共通）

このファイルは各 persona の `CLAUDE.md` から `@../PARTICIPATION.md` で取り込まれる**共通の動き方**。
「君は誰か」「人格」「役割」は各 persona 側の CLAUDE.md にある。ここは**手順だけ**。
以下 `<自分>` は君自身の名前（CLAUDE.md 冒頭「君は◯◯」の◯◯）に読み替える。

## 前提（daemon は chat.sh が起動済み）

`./chat.sh start` で server と自分の daemon（パイプ）は既に動いている。
**自分で daemon を起動しないこと**（二重接続になる）。君の仕事はこの参加ループ（脳）だけ。

## ループ（`/loop 10s` で回す）

1. `bash ../../tick.sh <自分>` を実行。出力で分岐：
   - `STOP` → **/loop を終了**（会話 DONE。もう次を回さない）
   - `SKIP (turn=...)` → 自分の番じゃない。**何もせず次 tick へ**（新着も読まない＝思考ゼロ）
   - `YOUR TURN. new:` ＋新着 → 下へ進む
2. 新着のうち無視: `<自分>:` 始まり（自分の発言の戻り）/ `joined` / `left`
3. **board で仕事を1歩進める**（YOUR TURN の時だけ／該当が無ければスキップ）。**成果物はコード＝`../../workspace/` に書く。chat は要約だけ**:
   a. `bash ../../board.sh project` で全 task の `state`/`owner`/`desc` を見る
   b. 上から順に**該当する最初の1つだけ**実行:
      - **他人が owner の `"state":"review"`** → 君はレビュアー。`cat ../../workspace/task<id>.*` で**コードを読み**、判断:
        - 正しく desc を満たす → `bash ../../board.sh approve <id> <自分>`
        - バグ/穴/要件漏れ → `bash ../../board.sh changes <id> <自分>`（本文で**具体的に**何がダメか）
      - **自分が owner の `"state":"working"`**（差し戻し or 未提出）→ `../../workspace/task<id>.*` を **Edit で直し**、`bash ../../board.sh review <id> <自分> <token>`（token は a の `active_fencing_token`）
      - **`"state":"proposed"` があり自分に active task 無** → 1つ担当:
        - `bash ../../board.sh claim <id> <自分>` → `fencing_token` を控える
        - `bash ../../board.sh start  <id> <自分> <token>`
        - **`../../workspace/task<id>.<拡張子>` に desc を解くコードを Write**（言語は依頼に従う。例: 素数→`task<id>.py`）
        - `bash ../../board.sh review <id> <自分> <token>`（レビュー要求で**止める**。自分で approve しない＝他人が見る）
        - ※ claim が `rejected` なら他人が先取り。別の proposed か会話へ
      - **board に無いが今やるべき仕事**（user1 の依頼・議論で確定した次の一手）→ **自分で** `bash ../../board.sh add "<desc>"` で task 化（`@誰かに頼まない`＝喋りで済ませない）。同趣旨の task が既にあるなら足さない。追加後は次 turn で誰かが claim する
      - 該当なし → board は触らず会話だけ
4. **1発言で要約**（turn=自分＝喋る契約。黙ると止まる）。コードは workspace にあるので chat は「何をした/どう直す/どこがダメ」だけ:
   - `bash ../../say.sh say "<要約> @<相手>"`（board を触ったら `--task <id>` を付ける）
   - 次に振りたい相手を本文に `@名前` で書く（orchestrator が拾って指名）
   - **終端権限を持つ persona だけ**：全 task が done ＆会話も収束したら `bash ../../say.sh done "<まとめ>"` で締める（型で判定＝本文に `DONE` と書いても閉じない）

## 発言規律

- **誰が喋るかは `../turn`（orchestrator）が決める**。自分は turn が来た時だけ動く。「直近喋ったか」等は気にしない
- 自分の役割に徹する。相槌・長文で場を独占しない
- 次に振りたい人を本文に `@名前` で書く

## 鉄則

- 本文に `<自分>:` を付けない（server が接続名で自動付与）
- 本文に `$(...)` やバッククォートを書かない（shell で実行されて承認プロンプトが出る）
- 1発言1-2文。長文で場を独占しない
- **出力は15行以内**。成果物（コード）は `../../workspace/` のファイルに書く＝response に貼らない。説明が15行を超えるなら `../../workspace/<topic>.md` に書き、chat は「書いた・ファイル名・要約」だけ
