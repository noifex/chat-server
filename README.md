# chat-system

Rust の `std::net` だけで書いた TCP chat server の上に、**複数の LLM セッション（Claude）を対等な peer として喋らせ、共有タスクボードで協調させて実際のコードを書かせる**マルチエージェント基盤。The Book Ch16（並行）の素振りとして始まり、生 TCP → JSON 行プロトコル → 自律 agent 協調 → イベントソーシングな task board（WAL・fencing・独立レビュー）まで、framework を使わず素手で拡張したもの。

> ⚠️ **学習用プロジェクト**。production 用途ではない。未実装・未接続の部分あり（下記「Status」「Known issues」）。

## How this was built

Claude Code（Anthropic）を主コーディング、OpenAI codex を独立レビューに使った **AI 支援プロジェクト**。コード（Rust / Python / bash）は大半が AI 生成で、著者の貢献は**システム設計・アーキテクチャ判断・問題分解・正しさの検証**。設計判断とその理由は [DESIGN.md](./DESIGN.md) に記載。

## 目的

- **並行/分散システムの硬いコアを素手で再実装して学ぶ**：event sourcing・WAL + crash recovery・fencing token・独立レビューを、ライブラリに頼らず自分で組む（saga／補償トランザクションは**設計のみ・未実装**、Known issues 参照）
- **多エージェント協調の実験場**：同質モデルの多数決は consistency 機構にすぎず精度に効かない。反証になるのは**異種モデル**（Claude ↔ OpenAI）だけ、という仮説の検証基盤（異種 peer 統合自体は**未完**）
- **「会話する AI」→「タスクを実行する agent」**：persona は雑談でなく、board 上の task を claim し、`workspace/` に実コードを書き、互いにコードレビューして done まで回す

## 3つの部品と関係

```
                 ┌─────────────────────────────────────────────┐
                 │  client/  (Python + shell)                  │
                 │                                             │
   persona ⇄ ⇄ ⇄ │  Coffee / Cola / Tea  (Claude, sonnet)      │
   (Claude Code   │  orchestrator.py  = 話者選択 (turn)          │
    セッション)   │  say.sh / tick.sh = chat 送受信の薄いラッパ  │
                 │  board.sh         = board CLI の薄いラッパ   │
                 │  workspace/       = 成果物(コード)の置き場    │
                 └───────┬─────────────────────────┬───────────┘
                         │ JSON行 broadcast         │ claim/review/approve
                         ▼                          ▼
              ┌────────────────────┐    ┌────────────────────────┐
              │  server/  (Rust)   │    │  board/  (Rust)         │
              │  = 土管 dumb pipe   │    │  = 真実 task WAL         │
              │  peer↔peer に JSON  │    │  event sourcing +       │
              │  行を中継するだけ   │    │  fencing + 独立レビュー  │
              │  task を知らない    │    │  proposed→…→done         │
              └────────────────────┘    └────────────────────────┘
```

（Codex＝OpenAI の異種 peer は `clients/Codex/` に**実験スクリプトのみ**。chat.sh / orchestrator には未登録＝現状 chat には参加しない。Known issues 参照）

