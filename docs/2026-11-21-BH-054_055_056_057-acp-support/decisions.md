# 設計の判断とその理由

本書は、`benitoiteh acp`（BH-054〜BH-057）の設計と実装プランを定めるにあたって行った判断（D1〜D8）、検討した代替案と却下理由、および 2 回の外部レビュー（ChatGPT Pro、[記録](../design/08-appendix/reviews/2026-11-21-chatgpt-pro/README.md)）の指摘に対する処置をまとめる。判断の基準は [00-01「目的と設計原則」](../design/00-overview/00-01-goals.md) と [03-01「セキュリティモデル」](../design/03-safety/03-01-security-model.md) である。

## 未決事項（BH-054〜BH-057）の判断の一覧

| 番号 | 論点 | 採った判断 | 状態 |
|---|---|---|---|
| `BH-054` | 道具とコマンドの実行を `benitoiteh` に残すか、クライアントの `fs/*`・`terminal/*` に委ねるか | `fs/*` と `terminal/*` は一切呼ばず、すべてのファイルの操作・処理系の呼び出し・コマンドの実行を `benitoiteh` の `safe_fs` と OS のサンドボックス（Seatbelt / bubblewrap + Landlock + seccomp）で行う（D1） | 決定（2026-11-21） |
| `BH-055` | T3Code の `provider-acp` に対する `benitoiteh acp` の実機検証 | 設計で固定した一次資料（T3Code `d5f8a7573a3817d8c20f7491e7672443355c4083`）との照合を終え、実機で確かめる 4 項目を [05-03](../design/05-interface/05-03-acp.md#確かめることbh-055) と P5-56 に定めた | 実装後に P5-56 で検証 |
| `BH-056` | ACP のクレートを依存に加えるか、`serde_json` で自前で型を定義するか | 新しい依存クレートは加えず、`serde` と `serde_json`、標準の OS スレッド、`std::sync::mpsc` で ACP 版 1 のメッセージ型と通信層を実装する（D2） | 決定（2026-11-21） |
| `BH-057` | ACP での通信の中継（`network = "proxy"`）の確認と、`secrets.askpass` の扱い | 通信の中継の確認は `AcpApprover` を通して `session/request_permission` で尋ね、`ProxyRun` の終了と確認応答を直列化する。`secrets.askpass` は ACP に秘密の単発入力メソッドがないため常に無効とする（D6・D7） | 決定（2026-11-21） |

---

## D1 道具と OS サンドボックスを `benitoiteh` に残し、クライアントの `fs/*`・`terminal/*` と MCP を使わない（BH-054、第 1 回指摘 01・10）

### 判断

1. `initialize` でクライアントが `fs.readTextFile`・`fs.writeTextFile`・`terminal` の能力を宣伝しても、`benitoiteh` はクライアント側の `fs/*` と `terminal/*` を一切呼ばない。すべての道具（`read`・`write`・`edit`・`glob`・`grep`・`benitoite_*`・`bash`・`web_fetch`）を `benitoiteh` 自身の `safe_fs` と OS のサンドボックスの中で実行する。
2. `session/new` と `session/load` の `params.mcpServers` は ACP 版 1 の必須欄として受け取るが、どの MCP サーバにも接続しない。`mcpServers` が非空のときは、無視した件数だけを一度だけ標準エラー出力に診断として出し、`env`・HTTP ヘッダ・URL・コマンド引数は標準エラー出力にも `session.jsonl` にも一切書かない。
3. T3Code がエージェントの起動環境へ注入する MCP 接続・制御用の環境変数（`T3_ACP_MCP_ENDPOINT`・`T3_ACP_MCP_AUTHORIZATION`・`T3_ACP_MCP_NODE`・`T3_ACP_MCP_ENTRYPOINT`・`T3CODE_THREAD_ID` など）を遮断するため、`T3_ACP_MCP_` または `T3CODE_`（ASCII の大文字と小文字を区別しない）で始まる環境変数を、提供者の API キーと同じく、環境の規則（`[environment] rules`）を当てた後に子プロセスの環境から必ず除く。また、`[environment] rules` で `*` を含まない `allow` や `set` にこれらの名前を書くことも設定の誤りとして拒む。

### 検討した代替案と却下理由

| 案 | 内容 | 却下または採用の理由 |
|---|---|---|
| A（採用） | `fs/*`・`terminal/*`・MCP を使わず、すべて `benitoiteh` 内で実行し、起動環境の `T3_ACP_MCP_*`・`T3CODE_*` も子プロセスから強制除外する | 制限モードの二重の防御（処理系のエフェクト検査 + OS のサンドボックス）と通常モードの `safe_fs`・Seatbelt/bubblewrap・通信中継の保証を、TUI・`exec`・`acp` で完全に一致させられる |
| B（却下） | ファイル操作やコマンド実行を ACP の `fs/*`・`terminal/*` でクライアント（T3Code）に委譲する | クライアント側は生パスでファイルを開き、サンドボックスなしでシェルを起動するため、`safe_fs`（シンボリックリンクによる範囲外脱出の阻止、秘密ファイル・`.git`・`.agents` の保護）と OS サンドボックスの保証がすべて失われる |
| C（却下） | `params.mcpServers` だけを無視し、起動環境は既存の `[environment] rules`（既定 `allow *`）に任せる | 第 1 回レビュー指摘 01 のとおり、T3Code の `provider-acp` は起動環境に `T3_ACP_MCP_ENDPOINT` と `T3_ACP_MCP_AUTHORIZATION`（Bearer トークン）を渡す。制限モードで `Process.Environment` と `Http.Connect` が許されると、スクリプトがこのトークンを読んで T3Code 本体の MCP HTTP サーバ（worktree 作成や setup script 実行を持つ）を直接呼び出し、サンドボックス外の操作を行わせる抜け道が残る |

---

## D2 新しい依存クレートや非同期ランタイムを加えず、`serde_json` と OS スレッドで ACP 版 1 を実装する（BH-056、第 1 回指摘 11、第 2 回 R05）

### 判断

1. `agent-client-protocol`（`0.11.0`）および `agent-client-protocol-schema`（`0.12.0`）は依存に加えず、既存の `serde` と `serde_json` で ACP 版 1 のメッセージ型だけを `crates/benitoiteh/src/cli/acp.rs`（および `src/ui/acp/`）に定義する。
2. 対応するプロトコルの版は **ACP 版 1（`protocolVersion: 1`）** に固定する。クライアントが `protocolVersion: 2`（またはそれ以上）を送ってきたときも、`result.protocolVersion: 1` を返して版 1 に交渉する。版 1 の範囲にある `session/new`・`session/load`・`configOptions`・`session/set_config_option`・`usage_update` のみを実装し、ドラフトの `session/list`・`session/resume`・`session/fork`・`session/close`・計画外の確認入力などは実装しない。

### 検討した代替案と却下理由

| 案 | 内容 | 却下または採用の理由 |
|---|---|---|
| A（採用） | 依存クレートを足さず、`serde_json` と標準の OS スレッド・`std::sync::mpsc` で ACP 版 1 の必要な型だけを定義する | 最小構成の原則（00-02、06-01）とリリースの実行ファイル 8 MiB 以下（06-02）を守れる。既存の `ui/tui/worker.rs` や `InteractiveApprover` と同じ同期チャネルの設計で統一できる |
| B（却下） | 公式の `agent-client-protocol` クレートを導入する | 公式 SDK（`8ac65cf6b0b3371ae4d8231760cb91dad99e92ef`）の `Cargo.toml` を確認したところ、`tokio` は `dev-dependencies` のみであるものの、通常の依存に `futures`・`async-trait`・`derive_more` などの非同期抽象を含んでおり、非同期ランタイムを持たない `benitoiteh` の設計方針と合わない |
| C（却下） | `agent-client-protocol-schema` クレートだけを導入する | `benitoiteh` が使わない多数の型の生成コードがバイナリサイズとビルド時間を増やし、T3Code の承認カード表示に必要な `rawInput` の細かな制御も自前の型定義のほうが明確に扱える |

---

## D3 T3Code の承認カードで全事実を確実に表示し、クライアント側の自動承認を防ぐ（第 1 回指摘 02・03、第 2 回 R06）

### 判断

T3Code（`d5f8a7573a3817d8c20f7491e7672443355c4083`）の `packages/provider-acp` と `apps/web` の実装を照合した結果、次の 3 点を `session/request_permission` の契約とする。

1. **`toolCall.rawInput.command` と `toolCall.content` の両方への全事実の格納**:
   - T3Code の `parsePermissionRequest`（`packages/provider-acp/src/acp/normalize.ts`）と `deriveToolActivityPresentation`（`apps/web/src/lib/toolActivityPresentation.ts`）は、承認カードの案内文（`<code>` ブロック）に `toolCall.command`（`rawInput.command` から抽出）を最優先で使い、`rawInput.command` が存在すると `title` や `detail`（`content` のテキスト）より優先してそれだけをカードに表示する。
   - したがって、`benitoiteh` が検査・組み立てた確認の事実（題、対象のパス、エフェクトの一覧と説明、`unread_imports`、サンドボックスの書ける場所と通信の有無、`.agents` への書き込み警告、8 文字以上の空白を `␠×N` にし制御文字を可視化したコマンドの全文）を **1 つの改行区切りの文字列** にまとめ、`toolCall.rawInput.command` と `toolCall.content` の両方に同じ全文を入れる。`toolCall.title` には 1 行の短い要約を入れる。
2. **LLM の `justification` の除外**:
   - LLM が自己申告した `justification`（理由）を承認カードの先頭や同じ欄に混ぜると、長い `justification` によって本当のコマンドやエフェクトが画面の下へ押し出される偽装が起きるため、`session/request_permission` の `toolCall` には `justification` を一切含めない。
3. **`toolCall.kind` の制限と T3Code 側の `Permission / access` 設定**:
   - T3Code の `shouldAutoApprovePermission`（`packages/provider-acp/src/server/clientPolicy.ts`）は、`runtimeMode` が `"approval-required"`（Web UI の `Permission / access: Supervised`）であっても、`toolKind` が `"read"`・`"search"`・`"think"` の確認要求を利用者に尋ねずに自動承認する。また、T3Code の既定の `Full access`（`runtimeMode: "full-access"`）ではすべての `session/request_permission` を無条件に自動承認する。
   - そのため、`benitoiteh` が送る `session/request_permission` の `toolCall.kind` には `"execute"`（`Effects`・`TestEffects`・`Command`・`Escalate`）、`"edit"`（`WriteScope`・`CreateProject`）、`"fetch"`（`WebFetch`・`Network`）の 3 種類だけを使い、`"read"`・`"search"`・`"think"` は一切使わない。あわせて、T3Code で確認を効かせるには `Permission / access` を **`Supervised`** に設定する必要があることを設計書に明記した。

---

## D4 承認の選択肢（`allow_once` と `allow_always`）と、呼び出し単位の一回限りのエフェクト許可（第 1 回指摘 06、第 2 回 R01）

### 判断

1. **T3Code のボタンの割り当てに合わせた `options`**:
   - T3Code の承認カード（`ChatView.tsx`）は、`Approve` ボタンで `kind === "allow_once"` の選択肢だけを探し、`Always allow this session` ボタンで `kind === "allow_always"`（なければ `"allow_once"`）を探す。`"allow_once"` を提示しないと `Approve` ボタンが押せなくなる。
   - そこで、確認の種類ごとに提示する `options` と選ばれたときの記憶の範囲を次のように定める。
     - **通常の制限モードのエフェクト確認**（`Effects`・`TestEffects` で `unread_imports == 0` かつ `agents_write == false`）と **通常モードのドメイン・コマンド確認**（`WebFetch`・`Network`、`Command` で `agents_write == false`）:
       - `allow-once`（`kind: "allow_once"`）: **その 1 回の呼び出しだけ** を許可する。セッションの `confirmed_effects` やドメイン・コマンドの許可集合を更新しない（既存のセッション許可があるときは、それを消すのでも広げるのでもなく、そのまま保つ）。
       - `allow-session`（`kind: "allow_always"`）: このセッションの間、そのエフェクト集合・ドメイン・コマンド接頭辞を記憶する。
       - `reject-once`（`kind: "reject_once"`）: 拒否する。
     - **一回限りの確認**（`Escalate`、`WriteScope`、`CreateProject`、および `agents_write == true` または `unread_imports > 0` を伴う確認）:
       - `allow-once`（`kind: "allow_once"`）と `reject-once`（`kind: "reject_once"`）の 2 つだけを提示し、`allow-session` は出さない。T3Code で利用者が `Always allow this session` を押して `allow_once` にフォールバックされたときも、`benitoiteh` はその 1 回の操作だけを許可し、セッションには一切記憶しない。
2. **呼び出し単位の承認済みエフェクト集合（`Invocation.call_approved_effects`）の導入**:
   - 既存の `benitoite_run`（`benitoite_scratch` も通る）と `benitoite_test` は、承認後の `execute_approved` の中で実行直前の再検査を行い、エフェクトが `Invocation.confirmed_effects`（セッションの記憶）に収まらなければ `EFFECTS_CHANGED` で拒む。
   - したがって、`allow-once` で `remember_effects` を呼ばないだけでは 1 回目の実行自体が `EFFECTS_CHANGED` で失敗する。これを解くため、`Invocation` にその呼び出し 1 回限りに承認されたエフェクト集合 `call_approved_effects: Option<&'a BTreeSet<Effect>>` を加え、実行直前の再検査では「セッションの `confirmed_effects` に収まる」か「`call_approved_effects` に収まる」ことを確かめる。これにより、実行直前のエフェクト増加（TOCTOU）を防ぐ検査を保ったまま、1 回目は実行でき、2 回目は再び確認になり、既存のセッション許可も壊さない。
3. **未提示の `optionId` と遅延・重複応答の拒否**:
   - 保留中の要求 ID に対してその要求で提示していない `optionId`（例: 一回限りの確認に対する `"allow-session"`）や不正な応答が返ったときは、その確認を直ちに `Denied` として完了させる。すでに完了・取消済みの要求 ID に遅れて届いた応答は無視し、確定済みの結果もセッションの許可集合も変更しない。

---

## D5 起動・セッション準備の方針の 5 分離と、`--allow` を超える実行の確認（第 1 回指摘 08、第 2 回 R02）

### 判断

1. これまで `ExecArgs.interactive` の単一の真偽値で切り替えていた準備処理を、次の 5 つの独立した方針に分離する。
   - **起動・再開時の確認**（未信頼プロジェクトの `.agents`、処理系の SHA-256 変化、モード変更の再開）: `Tui` のみ画面で確認し、`Exec` と `Acp` は保存済みの信頼または `--trust-project` に従う（ACP の `initialize` より前や `session/new`・`session/load` の最中には `session/request_permission` を送らない）。
   - **実行時の承認の方針と受け手**: `Tui` は `InteractiveApprover`、`Exec` は `Approval::Never`、`Acp` は `AcpApprover`（制限モードは `--approval` で `on-request` または `never`、既定 `on-request`。制限モードでの `--approval untrusted` は引数の誤り。通常モードは `--approval` または設定の `normal.approval`、既定 `on-request`）。
   - **事前許可のオプション**（`--allow`・`--allow-agents-write`・`--project-parent`・`--ephemeral`）: `Exec` と `Acp` の両方で受け付け、`Tui` では使い方の誤りとする。
   - **通信の中継（`normal.network = "proxy"`）の未許可ドメイン確認**: `Tui` と `Acp` で承認の方針が `never` でないときに有効にする。
   - **秘密の入力（`secrets.askpass`）**: `Tui` のみ有効とし、`Exec` と `Acp` では常に無効とする。
2. **`acp` で `--allow` 等を指定したときの扱い**:
   - `benitoiteh acp` で `--allow` を指定したとき、宣言したエフェクトが `--allow`（一時スクリプトでは確認なしの範囲との和）に収まり、`unread_imports == 0` かつ `.agents` への書き込みがなければ、`session/request_permission` を出さずに実行する。
   - `--allow` の範囲を超えるエフェクト、`unread_imports > 0`、または `--allow-agents-write` がない状態での `.agents` への書き込みがあるときは、承認の方針が `on-request`（既定）であれば `AcpApprover` で `session/request_permission` を送り、承認の方針が `never`（`--approval never`）であれば確認を送らずに拒否する（3 回続けて拒否されたら `EffectsNotAllowed` でその依頼を止める）。

---

## D6 通信の中継（`ProxyRun`）の終了と確認応答の直列化（BH-057、第 1 回指摘 04）

### 判断

1. `Agent` に `connect_network_prompts(&self, prompts: Sender<ApprovalPrompt>, wake: Option<WakeCallback>)` を設け、`InteractiveApprover` と `AcpApprover` の両方から通常モードの `NetworkGate` の確認チャネルを接続できるようにする。
2. `crates/benitoiteh/src/sandbox/proxy.rs` の `ProxyRun` で、コマンドの終了（`ProxyRun::finish`）と確認の応答の受領・許可集合の更新（`ProxyRun::decide`）を、**同じ `Mutex` のもとで直列化**する。
   - `ProxyRun::finish` はその錠を取って `finished = true` にする。
   - `ProxyRun::decide` が `response.recv_timeout(...)` から返ったときは、その錠を取って `finished` を確かめ、すでに `finished == true` であれば受け取った応答を捨てて `Err(Denied::NotAllowed)` を返し、`self.allowed` も `self.gate.session`（セッション全体のドメイン許可）も `self.declined` も一切更新しない。
   - 錠を持ったまま確認の返事待ちやネットワーク I/O を行わず、複数の錠を取る場合は常に `ProxyRun` の状態の錠 → `NetworkGate.session` の錠の順に取る。

### 理由

既存の `proxy.rs` では、`ProxyRun::finish` が `AtomicBool` の `finished` を `true` にする処理と、`decide` が `response.recv_timeout` の直後に `gate.session.lock().insert(...)` を呼ぶ処理が直列化されていなかった。そのため、コマンドのタイムアウトや取消の直後に遅れて `"allow-session"`（`ApprovedForSession`）が届くと、終わったコマンドの確認応答によって以後のセッションのドメイン許可が勝手に広がる競合があった（第 1 回指摘 04）。これを同じ錠で直列化して閉じる。

---

## D7 非ブロッキング出力キューと、取り消し・EOF・出力障害の 5 段の終了手順（第 1 回指摘 05・07、第 2 回 R03）

### 判断

1. **3 スレッド構成と非ブロッキングの有界出力キュー**:
   - **入力の読み取りのスレッド**: `stdin` から 1 行ずつ（上限 16 MiB）読み、主のスレッドの受信チャネルへ送る。
   - **主のスレッド**: セッションの状態、アクティブな `sessionId`、一連の処理の世代、および保留中の確認の台帳（`request_id -> PendingPermission`）を単一の持ち主として管理する。
   - **出力の書き込みのスレッド**: 容量 256 行の有界チャネルから JSON の行を受け取り、`stdout` に書いて 1 行ごとに `flush` する。主のスレッドからは **非ブロッキングの `try_send`** で送り、書き込み・`flush` の失敗、または出力キューの満杯（クライアントが `stdout` を読まずに詰まらせた場合）を検知したら `output_failed: Arc<AtomicBool>` を立てて即座に出力の障害の終了手順へ移る。これにより、主のスレッドが `stdout` 待ちでブロックして `session/cancel` や `AgentEvent` を処理できなくなるデッドロックを防ぐ。
2. **5 段の終了手順**:
   `session/cancel` の受信、`stdin` の EOF、または `output_failed` の検知のどれが起きたときも、主のスレッドは次の 5 段を順に実行する。
   1. 新しい要求の受付を止める。
   2. `RunControl` の `cancel_step` と `cancel_run` を `true` にし、確認の台帳にある保留中の全確認（通常の道具の確認と、`NetworkGate` の通信確認の両方）の返事チャネルへ即座に `ApprovalOutcome::Cancelled` を送って台帳を空にする。
   3. エージェントのスレッドが終わるまで、有界の `AgentEvent` チャネル（容量 64）から残りのイベントを抜き続ける（ここで抜くのを止めると、エージェントのスレッドが `events.send(...)` でブロックして終了できなくなる）。
   4. `session/cancel` による取り消しで接続が生きているときは、最後の `session/update` を出力キューへ積んだ後に、その `session/prompt` の要求 ID に対する `result: { stopReason: "cancelled" }` を一度だけ積む。
   5. EOF または出力の障害でプロセスを終えるときは、エージェントのスレッドの終了（子プロセスの停止と一時ディレクトリの掃除）を待ってからプロセスを終える。クライアントが `stdin` を閉じずに放置している入力スレッドや、詰まったパイプで停止している出力スレッドを無期限に `join` しない。

---

## D8 `session/cancel` の通知・要求の区別、最終応答の一元化、入力検証（第 1 回指摘 09、第 2 回 R04）

### 判断

1. **`session/cancel` の通知と要求の扱い**:
   - ACP の仕様上、`session/cancel` は JSON-RPC の **通知（`id` なし）** である。非アクティブまたは未知の `sessionId` に対する正規の通知 `session/cancel` は、JSON-RPC の規則どおり応答を返さずに無視する。
   - クライアントが誤って `id` 付きの要求として `session/cancel` を送ってきたときは、アクティブな `sessionId` なら取り消しを発動して `result: null` を返し、非アクティブまたは未知の `sessionId` なら `-32602`（`Invalid params`）を返す。
2. **`session/prompt` の最終応答の一元化と `AgentEvent::Error`**:
   - `AgentEvent::Error` は、ループの途中で起きる致命的でない警告（`storage_limit` や `compaction_failed`）でも送られる。そのため、`AgentEvent::Error` を受け取った時点では `session/prompt` の JSON-RPC 応答を返さず、標準エラー出力に診断として記録するだけにする。
   - `session/prompt` に対する最終応答（`result: { stopReason }` または JSON-RPC の `error`）は、エージェントのスレッドが終わり残りの `AgentEvent` をすべて回収した後に、`RunReport.stop_reason`（`StopReason`）とその実行の取消状態から **一か所で一度だけ** 決めて返す。
3. **入力検証とセッションの隔離**:
   - `session/new` と `session/load` の `params.cwd` は絶対パスであることを必須とし、欠落・空・相対パスは `-32602` で拒む。`session/load` では記録されたプロジェクトの実のパスと `cwd` の実のパスが一致しなければ拒み、過去の記録から承認済み状態を復元しない。
   - `session/prompt` の `ContentBlock` は `text`・`resource_link`・`resource`（テキスト）・`image`（`image/png` と `image/jpeg` の base64、`benitoiteh __image` 補助プロセスで検証・縮小）だけを受け付け、`audio`・`resource.blob`・未知の `type`・上限超過は `-32602` で拒んで提供者へ一切送らない。

---

## 外部レビュー（第 1 回・第 2 回）の指摘と処置の対応表

| 指摘番号 | 指摘の要点 | 反映した判断・文書・作業 |
|---|---|---|
| 第 1 回 01 | `mcpServers` を無視しても、起動環境から `T3_ACP_MCP_*` の資格情報が子プロセスへ渡る | D1、設計書 03-02・05-01・05-03、作業 **P5-53**・**P5-56** |
| 第 1 回 02 | `ApprovalFacts` の主要項目が T3Code の承認カードで表示されない（`rawInput.command` 優先、`justification` 混入） | D3、設計書 05-03、作業 **P5-55** |
| 第 1 回 03 | T3Code 側の自動承認（`Full access` 既定、`Supervised` でも `read`・`search`・`think` を自動承認） | D3、設計書 05-03、作業 **P5-55** |
| 第 1 回 04 | `ProxyRun::finish` の後に届いた確認応答が `NetworkGate.session` を更新する競合 | D6、設計書 03-02・05-03、作業 **P5-53**・**P5-56** |
| 第 1 回 05 | 取消・EOF・壊れたパイプと、有界チャネル・確認待ちが重なると停止しうる | D7、設計書 05-03、作業 **P5-55**・**P5-56** |
| 第 1 回 06 | 承認の記憶範囲（一回限りかセッション記憶か）と選択肢 `options` の対応が未定義 | D4、設計書 03-03・05-03、作業 **P5-54**・**P5-55** |
| 第 1 回 07 | アクティブなセッションの単一性と、確認応答の相関（セッション・世代・要求 ID）が未定義 | D4・D7、設計書 05-03、作業 **P5-55** |
| 第 1 回 08 | `ExecArgs.interactive` の単一フラグでは `acp` の準備方針を表せない | D5、設計書 05-01・05-03、作業 **P5-54** |
| 第 1 回 09 | `cwd`（絶対パス）、`ContentBlock` の上限・未知型、添付画像の検証・縮小が未定義 | D8、設計書 05-03、作業 **P5-54**・**P5-55** |
| 第 1 回 10 | `session/new`・`session/load` の `mcpServers` を無視するときの資格情報の秘匿 | D1、設計書 05-03、作業 **P5-55**・**P5-56** |
| 第 1 回 11 | 公式 Rust SDK（`agent-client-protocol`）の `tokio` 依存の説明が不正確 | D2、設計書 05-03・open-issues（`BH-056`）修正済み |
| 第 1 回 12 | `BH-054`〜`BH-057` の決定状況が `open-issues.md` と `05-03-acp.md` で未整理 | 本書冒頭の表、設計書 05-03・open-issues 更新済み |
| 第 2 回 R01 | 03-03 と 05-03 のエフェクト記憶規則の整合と、1 回目の `allow_once` を通す `Invocation.call_approved_effects` | D4、設計書 03-03・05-03、作業 **P5-54**・**P5-55** |
| 第 2 回 R02 | `--allow` を超える実行を尋ねるか拒むかの準備方針の表内の整合 | D5、設計書 05-01・05-03、作業 **P5-54** |
| 第 2 回 R03 | 標準出力が詰まったときの非ブロッキング出力キューと、停止したスレッドを無期限に `join` しない終了条件 | D7、設計書 05-03、作業 **P5-55** |
| 第 2 回 R04 | 通知 `session/cancel` の非アクティブ `sessionId` の無応答無視と、`AgentEvent::Error` と最終応答の一元化 | D8、設計書 05-03、作業 **P5-54**・**P5-55** |
| 第 2 回 R05 | 対応する ACP の版を版 1（`protocolVersion: 1`）に固定し、ドラフト機能を除外する明記 | D2、設計書 05-03、作業 **P5-55** |
| 第 2 回 R06 | T3Code で `rawInput.command` が `title` や `content` より優先されるため、`rawInput.command` と `content` の両方に全事実を入れる | D3、設計書 05-03、作業 **P5-55** |
