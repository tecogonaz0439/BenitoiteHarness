# P5-52 macOS で、Seatbelt が記録した拒否の理由を bash の結果に返す

- 状態: 完了
- 依存: P5-51

## 目的

通常モードの `bash` がサンドボックスに拒否されたと見なしたとき、今は推定の文（`[sandbox: the command may have been blocked by the sandbox. ...]`）だけを返す。macOS では、Seatbelt が記録した拒否の操作と対象も返し、LLM がどの操作が拒まれたかを知れるようにする（キャッチアップの案の段 3 の 3。[P5-44-stage3](../results/P5-44-stage3.md)の「限り」、設計書 03-02「拒否の検出」）。

## 経緯

2026-11-05 に設計者が、この作業を進めるよう指示した。作業はオーケストレータ（Claude Code）が行った。

## 作ったもの

- Seatbelt の方針の拒む規則（`deny default`、`process-info*`、秘密の名前、読ませない場所、書ける場所の中の実行、守る名前、gitdir の指す先）に、`(with message (param "DENIAL_TAG"))` を付けた。`DENIAL_TAG` は方針を作るたびの乱数の印で、`SeatbeltProfile::denial_tag` に持つ。
- `SandboxCommand::to_command_reporting`（macOS）は、起動の形と `sandbox::DenialLog` を返す。`DenialLog::read` は、`/usr/bin/log show --style ndjson --last <起動からの秒数＋2>s` を、送り手がカーネルの Sandbox の拡張で、末尾に印を持つ記録に絞って走らせ、拒否を `Denials`（示す 10 件と、残りの数）にする。出力は 256 KiB まで読み、10 秒で止める。
- 並べ方は、ファイル・通信・プログラムの起動の拒否を先にする。シェルは起動のたびに `sysctl-read` などで拒まれ、そのままでは失敗の理由のファイルの拒否が上限の外に出たためである。`/dev/dtracehelper` への書き込みの拒否は除き、制御の文字は `?` に替える。
- `bash` は、拒否と見なしたときだけ記録を読み、一件ごとに `[sandbox denied: <操作> <対象> (<プログラム>)]`、残りがあれば `[sandbox denied: N more records omitted]` を加える。Linux は今のままである。
- 設計書 03-02「拒否の検出」と 01-03「bash（通常モード）」に書いた。

## 確かめた結果

2026-11-05 に、macOS 27.2 の `sandbox-exec` で、`deny default` と個別の `deny` の規則のどちらでも、印が記録の末尾に付き、子の終了の直後の `log show --last` で読めることを確かめた。読むのにかかる時間は 1.5 秒ほどだった。

| テスト | 守る契約 |
|---|---|
| `tests/sandbox_bash.rs` の `macos_denied_bash_reports_the_recorded_operation_and_path`（新しいテスト） | 本物の Seatbelt で、書ける場所の外への書き込みと守る名前（`.git`）の作成の拒否が、操作と対象つきで結果に載る。二回目の起動の結果に一回目の拒否が混ざらない。拒否と見なさない失敗（`exit 1`）には載らない |
| `src/sandbox/denials.rs` の `denial_records_are_filtered_by_tag_and_kept_on_one_line` | 印の違う記録、形の崩れた記録、DTrace の補助、重なり、制御の文字、並べ方と上限。ほかの起動の記録と制御の文字を含む記録は、実機の起動では決まった形で作れないので、記録の解釈の境界で確かめる |
| `src/sandbox/tests.rs` の `seatbelt_source_does_not_depend_on_path_contents` | 印をパラメータで渡し、方針の文がパスにも印にも依らないこと（既存の行の扱いに `DENIAL_TAG` を足した） |

新しいテストを作ったのは、拒否の記録を返す契約を確かめるテストがなかったためである。推定の文だけを確かめる既存のテストは、記録を読まない退行を捕まえない。

壊した版で、次のように確かめた。

| 壊した箇所 | 結果 |
|---|---|
| 個別の拒む規則に印を付けない | 守る名前の行で失敗した |
| 起動ごとの印を同じ値にする | 二回目の起動の行で失敗した（一回目の拒否が混ざった） |
| 拒否の記録を読まない | 書ける場所の外の行で失敗した |

macOS で `scripts/check.sh --inside-codex` が終了状態 0 で通った。全体の検査（`scripts/check.sh`、`BENITOITEH_X86_64_HOST` を指定。コミット `7b994c0`）も終了状態 0 だった。

| 対象 | 結果 |
|---|---|
| macOS | 821 件が通り、失敗 0、無視 14（足した二つのテストで 2 件増えた）。評価の道具の Python のテスト 118 件も通った |
| aarch64 の Linux（Apple の `container`） | 721 件が通り、失敗 0、無視 13（Landlock を要する `sandbox_*.rs` は飛ばす） |
| x86_64 の Linux（試験の機械の podman） | 815 件が通り、失敗 0、無視 16 |
