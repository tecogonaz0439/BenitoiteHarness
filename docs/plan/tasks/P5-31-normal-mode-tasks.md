# P5-31 通常モードの判定の実行器（macOS）

## 目的

通常モードの成果物を、通信を許さず、判定のディレクトリだけに書ける形で検査する実行器を作る。macOS 用の example と、その実行器固有のテストを担当する。P5-29 の後に行い、P5-30 と並べて進められる。評価の道具への接続と試しの 2 課題は P5-34、残りの 8 課題は P5-35 に分ける。本物の LLM を呼ばない。

## 読む設計書

- [評価](../../design/07-quality/07-02-evaluation.md)の「通常モードの課題の作り方」「M の段の共通の条件」「M の段の通常モード」「M の段の残りの規模と費用」「評価の手順」
- [モード](../../design/01-agent/01-02-modes.md)の通常モード、[道具](../../design/01-agent/01-03-tools.md)の「bash（通常モード）」
- [OS のサンドボックス](../../design/03-safety/03-02-sandbox.md)の「通常モードの方針」「子プロセスの後始末」、[利用者の確認](../../design/03-safety/03-03-approval.md)の「非対話での扱い」
- [テスト](../../design/07-quality/07-01-testing.md)の「持ち主の境界」、スキル `test-audit`
- [P5-29](P5-29-completion-check.md)、[P5-34](P5-34-normal-mode-evaluation.md) の判定からの呼び出しの条件（P5-34 の実装は待たない）
- 既存の `crates/benitoiteh/src/sandbox/{mod.rs,policy.rs,normal.rs,run.rs}`、`src/tools/bash.rs`（同じサンドボックスと子プロセスの実行を再利用する）

## 作るもの

- `crates/benitoiteh/examples/skill_eval_judge.rs` を作り、Python の道具から起動する小さな実行器とする。LLM、認証、Agent のループを使わず、既存の `sandbox::{PolicyConfig, SandboxPolicy, SandboxCommand}` と `sandbox::run` を使う。テストの差し込み口として本番の API を広げない。
- 引数は `--helper <benitoiteh の実のパス> --cwd <判定のディレクトリ> --temp <専用の一時ディレクトリ> --timeout 60 --deny-read <リポジトリの実のパス> --deny-read <workspace の実のパス> -- <コマンドの実のパス> <引数>...` とする。コマンドはシェルの文字列に組み立てず、配列で渡す。
- `PolicyConfig::new` の書ける場所を判定のディレクトリと専用の一時ディレクトリだけにし、`allow_ip=false`、`deny_exec_in_writable=false` とする。通常モードの子プロセスと同じ起動と後始末を使う。サンドボックスを掛けられなければ、サンドボックスなしで続けず失敗させる。
- 子の環境は評価の道具で整理したものにし、`PYTHONPATH`・`PYTHONHOME` を除き、`PYTHONNOUSERSITE=1`、`LC_ALL=C`、判定用の HOME・一時ディレクトリを設定する。キー、利用者の設定、元の作業ツリーの環境を判定へ渡さない。
- 呼び出し元へ、終了状態、時間切れ、残る子孫の可能性と出力の打ち切りを返す。残る子孫の可能性があれば失敗として後続を止める。評価の道具側の `--judge-runner` と実行器の探索は P5-34 で作る。normal の self-test・dry-run・本物の走行のすべてでこの実行器を必要とする。

- 実行は macOS のみとする。Linux を含むほかの OS では cfg でビルドを通し、起動したら使えない理由を示して失敗させる。サンドボックスなしの代替は作らない。
- `--deny-read <場所>` を繰り返して指定できるようにし、P5-34 からリポジトリと元の workspace を渡す。`PolicyConfig.denied_read` に入れ、判定から元の入力と解の置き場を読めないようにする。
- `sandbox::run` は出力を上限なく渡すため、標準出力と標準エラー出力の受け取り先を、それぞれ 1 MiB で保存を打ち切るものにする。超過後もパイプを読み捨て、子を詰まらせない。打ち切りを呼び出し元へ返す。
- `scripts/check.sh` で `cargo build -p benitoiteh --example skill_eval_judge` を明示する。Codex 内の `cargo test --lib --bins` だけでは example はビルドされない。

## 触るファイル

- `crates/benitoiteh/examples/skill_eval_judge.rs`（新）
- `crates/benitoiteh/tests/sandbox_skill_eval_judge.rs`（新、`#![cfg(target_os = "macos")]`）と必要な既存の共有の準備
- `scripts/check.sh`（example の明示的なビルドと、実行器のテストを走らせなかったことの表示）
- `crates/benitoiteh/src/sandbox/{session.rs,mod.rs,text.rs,macos.rs}`（子だけの資源の上限と共通の process-info の拒否）、`tests/cli.rs`、`tests/sandbox_macos.rs`、`tests/support/macos_process_info.rs`、`crates/sandbox-probe/src/{process.rs,text.rs}`
- 設計書 03-02・07-01・07-02 と `docs/plan/{10-interfaces.md,tasks/P5-34-normal-mode-evaluation.md}`（レビューで明示された安全の性質と呼び出しの条件を揃える）

