# 事前検討との対応

本付録は、Benitoite の事前検討の決定と未決事項が、試作の設計でどう扱われるかを一覧にする。ADR は Benitoite の初回リリース版の設計書の写し（`v0.0.6` のタグの `docs/archive/2026-10-09-design-first-release/decisions/`）を指す。TODO は Benitoite の docs/todo の項目（非公開）を指す。

扱いの欄の意味は次のとおりである。

- **引き継ぐ**: 試作でもその決定に従う。
- **改める**: 試作では別の形をとる。理由を添える。
- **当てはまらない**: 試作の条件では対象がない。
- **後で**: 試作の範囲に含めないか、評価の後に決める。

## ADR

| ADR | 決定の要点 | 扱い | 試作の章 |
|---|---|---|---|
| [0194](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.6/docs/archive/2026-10-09-design-first-release/decisions/0194-tui-and-own-coding-agent-with-server-mode.md) | TUI と自前のコーディングエージェントを作り、サーバモードを通して検査・実行する。LLM のクライアントと TUI の部品に既存の OSS を使う | 改める（サーバモードを通さない）。LLM の API のクライアントは、HTTP と TLS の OSS の上に自前のアダプタを作る形に改める。TUI の部品は OSS（`crossterm`）を使う | 00-01、02-01、06-01 |
| [0340](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.6/docs/archive/2026-10-09-design-first-release/decisions/0340-llm-providers-for-own-agent-harness.md) | 提供者は OpenAI 互換（Chat Completions と Responses）、Sign in with ChatGPT、Anthropic の Messages API、Gemini API。Claude の購読で自らサインインする形は候補にしない | 引き継ぐ。Sign in with ChatGPT と Gemini の専用のアダプタは後で | 02-01 |
| [0341](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.6/docs/archive/2026-10-09-design-first-release/decisions/0341-agent-cli-wrapper-library.md) | エージェントの CLI を包むライブラリ（エージェントハーネスとは別の機能） | 当てはまらない | — |
| [0342](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.6/docs/archive/2026-10-09-design-first-release/decisions/0342-agent-harness-after-server-mode.md) | エージェントハーネスはサーバモードの後。サーバモードのない版で動かす経路は作らない | 改める（サーバモードなしで動き、OS のサンドボックスを `benitoiteh` が掛ける） | 00-01、03-02 |
| [0343](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.6/docs/archive/2026-10-09-design-first-release/decisions/0343-agent-harness-user-config-file.md) | TOML の利用者単位の設定ファイル。複数の提供者、既定の提供者、オプションで切り替え。役割ごとのモデルは対象外。決まった場所に置き探さない | 引き継ぐ。サブコマンドの名前（`benitoite agent`）は改める（別の実行ファイル） | 05-01 |
| [0344](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.6/docs/archive/2026-10-09-design-first-release/decisions/0344-agent-harness-operation-scope-and-tools.md) | 操作の範囲は (γ)。`.bnt` だけを書ける。プロジェクトの作成と一時的なスクリプト。道具の一覧。シェル・プロジェクトの外の読み書き・外部の MCP サーバを持たせない | 制限モードで引き継ぐ。道具の名前と形を改め、`ask_user` を設けない。通常モードはシェルを持つ | 01-02、01-03 |
| [0345](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.6/docs/archive/2026-10-09-design-first-release/decisions/0345-agent-harness-confirmation-and-server-approval.md) | 実行の前の確認をハーネスの画面で受け、LLM は飛ばせない。サーバモードの承認とは別。一時的なスクリプトの確認なしの範囲。非対話の `--allow` | 引き継ぐ。サーバモードの承認との関係は当てはまらない。確認なしの範囲に `State` と `Process.Exit` を加え、確認なしで実行するときの読める範囲をサンドボックスで限る。非対話で範囲を超えたときは LLM に理由を返す形に改める | 03-03 |
| [0346](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.6/docs/archive/2026-10-09-design-first-release/decisions/0346-agent-harness-connects-to-daemon-directly.md) | デーモンの通信口に直接つなぐ。道具の形を MCP の道具と揃える | 改める（処理系の CLI を起動する）。道具の形を揃える点は後で（BH-009） | 04-01、01-03 |
| [0347](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.6/docs/archive/2026-10-09-design-first-release/decisions/0347-agent-harness-web-fetch.md) | `web_fetch` を加え、一覧のドメインは確認なし、ほかはドメインごとに確認。GET だけ、クッキーなし、大きさの上限 | 引き継ぐ。リダイレクトとプライベートのアドレスの規則を加える | 01-03 |
| [0348](https://github.com/tecogonaz0439/Benitoite/blob/v0.0.6/docs/archive/2026-10-09-design-first-release/decisions/0348-agent-harness-rewind-and-git.md) | 巻き戻しはプロジェクトの状態をまるごと記録して戻す。git の道具は git のライブラリで行い hooks を起動しない | 巻き戻しは引き継ぐ（記録の形は git を使わない形）。git の道具は後で（BH-007） | 01-05 |

## TODO（Benitoite の docs/todo）

| TODO | 論点 | 扱い | 試作の章 |
|---|---|---|---|
| TODO-124 | 目的と作る時期 | 目的は引き継ぐ。時期は改める（ADR 0342 と同じ） | 00-01 |
| TODO-125 | TUI の画面の構成と承認の画面の関係 | 画面の構成を定める。承認の画面との関係は当てはまらない | 05-02 |
| TODO-126 | LLM を使ったテストで要る機能 | 非対話の実行と集計の記録で応える。`read_reference`（試作の `benitoite_reference`）が返す文書の差し替えは後で | 01-05、07-02 |
| TODO-127 | 制限に残る抜け道と代償 | 言語の中の抜け道（`Process.Run`）を OS のサンドボックスで塞ぐ。許したエフェクトの中の対象の制限は後で（BH-040） | 01-02、03-02 |
| TODO-128 | 採る提供者と作りの順・使うクレート | 三つのアダプタを先に作る。LLM の API のクライアントのクレート（`async-openai`・`genai` など）を使わず、自前で組み立てる | 02-01 |
| TODO-129 | 自己ホスト・クラウドの基盤・Foundation Models | 後で | — |
| TODO-130 | 提供者に関する事実の確認 | 試作に要る範囲を【要検証】として残す（BH-030、BH-031） | 02-01 |
| TODO-131 | Claude Code の CLI を経由する提供者 | 後で | — |
| TODO-132 | 認証の情報の保管と表示 | キーの参照（環境変数とコマンド）と、送る内容の表示を作る。キーストアは後で（BH-033） | 02-01、01-04 |
| TODO-133 | 設定ファイルとプロジェクトごとの設定ファイル | 引き継ぐ（書ける欄を限る対策 (a)） | 05-01 |
| TODO-134 | 操作の範囲と道具、作業の流れと文脈の管理 | 道具は引き継いで改める。検査と修正の回数の上限（10 回）は引き継ぐ。圧縮と再開は設ける | 01-01、01-03、01-04 |
| TODO-135 | 実行の前の確認と非対話の許可 | ADR 0345 と同じ | 03-03 |
| TODO-136 | デーモンへの接続と道具の形 | ADR 0346 と同じ | 04-01 |
| TODO-137 | ウェブの取得 | ADR 0347 と同じ。既定の一覧は空とする | 01-03 |
| TODO-138 | 送る内容と秘密を含みやすいファイル | 読まないファイルの一覧と出力の上限を定める。秘密を出力から伏せる仕組みは後で | 01-03 |
| TODO-139 | 対話の操作 | `/` のコマンド、キャンセルを定める。入力の履歴を残すかは後で（BH-018） | 05-02、01-01 |
| TODO-140 | 巻き戻しと git の操作 | ADR 0348 と同じ。記録の時点に通常モードの `bash` を加える | 01-05 |
