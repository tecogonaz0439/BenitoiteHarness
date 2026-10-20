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
