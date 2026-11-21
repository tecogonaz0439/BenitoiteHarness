# ChatGPT Pro への再レビュー依頼の文面（ACP 対応・第2回）

前回のレビュー（指摘 01〜12 および未決事項 `BH-054`〜`BH-057` への判断）をすべて反映して、ACP 対応（`benitoiteh acp`）の設計書を改訂しました。実装プランの作成に進む前に、改訂内容が指摘を過不足なく満たしているか、再確認をお願いします。

### レビューの対象

- リポジトリ: https://github.com/tecogonaz0439/BenitoiteHarness （公開）
- 対象のブランチ: `design/acp-support`
- 前回のコミット: `f04e5d9c2985c84da747491e17b14c548bef252e`
- 今回の改訂コミット: `b3e42b56c8f749cff97668e82138a9ed344fb46d`
  - ツリー全体: https://github.com/tecogonaz0439/BenitoiteHarness/tree/b3e42b56c8f749cff97668e82138a9ed344fb46d
  - 前回からの差分: https://github.com/tecogonaz0439/BenitoiteHarness/compare/f04e5d9c2985c84da747491e17b14c548bef252e...b3e42b56c8f749cff97668e82138a9ed344fb46d
- 主な改訂ファイル:
  - `docs/design/05-interface/05-03-acp.md`（ACP（エディタとの連携））
  - `docs/design/03-safety/03-02-sandbox.md`（「通信の中継」「規則の後に必ず当てる変更」）
  - `docs/design/03-safety/03-03-approval.md`（「確認の共通の規則」）
  - `docs/design/05-interface/05-01-cli.md`（「設定ファイル」）
  - `docs/design/07-quality/07-01-testing.md`（「安全の性質のテスト」）
  - `docs/design/open-issues.md`（`BH-054`〜`BH-057`）
  - `docs/design/08-appendix/08-03-external-review.md` および `docs/design/08-appendix/reviews/2026-11-21-chatgpt-pro/`（前回のレビュー記録と指摘ごとの扱い）

### 前回の指摘への対応の概要

1. **指摘 01・10（MCP 非対応の明記と T3Code の MCP 資格情報の強制除外・秘匿）**:
   - `05-03-acp.md` に「MCP を利用しない ACP 版 1 の限定対応」と明記し、非空の `mcpServers` はエラーで拒まず、資格情報を診断・記録に一切出さずに無視した件数だけを示す形にしました。
   - `03-02-sandbox.md`「規則の後に必ず当てる変更」と `05-01-cli.md`「設定ファイル」に、T3Code が起動環境に渡す `T3_ACP_MCP_*` と `T3CODE_*` を提供者のキーと同じく子プロセスの環境から強制除外し、環境の規則の `set` や `*` を含まない `allow` で指定することも設定の誤りとして拒む規則を加えました。
2. **指摘 02・03・06（T3Code の承認カードへの全事実表示、Supervised 前提、選択肢と記憶範囲の対応表）**:
   - T3Code の `parsePermissionRequest` が `command` を優先して承認カードの `prompt` に渡す作りを踏まえ、`benitoiteh` が組み立てた全事実（題、対象パス、エフェクトと説明、`unread_imports`、サンドボックスの範囲、`.agents` 警告、空白と制御文字を可視化したコマンド全文）を一つの複数行テキストにまとめて `toolCall.rawInput.command` と `content` の両方に入れ、LLM の `justification` は承認表示から省く規則にしました。
   - `03-03-approval.md`「確認の共通の規則」を、全入口の共通規則・TUI 固有の規則・ACP の規則に分け、人間による毎回の確認を成り立たせる条件としてクライアント側で自動承認を行わない設定（T3Code の Supervised 相当）を明記しました。
   - `05-03-acp.md` に `ApprovalKind × facts の条件 × 出す options（optionId / kind） × 内部の扱い × 記憶する範囲と有効期間` の表を追加し、通常の `Effects`・`TestEffects` でも `allow-once`（`allow_once`: 今回だけ実行し `confirmed_effects` を更新しない）と `allow-session`（`allow_always`: `Approved` で `confirmed_effects` を更新）を分ける形にしました。
3. **指摘 04・05（`ProxyRun` の終了競合の修正と、取消・EOF・出力障害の終了手順）**:
   - `03-02-sandbox.md` と `05-03-acp.md` に、`ProxyRun::finish` の終了状態と確認の応答の採用・許可集合の更新を同じ `Mutex` で直列化し、終了が確定したコマンドへの遅れた応答からセッションのドメイン許可が増えないようにする仕様を明記しました。
   - `05-03-acp.md`「スレッドの構成と終了の手順」に、出力専用スレッドと主スレッドの確認台帳を分け、取消・EOF・出力障害のときの 5 段の状態遷移（新規受付停止 → 取消設定と保留確認の即時解除 → `AgentEvent` チャネルの継続回収 → 残イベント回収後の完了応答 → 出力障害時は開いた `stdin` スレッドを無期限に `join` せず終了）を定めました。
4. **指摘 07・08・09・11・12 および `BH-054`〜`BH-057`**:
   - 初版の通信契約を ACP 版 1（`protocolVersion: 2` 要求に対して `1` を返して交渉）に固定し、メソッド表、`ContentBlock`（`text`・`resource_link`・`resource`・base64 を `benitoiteh __image` に通す `image`）、`sessionUpdate`（承認待ちは `pending`、`usage_update` は現在の文脈の見積もり `used` と `context_window` の `size`）、終了理由の型付け、準備の方針の 5 分離、Rust SDK の依存に関する記述の訂正、`07-01-testing.md` のテスト項目、`BH-054`〜`BH-057` の方針を反映しました。

### 特に見てほしい点

1. 前回の指摘 01〜12 の修正に抜けや新たな矛盾がないか。
2. 特に、`toolCall.rawInput.command` と `content` への全事実の格納、`Effects`・`TestEffects` における `allow_once`（記憶しない）と `allow_always`（記憶する）の分離、`ProxyRun` の直列化、終了手順の 5 段の状態遷移に穴が残っていないか。
3. この設計書をもとに `docs/plan/` の実装プランの作成へ進んでよいか。

### 報告の形

- 日本語でお願いします。
- 残る問題点があれば、前回と同じく場所・問題点・重さ・修正案の形で示してください。
- 最後に、実装プランの作成へ進んでよいかの判断をまとめてください。
