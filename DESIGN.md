# 設計判断ログ

このドキュメントは chat-system の**設計判断とその理由**をまとめたもの。コード自体は大半を AI（Claude Code）が生成しているが、下記のアーキテクチャ判断・問題分解・正しさの検証は著者が行った。「何を作るか・なぜそう作るか」を残す。

## 責務分離：server は「土管」に徹する

chat server は JSON 行を全 peer に中継するだけで、task も persona も相手が誰かも知らない。中央に「賢い」ものを置くと単一障害点・ゲートキーパーになるため、**運搬に徹する dumb pipe** とした。結果、言語非依存のプロトコル（1行目=名前、以降=発言）だけで人間・Claude・任意プロセスが対等に繋がる。

## board は直列化multi-writer + event sourcing

「誰が何をやっているか」の単一真実を task board が持つ。状態を直接保存せず、**状態を変える"出来事"(event)を WAL に追記し、状態は毎回 replay で再生成**する（event sourcing）。書き手は複数のCLI processだが、`flock` で直列化し、reducerがeventを1件ずつ受理する。

## fencing が「正しさ」、lock は「整頓」

並行書き込みの制御を2層に分けた。

- **lock（flock）＝整頓**：1人ずつ書くための協調ロック。安価で、将来 leaderless 構成に差し替え可能。
- **fencing token ＝正しさ**：claim 時に整理番号（＝採番 event の seq）を発行し、以降その番号を持つ者だけが遷移できる。横取り（reclaim）は番号が増え、**古い番号は無効化**される。正しさをデータ側に埋め込むことで中央調停なしに担保する。

→ 「共有イベントログ（bus）」と「整合性」を両立できる。lock を強制ロックにすると中央ゲートキーパーが復活してしまうため採らなかった。

## 独立レビューを規律でなく機構で強制する

board が `approve`/`changes` に対し **`by ≠ author` を要求し、通常経路の自己承認を機械的に弾く**。これはfencingとは別の認可。「割り込むな」を persona へのお願いで縛るのではなく、状態機械で制限する。ただし `author` はscalarでcontributor履歴ではないため、reclaim-before-review後の旧contributor承認は未解決。

## crash recovery は範囲を明示して引く

末尾が千切れた書き込み（torn-tail）は切って回復し、中間破損は `poison` として拒否する。ただし**電源断（power-cut）耐性は `kill -9` では証明できない**（page cache が生き残るため）。本物の証明は Jepsen / CrashMonkey の領域なので、**「未証明」と明記して範囲外**とした。動く範囲と保証しない範囲を分けて言うことを、正しさそのものより重視した。

Git commit/revertによる補償workflowのhappy pathは実装済みだが、Git操作とWAL追記の間で落ちた状態を照合するintent/reconciliationは未実装。

## 異種モデルで自分を反証する

同質モデルの多数決は独立性を生みにくく、異種モデルは独立した第二意見を得る有力なlever、という仮説を持つ。実際、OpenAI の codex に本 README を実コードと突き合わせてレビューさせ、**著者自身が書いた誇張・不正確を7件検出して修正**した（例：「git による安全網」を主張しながら実装が伴っていなかった）。現在は FSM の `REBUT → AUDIT(Codex) → SYNTHESIZE` に組み込み、queue が続いても8発言でCodexを先頭へ積む。

Codexはturnごとにfresh/ephemeralなsessionを起動し、未読全量と直近80行から文脈を再構成する。短い監査にユーザー設定の高いreasoning effortを引き継いでcostを膨らませないよう、既定は`medium`に固定する。モデルにはread-only sandboxで `{text, confidence}` だけを返させ、trusted wrapperがschema・handoff禁止・confidence範囲を検査してからbusへ送る。モデル自身にFIFO書き込み権限を渡さないことで、異種性と実行権限を分離した。

## client の配送境界

`tick.sh` は表示したinbox範囲をpending cursorに記録するだけで、`say.sh`がdaemonのoutbox FIFOへ正常に書いた後に確定する。したがってモデル失敗やFIFO書き込み前crashは再試行できるが、保証はlocal handoffまでである。daemonがFIFOから読んだ後、TCP送信前に落ちる場合の消失や、handoff後cursor確定前の重複を除くには、message seq・end-to-end ack・dedupが別途必要になる。

JSON payloadの`from`は受信側でtransport prefixに上書きする。これにより同一接続内のpayload spoofingは防ぐが、接続時に申告する名前そのものは未認証であり、identity/authの代わりにはならない。

## 承認モデルの現実解

agent に実作業をさせると、コマンド単位の人手承認は摩擦で回らない。Claude Code の作業personaは許可境界の中で動く一方、批判だけを返すCodex peerはread-onlyに絞った。任意コード実行を持つpersonaでは削除拒否を迂回しうるため、**削除の最終防波堤は権限設定でなく `workspace/` の git（revert）**に置いた。

---

未実装・未解決（Git/WAL dual-write復旧／board 駆動 routing／identity など）は [README.md](./README.md) の「Known issues」に記載する。