既存の sandbox の API を再利用し、実行の本体と Seatbelt の方針を写さない。テストのために本番の API を広げない。

## テスト

`test-audit` の作成時の関門に従い、実行器固有の契約を本物の子プロセスで確認する。

- 通信、判定の外への書き込み、`--deny-read` の場所の読み取りを拒み、判定内の許した操作が通ることを対照にする。
- 時間切れの子が止まること、起動前の失敗を成功にしないこと、出力の打ち切りと残る子孫の可能性が失敗として返ることを確認する。
- 通信の拒否の確認は手元の TCP のループバックを使うか、既存の `tests/support/socket_directory.rs` の短いパスの準備を使う。外部には接続しない。
- テストからは `current_exe()` を起点に `../examples/` を辿り、ビルドした example を探す。既存の OS ごとのサンドボックスのテストを繰り返さない。

## 完了条件

- `scripts/check.sh --inside-codex` で example を含むビルドと lint が通る。Linux 向け `clippy --all-targets` でもビルドできる。
- Codex 内では `sandbox_skill_eval_judge` を走らせず、除いた名前を示す。オーケストレータが外の macOS で実行器のテストを完了できる。
- P5-34 が使う引数、結果の形、出力の打ち切りと後始末の失敗の例を報告する。Python の評価の道具と課題の完成には依存しない。

## 補足（2026-10-20、始める前の点検）

本節は、上の各節に優先する。以下の推奨を、明記した修正を除いてこの作業の指示として採る。コードの行番号は、点検報告が参照した main の `d67e1af` のものである。

### 既存の API と実行の範囲

- macOS の起動は `SandboxCommand::to_command(helper)`（`sandbox/mod.rs:108–130`）と `sandbox::run::run`（`run.rs:242`）。通常モードの方針は `normal.rs:11–37`、起動の手本は `bash.rs:269–330`。
- Linux の `SandboxCommand::prepare` は `scratch` と `placeholders` を要する（`sandbox/linux/process.rs:46–51`）。今回は Linux の実行には対応しない。

1. **判定の実行器**
   - macOS の `/usr/bin/sandbox-exec` は、この機械（Darwin 27）にあり、試作も使っている。
   - Python から直接 sandbox-exec を呼ぶと Seatbelt の方針を二重に持ち、既存のテストの外に出る。作業の文書どおり、example から既存の API を呼ぶ形を勧める。
   - Linux では `scratch` と `placeholders` が要るが、作業の文書の引数にない。評価は macOS で行うので、Linux では「使えない」として失敗させる（cfg でビルドは通す）ことを勧める。
   - `run` は出力を上限なく渡す（`run.rs:240` の注記）ので、受け取り先で 1 MiB に切り、打ち切りの印を返す。
   - 推奨: `denied_read` にリポジトリと workspace を足す。

### ビルドとテストの場所

- 実行器のテストは macOS の `tests/sandbox_skill_eval_judge.rs` に置く。Codex 内では sandbox-exec を重ねられない。
- `check.sh` の `cargo test --lib --bins`（123）では example はビルドされないので、example のビルドを明示する。Linux 向けの `clippy --all-targets`（100–103）にも通す。
- 権限の印だけによる書き込みの拒否を期待せず、方針の書ける場所の外で確かめる。今回は Linux のサンドボックスのテストは足さない。
- macOS の TMPDIR は長いので、Unix ソケットのパスの上限に注意する。サンドボックスへ渡すパスは正規化し、`/var` と `/private/var` を揃える。

### P5-34 との分担

- 評価の道具が整理した環境を渡す。判定の環境に `PYTHONDONTWRITEBYTECODE=1` を加え、Python の実体のパスでサンドボックス内の起動を準備時に試す処理は P5-34 が持つ。
- 判定の失敗、出力の打ち切り、残る子孫の可能性を P5-34 が行の失敗として残す。

## 補足（レビュー後の安全の性質と結果の形）

本節は、上の各節の古い記述に優先する。

