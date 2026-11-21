基本方針は維持できるが、現状のまま実装プランを確定するのは勧めない。 特に、T3Code が起動環境へ渡す MCP 用資格情報、承認画面に実際に表示される情報、クライアントの自動承認、取消と遅延した承認応答の扱いを先に詰める必要がある。

fs/*・terminal/* を使わず、道具とサンドボックスの実行を benitoiteh に残す方針、モード変更を拒む方針、提示していない optionId を拒否する方針は妥当である。ただし、これらだけでは閉じられない経路が T3Code との組み合わせで見つかった。

確認した範囲

参照したソースは次のコミットに固定した。

対象	確認したコミット

BenitoiteHarness	f04e5d9c2985c84da747491e17b14c548bef252e
ACP の仕様・スキーマ	5ce539f03d2750e7f58a3ffdfab6919c2d04445c
T3Code	d5f8a7573a3817d8c20f7491e7672443355c4083


以下は設計書とソースコードのレビューであり、T3Code を起動した統合試験や、指摘する経路の実行による再現は行っていない。未実装部分の設計不足と、流用予定の既存コードの問題を区別して記す。

AGENTS.md のレビュー観点には従った。ただし、そこで参照している japanese-tech-writing 本体は今回参照可能な資料・環境では見つからず、その細則は未確認である。

指摘

1. mcpServers を無視しても、起動環境から MCP の権限が子プロセスへ渡る

場所

- docs/design/05-interface/05-03-acp.md「信頼の境界と安全の原則」
- docs/design/03-safety/03-02-sandbox.md「子プロセスの環境」「規則の後に必ず当てる変更」
- crates/benitoiteh/src/config/environment.rs

重さ：実装の前に直すべき

問題点

T3Code は、session/new・session/load の mcpServers とは別に、ACP エージェントの起動環境にも次の値を渡す。

- T3_ACP_MCP_ENDPOINT
- T3_ACP_MCP_AUTHORIZATION
- MCP 呼び出し用プログラムに関する T3_ACP_MCP_NODE など

これは provider-acp の acpMcpContext が作り、provider-acp-registry の起動処理 がエージェントの環境へマージしている。

一方、現在の config/environment.rs は、既定で allow * に相当する環境を継承し、強制的に除くのは提供者のキーや自分自身の一部の制御用変数である。上記の T3Code 用変数は除かれない。

この組み合わせから、次の経路が残る。

1. 制限モードのスクリプトに Process.Environment と Http.Connect が許可される。
2. スクリプトが継承した MCP 用資格情報を取得する。
3. HTTP で T3Code の MCP 接続先を直接呼ぶ。
4. T3Code 本体に、benitoiteh の safe_fs とサンドボックスを通らない操作を行わせる。

制限モードで通信を許した場合、現在の設計は IP 通信全体を許す。通常モードのプロキシが持つ私的アドレスの拒否は、この直接通信の経路には掛からない。これは対象の 03-02「通信の中継」 にも明記されている。

T3Code 側の資格情報も、単なる識別情報ではない。McpSessionRegistry.ts は worktree などの権限を付けて発行し、McpHttpServer.ts が Bearer token として受け付ける。具体的には、WorktreeMcpService.ts に、T3Code 本体で worktree を作成し、条件が合えば設定済みの setup script を起動する処理がある。

成立には、資格情報の継承、HTTP 到達性、有効な T3Code のスレッド・プロジェクトなどの条件が要る。エンドツーエンドの成功は未検証である。 しかし、ハーネス外の操作へつながる各部分はソースで確認でき、設計上は閉じるべき経路である。

修正案

ACP クライアントが渡す制御用資格情報を、提供者のキーと同様に、LLM が動かす子プロセスへ渡さない対象に加える。少なくとも T3Code の MCP 用資格情報は、環境の規則を適用した後に強制除外し、allow * や set で復活させない。

あわせて、受信した mcpServers[*].env・HTTP headers・起動環境の資格情報を、診断やセッション記録へ丸ごと載せない規則を加える。「MCP に接続しない」規則には、資格情報を子へ渡さないことまで含める必要がある。


---

2. ApprovalFacts を送っても、T3Code の承認カードに全事実が表示されるとは限らない

場所

- docs/design/05-interface/05-03-acp.md「実行の前の確認」手順 1
- docs/design/03-safety/03-03-approval.md「確認の共通の規則」

重さ：実装の前に直すべき

問題点

草稿は「事実を組み立ててクライアントへ送ること」と「確認に必要な事実を利用者へ表示すること」を、ほぼ同じものとして扱っている。しかし、確認した T3Code の一般 ACP 経路では一致しない。

T3Code の parsePermissionRequest は、承認表示に使う説明を、おおむね次の優先順で選ぶ。

1. コマンド
2. 題
3. 詳細

さらに、題はその前に deriveToolActivityPresentation を通り、execute は Ran command、edit は Changed files などへ要約される。承認カードには、こうして選んだ parsed.detail が prompt として渡される。toolCall.content 全体をそのまま承認カードへ渡す処理ではない。該当するカードの組み立て

したがって、例えば次の形式でも十分ではない。

- rawInput.command にコマンドを入れる。
- toolCall.content に、サンドボックス外での実行、書ける場所、.agents の警告などを入れる。

この場合、承認欄にはコマンドだけが渡り、追加の事実が抜ける経路がある。長い title にすべてを詰めれば解決するともいえない。

修正案

設計の完了条件を、JSON に載せるところではなく、対象クライアントの承認画面で必要な事実を確認できるところまで広げる。

具体的には、次を定めるべきである。

- 検証済み事実を載せる ACP の欄。
- T3Code の対象版と、承認画面へその欄を渡す方法。
- コマンド全文、サンドボックス内外、書込範囲、エフェクト、.agents、読まないファイルの取り込みが欠けないこと。
- LLM の justification を検証済み事実と別に表示すること。区別できないクライアントでは、承認表示から justification を省くこと。

現行 T3Code への表示対応が必要になる可能性が高い。「TUI と同じ事実を送ったので、TUI と同じ確認を受けた」とは扱わないことが重要である。


---

3. request_permission の正常な応答は、人間がその都度確認した証拠にはならない

場所

- docs/design/05-interface/05-03-acp.md「信頼の境界と安全の原則」「実行の前の確認」
- docs/design/03-safety/03-03-approval.md「確認の共通の規則」

重さ：実装の前に直すべき

問題点

T3Code はクライアントの方針に応じて承認要求へ自動応答する。

clientPolicy.ts では、read・search・think は自動許可の対象であり、そのほかも承認方針、サンドボックスの指定、runtime mode によって自動許可される。例えば、サンドボックス指定のない auto-accept-edits では編集系を自動許可する一方、実行系は尋ねる。この区別があるため、「Auto ならすべて自動許可」と一括りにするのも正確ではない。

自動許可の場合、adapter.ts は確認画面を出さず selected を返す。選択肢は allow_once を優先し、なければ allow_always を選ぶ。選択処理

これは、利用者が選んだ T3Code の方針による動作であり、それ自体を LLM によるクライアント侵害と呼ぶべきではない。しかし、.agents への書き込みなどを「毎回、人間が確認する」という保証は、ACP の応答だけからは得られない。

また、03-03 は依然として、次を無条件に定めている。

- 確認は benitoiteh の画面で受ける。
- 全事実が端末に収まらなければ許可を受け付けない。
- 表示前の入力や直後のキーを捨てる。
- 標準入力が端末でなければ非対話として扱う。

ACP を認めるには、ここも整合させる必要がある。対象の共通規則

修正案

「クライアントを信頼する」に含める条件を具体化する。

毎回の人間の確認を維持するなら、T3Code の Supervised 相当の設定を対応条件にし、必要な承認要求が自動許可されないことを統合試験で確かめる。標準 ACP の selected に、人間の操作だったことを証明する欄はない。

03-03 は、TUI 固有の入力・表示規則と、すべての入口が守る承認の性質に分ける。ACP では表示と操作の一部を信頼するクライアントへ委ねること、その対応条件を明記する。

allow_always を選択肢から外す措置は、許可の記憶範囲を守るうえで有効である。ただし、クライアントの自動承認を止める措置ではない。


---

4. 通信確認の遅れた応答を、終了後にセッションの許可へ取り込める競合がある

場所

- docs/design/05-interface/05-03-acp.md「通信の中継と秘密の入力の扱い」
- docs/design/03-safety/03-02-sandbox.md「通信の中継」
- crates/benitoiteh/src/sandbox/proxy.rs の ProxyRun::finish・decide

重さ：実装の前に直すべき。BH-057 の流用前提として明記すべき

問題点

これは新しい ACP 実装の不具合ではなく、流用予定の既存コードで見つかった競合である。

ProxyRun::finish() は finished = true を設定する。しかし、decide() の応答待ち は、recv_timeout が Ok(answer) を返した場合にはそのまま答えを採用する。終了状態を見るのはタイムアウト側であり、ApprovedForSession の場合はその後にドメインをセッションの許可へ加える。

したがって、次の順序が成立する。

1. 通信の確認への返事を待つ。
2. コマンドが終了して finish() が呼ばれる。
3. 次のタイムアウトより前に、古い確認への許可が届く。
4. 終了したコマンドの確認から、セッションのドメイン許可が増える。

これは「コマンドが終わった後に届いた答えは許可へ加えない」という設計に反する。

修正案

コマンドの終了と、承認の採用・許可の更新を、同じ状態管理で直列化する。単に応答受領後にもう一度 finished を読むだけでは、その読み取りと許可更新の間に競合が残る。

ACP の保留中要求にも、sessionId、一連の処理の世代、元のコマンドの生存状態を結び付ける。終了・取消が先に確定した要求への応答は、許可を変更しない。


---

5. 取消・EOF・出力障害の終了手順が不足している

場所

- docs/design/05-interface/05-03-acp.md「コマンドラインと入出力」「スレッドの構成」「実行の前の確認」
- docs/design/06-impl/06-01-crate-structure.md「スレッドと非同期」
- docs/design/07-quality/07-01-testing.md「持ち主の境界」

重さ：実装の前に直すべき

問題点

Mutex で stdout の行を混ぜないことと、終了できることは別の条件である。

既存コードには、次の制約がある。

- InteractiveApprover は返事のチャネルで待つ。取消の原子的な印だけでは、この待ちは解けない。
- Agent::emit は上限付きチャネルへ send() する。
- Worker::close は、仕事の完了とイベントの受信を終えてから呼ぶ契約である。
- Worker::drain は、完了通知を受けた後にも残るイベントを回収する。

ACP 側で EOF を受けた直後にイベント受信を止めて join() すると、満杯のイベントチャネルへ送っている Agent が終了できない。

また、主スレッドが stdout の write・flush で止まると、入力スレッドから届いた cancel や EOF を処理できない可能性がある。AcpApprover 自身が stdout の錠や書き込みを待っている場合も、返事用チャネルへ Cancelled を送るだけでは解けない。

修正案

少なくとも次を状態遷移として固定する。

- 取消・切断を確定したら、新しい仕事と承認要求の受付を止める。
- 対象の処理へ取消を設定し、保留中の承認を無効にして待ち手を起こす。
- 子プロセスと Agent の終了を待つ間も、イベントを受信する。切断後は必要に応じて破棄する。
- Agent 完了と残イベントの処理を終えてから join する。
- 接続が残る cancel では、最後の更新を送った後、元の session/prompt に一度だけ stopReason:"cancelled" を返す。
- stdout の書き込み失敗や滞留があっても、取消を設定し後始末を進められるようにする。
- 出力障害などによる終了では、まだ開いている stdin の読み取りスレッドを無期限に join しない。

ACP v1 も、cancel 後に下位処理が例外になった場合を含めて cancelled を返すこと、元の prompt の応答前に処理と更新を終えることを求める。公式の取消仕様 

出力を一つの送信担当に集約する構成は有力である。ただし、その送信待ちによって取消まで止まらないことが必要であり、「送信スレッドを足す」だけで完了とはしない。


---

6. ACP の選択肢と、内部の許可の記憶範囲を種類別に対応させる必要がある

場所

- docs/design/05-interface/05-03-acp.md「実行の前の確認」手順 2・3
- crates/benitoiteh/src/approval/mod.rs の remember_effects・remember_command・remember_domain

重さ：実装の前に直すべき

問題点

草稿には、通常の Effects・TestEffects について「許可するとエフェクトの範囲を覚える」とあるが、ACP の kind と内部結果の対応が確定していない。

現在の approval/mod.rs では、次の違いがある。

記憶するもの	記憶する内部結果

エフェクト	Approved
同じコマンドのセッション許可	ApprovedForSession
WebFetch のドメイン許可	ApprovedForSession


したがって、機械的に allow_always → ApprovedForSession とすると、エフェクトの記憶が働かない。反対に、通常の Effects を allow_once と表示して既存の Approved へ渡すと、表示上の寿命と実際に覚える寿命が食い違う。

T3Code 固有の問題もある。一般の Local ACP 経路では、ACP の options[].name がそのまま承認ボタンになるわけではない。確認した版では、表示用の選択肢が未指定となり、Web UI は固定のボタンを使う。registry の設定、承認ボタン

T3Code の操作	探す ACP の kind

Approve	allow_once のみ
Always allow this session	allow_always、なければ allow_once
Decline	reject_once


該当する選択肢がなければ cancelled を返す。選択処理、応答処理

このため、Effects に allow_always だけを出す修正では、通常の Approve が取り消しになる。

修正案

次の列を持つ対応表を設計へ追加する。

ApprovalKind × facts の条件 × optionId × kind × 内部結果 × 記憶する範囲 × 有効期間

そのうえで、次のいずれかを決める必要がある。

- 現行のエフェクト記憶の意味を保ち、T3Code 側で選択肢と意味を正しく表示・選択できるようにする。
- ACP 向けに本当の一回限りのエフェクト許可を追加し、今回の実行許可とセッションへ記憶する許可を内部でも分ける。

なお、.agents や読まないファイルの取り込みの一回限りの確認と、基礎となるエフェクトの記憶は別である。 既存コードはエフェクトが確認済みでも追加の確認を求める。この既存動作自体を、許可拡大の不具合とは判断しない。

一回限りの要求に対し T3Code が「セッション中許可」を表示しても、応答が提示済みの allow_once の ID なら一回だけとして扱う、という草稿の側の防御は維持すべきである。


---

7. exec::prepare の interactive 一つでは、ACP の方針を表現できない

場所

- docs/design/05-interface/05-03-acp.md「起動とセッションの準備の確認」「実行の前の確認」「通信の中継と秘密の入力の扱い」
- docs/design/06-impl/06-01-crate-structure.md
- crates/benitoiteh/src/cli/exec.rs
- crates/benitoiteh/src/agent/mod.rs

重さ：実装の前に分離方針を決めるべき

問題点

現在の ExecArgs.interactive は、複数の方針をまとめて切り替えている。exec.rs

項目	interactive = false	interactive = true

実行時の承認	Approval::Never	制限モードは OnRequest、通常モードは設定値
制限モードのエフェクト	--allow、省略時は空集合	対話で確認
未許可ドメインの確認	無効	承認方針に応じて有効
askpass の対話	無効	有効にできる
--project-parent	制限モードで利用可能	拒否


ACP の案は、起動時は案 A の非対話的処理、実行時はクライアントへ確認、通信確認は有効、askpass は無効である。既存の bool のどちらにも一致しない。

さらに、Agent::with_approver() は通常の承認先を変えるだけで、NetworkGate へ確認チャネルをつなぐ処理は具象型の with_interactive_approver() にある。AcpApprover を実装して差し替えるだけでは BH-057 は動かない。

修正案

準備処理の責任を、少なくとも次に分ける。

- 起動時の信頼・処理系変更・モード変更の確認方針
- 実行時の承認先と承認方針
- --allow 等による事前許可の扱い
- 通信確認の可否と受け手
- 秘密入力の可否

具体的な型名は実装プランでよいが、ACP がそれぞれに何を選ぶかは設計で固定する。通常の承認と通信確認が、同じ取消可能な承認要求管理を使えるようにするのが自然である。


---

8. BH-055 は、v1 の具体的な通信契約として確定させる必要がある

場所

- docs/design/05-interface/05-03-acp.md「メソッドの対応」「実行の前の確認」
- docs/design/open-issues.md「BH-055」

重さ：実装の前に直すべき

問題点

現状のメソッド表は概要としては有用だが、自前の通信実装を作る契約としては不足している。また、「版 2 preview の拡張」という表現では、v1 へ追加できる欄だけのように読める。

v2 は、prompt の完了方式、再開、権限要求などが変わるメジャー改訂である。例えば v2 の session/prompt 応答は受け付けの応答であり、終了は state_update で伝える。session/load も廃止されている。固定した移行仕様 

修正案：初版は v1 に明確に固定する

T3Code は初期化で protocolVersion:2 と v1/v2 の両方の情報欄を送るが、v1 の応答形式を受けて v1 の処理へ進める。effect-acp/src/client.ts

したがって、初版では次を明記すればよい。

- 対応する通信形式は v1。
- 2 を要求されても、対応版として 1 を返す。
- 以後は v1 のメソッドとデータ形式だけを使う。
- v2 の追加欄を受信しても、未知の欄があるという理由だけで初期化を拒否しない。
- v2 対応は別の作業とする。

公式の移行仕様も、v1 専用 Agent が 2 の要求に 1 を返す交渉を認めている。

確認できた v1 の要求・応答

次表は、固定した v1 スキーマ に基づく。configOptions については、この設計で使う選択式の設定を対象としている。

メソッド	要求の主な params	正常時の result

initialize	protocolVersion。clientCapabilities・clientInfo は任意	protocolVersion。この実装では agentInfo・agentCapabilities・authMethods も明示するとよい
authenticate	methodId。広告した認証方法の ID	{}
session/new	絶対パスの cwd と mcpServers が必須	sessionId が必須。configOptions・modes は任意
session/load	sessionId・絶対パスの cwd・mcpServers	{} または configOptions・modes を含むオブジェクト。応答の sessionId は必須ではない
session/prompt	sessionId・prompt	処理終了時の stopReason
session/cancel	sessionId。JSON-RPC の id を持たない通知	応答しない
session/set_config_option	sessionId・configId・value	変更後の全 configOptions
session/set_mode	sessionId・modeId	{}
session/update	sessionId・update	Agent からの通知なので応答なし


session/load は、履歴の session/update を送ってから応答する。モデル・effort を提供するなら、load 時にも現在の configOptions を返すべきである。公式のセッション仕様 

草稿の cwd 省略時のフォールバックは、標準 v1 の要件ではなく独自の寛容化である。T3Code 向けの初版では、省略・相対パスを Invalid params にする方が明確である。

モデルと effort は、type:"select" の configOptions とし、意味を表す category はそれぞれ model・thought_level が適切である。制限モードと通常モードを選べる modes は広告しない。session/set_mode を実装しないなら -32601 とする方針でよい。

session/request_permission の正確な入れ子

v1 の要求は、次の形になる。これは通信形式を示す架空の例であり、この形式だけで指摘 2 の表示問題が解決するわけではない。

{
  "jsonrpc": "2.0",
  "id": "bh-permission-7",
  "method": "session/request_permission",
  "params": {
    "sessionId": "session-1",
    "toolCall": {
      "toolCallId": "call-3",
      "title": "Run command outside the sandbox",
      "kind": "execute",
      "status": "pending",
      "rawInput": {
        "command": "git status"
      },
      "content": [
        {
          "type": "content",
          "content": {
            "type": "text",
            "text": "Verified by benitoiteh\nCommand: git status\nWorking directory: /project\nSandbox: disabled for this call only"
          }
        }
      ]
    },
    "options": [
      {
        "optionId": "permit-this-call",
        "name": "Allow this execution once",
        "kind": "allow_once"
      },
      {
        "optionId": "reject-this-call",
        "name": "Reject",
        "kind": "reject_once"
      }
    ]
  }
}

選択された場合の応答：

{
  "jsonrpc": "2.0",
  "id": "bh-permission-7",
  "result": {
    "outcome": {
      "outcome": "selected",
      "optionId": "permit-this-call"
    }
  }
}

取り消された場合の応答：

{
  "jsonrpc": "2.0",
  "id": "bh-permission-7",
  "result": {
    "outcome": {
      "outcome": "cancelled"
    }
  }
}

ここで注意すべき点は次のとおりである。

- toolCall は ToolCallUpdate。その中の必須欄は toolCallId だが、安全な確認には十分な事実を追加する必要がある。
- options の各要素は optionId・name・kind を持つ。
- optionId と kind は別物である。
- v1 の要求直下に、任意の説明を入れる標準の description 欄はない。
- 選択の応答は result.outcome.outcome と二重になる。
- 拒否は通常、selected と拒否用 optionId で表す。cancelled と同一視しない。

これらは v1 スキーマ と Tool Calls の仕様 で確認できる。

また、未知の要求には -32601 を返す一方、未知の通知には返答しない。JSON-RPC の応答は、要求 ID の値と型を保存する必要がある。JSON-RPC 2.0 §4–5 


---

9. イベントと終了理由の変換には、未確定の形式と既存 API との差がある

場所

- docs/design/05-interface/05-03-acp.md「イベントから session/update への変換」「依頼の受け取りと終わりの理由」
- crates/benitoiteh/src/agent/mod.rs の run_controlled

重さ：通信上の値は実装の前に直すべき。内部の型の具体化は実装の中でよい

問題点

「エージェントの通知」「状態の通知」「診断の通知」は、送信する sessionUpdate の値としては未定義のままである。

確認した v1 では次のようになる。スキーマ

用途	使用できる形式・条件

エージェントの文	agent_message_chunk
推論の文	agent_thought_chunk
履歴の利用者メッセージ	user_message_chunk
道具の開始・更新	tool_call・tool_call_update
通知	notice。クライアントの session.notices 対応が必要
圧縮	compaction_update 等。クライアントの session.compaction 対応が必要
文脈使用量	usage_update。used と size が必須


特に usage_update.used は、現在の文脈にあるトークン数である。各番の入力・出力トークン数や、累積消費量をそのまま入れる欄ではない。size は文脈窓の上限である。

また、承認を待っている道具の状態は pending である。草稿のように、すべての ToolCall を初めから in_progress とする対応は修正した方がよい。

終了理由にも実装との差がある。現在の run_controlled は、run_request のエラーを内部で捕まえて Outcome::Failed と文字列の理由に変える。そのため、呼び出し側で単純に Err(AgentError::Limit) を match して、草稿どおりに変換することはできない。

修正案

各イベントについて、完全な JSON の形、対応 capability、非対応時の扱いを決める。通知に対応しないクライアントへ必要な情報を伝える場合は、通常のエージェントメッセージなどへ変換する。ただし、安全上必須の承認情報を、表示される保証のない通知へ移して済ませてはいけない。

usage_update は文脈使用量と上限を取得できる場合だけ送る。履歴再送には利用者メッセージも含め、再送から承認済み状態を復元しない。

終了理由は、番数上限、トークン上限、取消、提供者の失敗などを型として持たせる。英語の診断文を解析して stopReason を選ぶ実装は避ける。


---

10. MCP 非対応は維持すべきだが、ACP v1 の標準との差を明示する必要がある

場所

- docs/design/05-interface/05-03-acp.md「前提」「信頼の境界と安全の原則」
- docs/design/open-issues.md「BH-055」

重さ：実装の前に対応範囲を明記すべき

問題点

確認した ACP v1 の「MCP servers」は、すべての Agent に stdio の MCP 接続対応を MUST として求めている。HTTP・SSE は任意だが、標準 v1 の mcpCapabilities に stdio 非対応を広告する欄はない。固定した仕様

したがって、MCP を一切取り込まない本設計は、ACP v1 の制限付き対応になる。

ここで非空の mcpServers を一律に拒否すれば分かりやすいが、T3Code との連携目的には適さない可能性がある。T3Code は自分の MCP bridge を渡し、ACP/HTTP の対応が広告されていなくても stdio 設定を選ぶ。AcpSessionRuntime.ts

修正案

安全方針を維持し、「初版は MCP を利用しない ACP v1 の限定対応」と明記する。

T3Code 向けには、草稿の「接続せず、道具へ加えず、無視したことを診断する」扱いが現実的である。ただし、診断には資格情報を載せず、指摘 1 の環境経路も閉じる。

fs/*・terminal/* を使用しない方針は、そのままでよい。MCP については、標準との差を意識した判断として記録する必要がある。


---

11. 添付画像と埋め込みリソースは、パス参照と受信済みの内容を分けて定義する必要がある

場所

- docs/design/05-interface/05-03-acp.md「依頼の受け取りと終わりの理由」

重さ：実装の中で直せばよい。ただし受け付ける種類はプランに入れるべき

問題点

ACP の画像は、ファイルパスを持つとは限らない。v1 の image は data、つまり base64 の内容と、mimeType が必須であり、uri は任意である。

また、次を区別する必要がある。

- resource_link：内容を持たない参照。
- resource：resource.uri と text または blob を持つ埋め込み内容。
- image：base64 の画像内容。

resource は embeddedContext の対応に関係する追加機能であり、text と resource_link の扱いとは別である。v1 の ContentBlock・ImageContent 等

草稿の「画像は read と同じ規則を通す」は、ローカルパスを自分で開く場合には妥当だが、クライアントが既に送ってきた bytes に、そのままパスの規則を適用することはできない。

修正案

受け付ける種類ごとに、変換と拒否の規則を定める。

- パス・URI の参照を自動で開かない現方針は維持する。
- 埋め込み内容は、クライアントから渡された内容として扱い、ローカルファイルを読んだ証拠にはしない。
- bytes を受け取る画像縮小経路を用意する。
- base64 の長さ、復号後の大きさ、画像寸法、添付数、1 メッセージの大きさに上限を設ける。
- 非対応の blob や音声を、黙って文字列化・脱落させない。
- リソース本文も LLM にとって信頼しない内容であり、モードや権限を変える指示として扱わない。


---

12. 公式 Rust SDK を使わない理由のうち、tokio 依存という事実は訂正した方がよい

場所

- docs/design/05-interface/05-03-acp.md「前提」

重さ：実装の中で直せばよい

問題点

今回確認した公式 Rust SDK agent-client-protocol 5.4.0 の manifest では、tokio は dev-dependencies にあり、通常依存として記載されていない。通常依存には futures などがある。

出典は agentclientprotocol/rust-sdk@8ac65cf6b0b3371ae4d8231760cb91dad99e92ef の Cargo.toml である。

修正案

自前の同期 I/O を採る判断自体を変える必要はない。理由を、OS のスレッドと mpsc の構成を保つこと、非同期 API に合わせた変更を避けること、依存を抑えることとして記述する。

その代わり、自前の送受信形式は、固定した公式スキーマや仕様から作った通信例で検証する。実行時の依存を増やさずに、形式の取り違えを検出する方法を実装プランへ入れるとよい。

未決事項への判断

BH-054：案 A を推奨する

起動時は保存された信頼の決定と明示的な起動オプションに従い、制限モードでの処理系変更と、制限モードから通常モードへの再開を拒む案がよい。

T3Code の一般 ACP 経路には、active turn がないと入力要求を扱えない処理がある。activeContext 案 B を prompt 内へ移せばこの条件は満たし得るが、信頼する前の準備を途中で止める構造、指摘 2・3 の表示と自動承認の問題を引き受ける。

ただし、「ACP の仕様が new/load 中の permission を禁止している」とまでは今回確認できなかった。根拠は、v1 にこの起動確認のための独立した形式がなく、対象クライアントの入力処理にも制約があるためとするのが正確である。

BH-055：v1 の仕様確認部分と、実機確認部分を分ける

今回、主要なメソッドと JSON の形は一次資料で確定できた。これらは草稿の【要検証】から、出典を付けた具体的な仕様へ進めてよい。

一方、次は実機確認として残す必要がある。

- T3Code の初期化要求から v1 へ交渉できること。
- 承認の全事実と選択肢の意味が表示されること。
- new/load、モデルと effort の復元、履歴表示が成立すること。
- cancel・EOF・再接続で処理と子プロセスが残らないこと。

BH-056：案 A を推奨する

確認した T3Code の一般 ACP 経路では、1 プロセス内に複数 Agent を並行保持する必要性は見つからなかった。

T3Code は同じ runtime に active turn があれば新しい turn を拒否し、別の session ID が必要な場合はセッションを活性化し直す。startTurnUnlocked

したがって、次が成立すれば案 A と整合する。

- 前のセッションを保存し、後から session/load で再開できる。
- active でない ID への prompt は拒否する。
- 実行中の new/load は拒否する。
- 切り替え時に設定と保留中承認を混同しない。
- 再開時に過去の承認記録を現在の許可へ復元しない。

「T3Code のすべてのスレッドが常に専用プロセスを持つ」とまでは断定しない。複数スレッドの同時利用と切り替えは統合試験に残せばよく、初版から案 B の複雑さを引き受ける根拠はない。

BH-057：現方針に賛成する

通信のドメイン確認を session/request_permission に流し、askpass は無効とする方針がよい。

ドメイン確認では、要求 ID と元のコマンドを結び付け、「今回だけ」がそのコマンドの間に効くこと、許可がそのドメインの全ポートに効くことを表示する。指摘 4・5・7 の修正を前提とする。

askpass 無効化の理由は、「ACP に文字列入力 API が一切ないため」とは書かない方がよい。現行 ACP には elicitation/create がある。しかし、その form mode はパスワードや API key、token などの入力に使うことを明示的に禁止している。固定した elicitation 仕様

したがって、通常のメッセージやフォームへ秘密入力を落とさず、初版では無効にする判断は妥当である。

実装プランへ入れるべき確認

07-01 に加えた ACP の入口のテストは、現状では項目の見出しに近い。少なくとも次を完了条件として具体化すると、今回の設計判断を検証できる。

確認する性質	最低限の確認内容

信頼の境界	T3Code 用の囮の資格情報が子に渡らない。非空 mcpServers で接続・起動・道具追加が起きない
承認の範囲	未提示・重複・古い optionId を許可にしない。一回限りの操作が次回も確認を要する
承認の表示	必須事実の欠落、長いコマンド、制御文字、偽の justification、T3Code の自動承認設定を確かめる
終了処理	通常承認待ち・通信承認待ち・子の実行中の cancel/EOF、イベントキュー満杯、stdout 障害を扱える
競合	コマンド終了と承認応答を競合させ、終了後にドメイン許可が増えない
通信形式	固定した v1 スキーマとの照合と、T3Code での new/load/config/prompt/cancel の往復
セッション切替	別 ID・別 cwd・再開・設定復元を確かめ、承認が別のセッションへ漏れない


全体の所見

先に、安全上の信頼境界、承認画面との契約、取消処理、v1 の通信形式を改訂してから、実装プランを確定することを推奨する。 とりわけ指摘 1 の MCP 資格情報と、指摘 2・3・6 の T3Code の承認処理は、単に AcpApprover を追加すれば済む箇所ではない。

未決事項は、BH-054 は案 A、BH-056 も案 A、BH-057 は現方針で進めるのがよい。これらを具体化すれば、OS のスレッドと mpsc を使う現在の構成を保ったまま、実装プランの作成へ進める。