| dir | 言語 | 役割 | 原則 |
|---|---|---|---|
| **server/** | Rust | 全 peer に JSON 行を同報する **dumb pipe（土管）**。`thread per connection` + `mpsc` 集約、履歴は 30 行 ring buffer。相手が誰かも task も知らない | 運搬に徹する＝中央ゲートキーパーにしない |
| **board/** | Rust | task の**単一真実**。WAL を真実とし状態は replay で再生（event sourcing）。`proposed→claimed→working→review→done` ＋ fencing token ＋**独立レビュー**（`approve`/`changes` は owner 以外のみ） | 単独 writer・機構が規律を代替する |
| **client/** | Python + shell | persona daemon（Claude Code セッション）＋ orchestrator（話者選択）＋ 薄いラッパ群。persona は chat で会話し、board で task を回し、`workspace/` にコードを書く | persona に精度を期待しない。効くのは**異種モデル** |

server は board を知らない（土管のまま）。board は誰が呼ぼうと「状態」しか見ない。**2つの独立した真実（chat の運搬・task の順序）を persona が繋ぐ。**

## タスク協調の流れ

persona は自分の turn（`orchestrator` が `client/clients/turn` で指定）が来たら board を1歩進める：

1. **新規着手**：空き task を `claim`→`start`→`workspace/task<id>.py` にコードを Write→`review` 要求（**自分では done しない**）
2. **レビュー**：他人が owner の `review` 状態の task があれば、`workspace/` のコードを読んで `approve`（→done）か `changes`（→差し戻し）
3. **差し戻し対応**：自分の task が `working`（差し戻された）なら直して `review` 再要求
4. **task 生成**：やるべき仕事が board に無ければ `board.sh add` で自分で task 化（依頼を寝かせない）

owner と reviewer は別人になる（fencing で強制）ので、**独立したレビュー判定**になる。ただし board の WAL に残るのは `approve`/`changes` の**判定イベント（task_id + 誰が）だけ**で、critique の本文は chat 側に出る（board には保存されない）。

## Protocol

生 TCP + UTF-8 行。JSON envelope 化済み：

```json
{"from":"Cola","model":"claude-sonnet-5","type":"say","reply_to":17,"task_id":2,"confidence":null,"text":"..."}
```

server は無改造（整形して撒く `name: text` のまま）。受信側が封筒を剥がす（transport 分離 → `json.loads`）。非 JSON 行は `{type:"say"}` に fallback＝人間の平文 `nc` も後方互換。parse は `protocol.py` に単一ソース化。

## セットアップと起動

前提: Rust toolchain（`cargo`）、Python 3、`claude`（Claude Code CLI）、`tmux`（任意・1画面運用時）。

```sh
# 1. Rust をビルド
( cd server && cargo build )
( cd board  && cargo build )

# 2. persona 権限 + workspace を用意
#    各 settings.local.json（.gitignore 済み＝machine 固有）を生成し、
#    workspace/ を隔離 git repo として初期化する
( cd client && ./setup.sh )

# 3. infra 起動（server build & 起動 + 各 daemon + orchestrator）
cd client
./chat.sh start
#   もしくは tmux で1画面に全部:  ./chat.sh tmux

# 4. 各 persona を参加させる（別ターミナルで）。claude 起動後、自分で /loop 10s を打つ
cd clients/Coffee && claude   # → プロンプトで  /loop 10s
cd clients/Cola   && claude   # → /loop 10s
cd clients/Tea    && claude   # → /loop 10s

# 人間として参加
nc 127.0.0.1 8080        # 平文で喋る（1行目=名前）
python3 human.py         # peer として参加

# 停止
./chat.sh stop
```

> ⚠️ `./chat.sh start` が起動するのは **daemon と orchestrator まで**。persona の思考ループ（`/loop 10s`）は各 `claude` セッションで**手で打つ**必要がある。

### 権限モデル

persona は `client/clients/<Name>/.claude/settings.local.json` で権限を持つ（`.gitignore` 済み、`setup.sh` が生成）。

- **`defaultMode: bypassPermissions`**：sandbox 内は承認ゼロで自律（codex の trust-the-boundary と同型）
- **`deny`**：`rm` / `sudo` / `curl` / `wget` / `git push` を硬くブロック
- **真の安全網は `client/workspace/` の git（revert）**：arbitrary なコード実行は deny を迂回しうるので、削除の最終防波堤は権限でなく workspace の版管理。`setup.sh` がこの workspace を git repo として初期化する

### persona（例と自作）

`clients/` の **Coffee / Cola / Tea は例**（提案・実装 / 批判・レビュー / まとめ・合意）。人格・役割は自由に書き換え・追加してよい。

- **参加ループ（board 手順・鉄則）は `clients/PARTICIPATION.md` に一元化**。各 persona の `CLAUDE.md` は `@../PARTICIPATION.md` で取り込むので、書くのは**人格 + 役割だけ**（ループを変えるのは1ファイルで済む）。
- **追加手順**：`clients/PERSONA.template.md` を `clients/<Name>/CLAUDE.md` にコピー → 人格/役割を埋める → `./setup.sh` で権限生成 → `cd clients/<Name> && claude` で参加。
- ⚠️ **`@../PARTICIPATION.md` は Claude Code の CLAUDE.md import 機能に依存**。これが効かないと persona は board 手順・`/loop` を知らず**ただの雑談 peer に退化**する。起動直後に「persona が board を触るか」で効いてるか確認すること。効かない環境では、`PARTICIPATION.md` の中身を各 `CLAUDE.md` に直接貼る fallback を使う。
- **Codex**（OpenAI）は現状 chat に**未接続**（`clients/Codex/` は実験スクリプトのみ）。Known issues 参照。

## Status（動くもの）

- chat bus：echo → broadcast → mpsc 集約 → Python client → Claude 複数体協調 → orchestrator 話者選択 → 自動 loop → history replay → reactive routing → event-driven wake（FIFO）→ JSON 行 protocol ＋ 人間 peer 化 → 権限基盤
- task board：`proposed→claimed→working→review→done`、flock 排他、torn-tail recover、fencing token（横取り無効化）、seq 連続チェック、**独立レビュー**（approve/changes）
- agent 化：作業＝`workspace/` にコード成果物、レビュー＝コードを読む、task の自己生成（intake）

## Known issues

- 🔴 **長時間稼働で token/context が劣化**：persona の `*.inbox` が無制限追記、`tick.sh` が未読を全量投入。compaction / token budget / max-round が未実装。長時間自律運用では context 溢れ・コスト爆発に落ちる
- 🟡 **異種 peer（Codex）は未接続**：`clients/Codex/` に実験スクリプトはあるが、`chat.sh` の起動対象・`orchestrator` の persona 集合いずれにも登録されておらず、`@Codex` も宛先として認識されない。現状は chat に参加しない
- 🟡 **`@import` 依存が検証不能**：persona の board 手順は `@../PARTICIPATION.md` 頼み。Claude Code の import が無効/仕様変更/相対解決失敗すると雑談 peer に退化する。起動時の自動検査・fallback は未整備
- 🟡 **話者選択がまだ debate 固定 FSM**：`orchestrator` は task 状態でなく PROPOSE→CRITIQUE→… の固定サイクル。board 駆動 routing（review 待ちを非 owner に確実へ振る）は未実装
- 🟡 **客観終端が未実装**：レビューは今のところ「コードを読む」主観。`cargo test`/実行 pass で done とする検証ベース終端は未実装。収束は Tea の宣言頼み
- 🟡 **identity/auth 無し**：board の `by` は audit 文字列で、名前 spoofing 可能。誰が approve/reclaim 可能かの制限は未実装
- 🟡 **durability の詰め残り**：mock harness による crash 注入検証（死験）、saga（git 補償）、行 CRC は未実装
- 🟡 **`./chat.sh stop` の kill が広範**：PID file に加えて `chat-server` / `client_daemon.py` / `orchestrator.py` を名前一致で `pkill` する。同名プロセスを走らせている他プロジェクトも巻き込みうる

## ディレクトリ構成

```
chat-system/
├── server/                 # Rust TCP chat server（土管）
│   └── src/
├── board/                  # Rust task board CLI（WAL/event sourcing/fencing/独立レビュー）
│   └── src/
└── client/                 # Python + shell
    ├── chat.sh             # infra launcher（start / tmux / stop）
    ├── client_daemon.py    # server ⇄ persona inbox ブリッジ
    ├── orchestrator.py     # 話者選択（決定的 Python・LLM は呼ばない）
    ├── protocol.py         # JSON 行 parse/build 単一ソース
    ├── tick.sh / say.sh    # persona が叩く固定 I/F（承認ゼロ）
    ├── board.sh            # board CLI ラッパ
    ├── check.sh            # workspace 内で cargo build/test（静的封印）
    ├── setup.sh            # settings.local.json 生成 + workspace を git init
    ├── human.py            # 人間 peer client
    ├── clients/
    │   ├── PARTICIPATION.md         # 共有エンジン（参加ループ・board手順・鉄則）
    │   ├── PERSONA.template.md      # 新 persona 用テンプレ
    │   ├── settings.example.json    # 権限テンプレ
    │   ├── Codex/                   # 異種 peer 実験スクリプト（chat 未接続）
    │   └── <Name>/CLAUDE.md          # persona 固有（人格 + 役割 + @PARTICIPATION.md）
    └── workspace/          # agent 成果物の隔離 git repo（setup.sh が作成・.gitignore 済み）
```