- `--cwd` と `--temp` は既存のディレクトリで、`..` の要素、末尾のシンボリックリンクを拒む。開いたパスを `rustix::fs::getpath`（F_GETPATH）でカーネルのパスへ直し、字面と一致することを求める。先頭の `/tmp`・`/var`・`/etc` と `/private` の別名だけを許す。中間のリンクや firmlink の別名による writable の指定は拒む。`--helper`・`--deny-read`・コマンドも F_GETPATH で直し、直せなければ失敗とする。
- 書ける場所と読ませない場所がどちら向きに含み合っても拒む。二つの書ける場所の重なり、`/`、HOME とその祖先、実行器または補助を含む書ける場所も拒む。`--temp` は空に限る。`DataPaths::from_env()` による設定とデータ、HOME 下の `.ssh`・`.gnupg`・`.aws`・`Library/Keychains` を、存在すれば必ず読ませない場所に足す。
- `--helper` は実行器の `examples/` の隣の `benitoiteh` と同じ実のパスであることを確かめる。起動前に `/dev/fd` を列挙し、標準入出力のほかの開いた記述子があれば失敗とする。
- 共通の Seatbelt の方針で、`(deny process-info*)` の後に `(allow process-info* (target self))` を置く。子の環境を整理しても残る、親やほかのプロセスの引数・環境からのキーの読み取りを防ぐ（設計書 03-02「方針の要素」）。
- `session::ResourceLimits` と `SandboxCommand::to_command_with_limits` は、実行器の CPU 時間を制限せず、子孫だけを制限するための起動の口である。テスト用の差し込み口ではない。`__session` は setsid の後、exec の前に `--rlimit-cpu`（秒）・`--rlimit-fsize`（バイト）を掛ける。判定の CPU は `timeout × 論理 CPU 数 + 5 秒`、FSIZE は 64 MiB。継いだ小さい上限は広げない。SIGXCPU（24）と SIGXFSZ（25）は別の理由を返す。
- 判定ごとに新しい乱数の印 `SKILL_EVAL_JUDGE_RUN=<32 桁の 16 進>` を子の環境へ加える。回収の後に `ps -axE -o pid=,command=` で今回の印だけを探し、同じ UID の cwd を `/usr/sbin/lsof -a -d cwd -u <uid> -Fpn` で調べる。書ける場所またはその下の cwd も、検出した残りとして失敗させる。無関係な孤児の出現だけでは失敗させない。lsof の `p`・`f`・`n` のフィールドと名前の文字の表記は macOS の `lsof(8)`「OUTPUT FOR OTHER PROGRAMS」「OUTPUT」に従い、解く関数を OS のサンドボックスなしの単体テストで確かめる。

結果は schema_version 1 の JSON オブジェクト一つで返す。`success`、`exit_code`（なければ null）、`signal`（なければ null）、`end`（`exited|timed_out|cancelled|error`）、`timed_out`、`possibly_remaining`、`stop_required`、`stdout`、`stderr`、`stdout_truncated`、`stderr_truncated`、`error`（成功なら null）を持つ。成功は終了状態 0・通常の終了・打ち切りなし・残りの兆候なしを満たすときだけで、実行器自体の終了状態も 0、それ以外は 1 とする。macOS 以外はサンドボックスなしで起動せず、使えないという失敗の JSON を返す。

cwd の一覧から実行器自身は除く。検査の lsof は `/` を cwd にして起動する。理由: 呼び出し元が実行器自身の cwd を `--cwd` と同じにしても、実行器と検査の子を残存した子孫と誤認しないためである。

`possibly_remaining` は、閉じないパイプ、環境の印、書ける場所の cwd を検出したか、実行・後始末・検査が失敗して残りを否定できなかったことを表す。true のときは `stop_required` も true とする。false は全子孫の終了の保証ではない。macOS 27 の Apple 製プログラム（sh・sleep・perl など）の環境は `ps -E` に出ないので、悪意がなくても環境の印だけでは拾えない。印が見えず、cwd も外へ移した残りは拾えない。cwd の検査には同じ場所を使う無関係な同 UID のプロセスを拾う偽陽性もある。

時間切れの例は `end: "timed_out"`・`timed_out: true` とし、子が SIGTERM を扱って 0 を返しても失敗とする。各出力の保存は 1 MiB で打ち切り、超過は読み捨て続けて失敗とする。検出した残り・後始末の失敗・一覧の検査の失敗は、後続を止める理由を `error` に返す。出力の打ち切りがあっても、時間切れや資源の合図の理由を優先する。

P5-34 では、コマンドごとに新しい判定のディレクトリと空の一時の場所を用意し、verify と期待する値を書ける場所の外に置く。評価の終わりに残ったプロセスを調べ、Python からは `close_fds=True` で起動する。実行器が合図で終わった場合や JSON が読めない場合も停止扱いとする。出力の制御文字を端末へそのまま出さない。実行器自身には元の HOME と XDG の場所を渡して通常の秘密を除外させ、判定の子だけを私有の HOME へ切り替える。
