# Strata と IQ3_S の構築

## 目的と確認状況

BenitoiteHarness の動作確認に使うローカル LLM として、Linux の推論サーバに Strata と `ISTA-DASLab/Qwen3.8-Flash-Next-GSQ-RCO-GGUF` の IQ3_S を導入する。`benitoiteh` は手元の機械で動かし、SSH のポート転送を通じて推論サーバを呼ぶ。初期の用途は挨拶と Benitoite のソース生成とし、生成コードの実行は行わない。

2026-10-25 23:33 JST までに行った生成の確認結果は次のとおりである。取得と API の自動確認、32K での `benitoiteh` の接続確認を終え、その後に文脈長を 64K、128K、192K、256K と段階的に増やして確認した。256K では最初と同じ依頼によるソース生成と修正を再確認し、その後に effort を low、medium、high と変えて同じ生成を試し、さらに low に戻して 2 回測定した。以下の構築手順と提供者設定は、現在の 256K と、BenitoiteHarness が low を明示する設定に合わせている。

| 項目 | 状態 |
|---|---|
| Strata Terminal の導入 | 完了。取得物の SHA-256 と署名の検証を通過 |
| CUDA 用エンジンの導入 | 完了。署名とファイルの検証、試験起動を通過 |
| IQ3_S と MTP 補助ファイルの取得 | 完了。Strata 向けの準備とモデルの読み込みも完了 |
| SSH 切断後の稼働 | 取得中の継続を確認。取得後も別の接続から API の応答を確認 |
| API の自動確認 | 32K・64K・128K・192K・256K で `passed`。現在は 256K、画像なし、API キー有効で応答 |
| `benitoiteh` の提供者設定 | 追加済み。既定の提供者は変更していない |
| `benitoiteh` からの挨拶とソース生成 | 各文脈長で挨拶を確認。32K と 256K の effort 省略時は修正後に静的検査を通過。256K の low・medium・high 明示の各試行では修正なしで通過 |
| `benitoiteh` の effort | 現在は `low` を API へ明示。low の確認より前は省略し、Strata の既定の high 相当を使用 |
| 旧上限を超える入力 | 各段階で確認。現在の 256K 設定では入力 256,027 トークンの要求が正常終了 |
| 生成コードの実行、課題の評価 | 未実施 |
| OpenCode | 接続設定を追加し、モデル一覧への反映と応答の受信を確認。道具の実行は未確認 |
| Codex CLI・Claude Code | API 対応と設定方法の資料確認まで。利用者設定の追加と実接続は未実施 |

## クライアントの設定変更の範囲

本構築と今後の接続確認では、利用者が指定した次の範囲に限って設定を変更する。

| 対象 | 変更してよい範囲 |
|---|---|
| BenitoiteHarness（`benitoiteh`） | 動作確認に必要な設定。権限・承認・サンドボックス・道具の設定も、確認の目的に必要な範囲で扱う |
| OpenCode・Codex CLI・Claude Code | モデル選択・API キー・SSH 転送に関する設定のみ。権限・承認・サンドボックス・道具の有効／無効は変更しない |

OpenCode・Codex CLI・Claude Code では、設定ファイルの編集に加え、補助コマンド、環境変数、起動引数による権限などの間接的な上書きも行わない。これらの設定が動作上の問題に関係する場合も、設定を変更せず、原因と影響を利用者へ報告する。

BenitoiteHarness の動作確認に必要な設定変更の許可は、設計で定めた確認と OS のサンドボックスを弱める許可にはしない。確認する条件と目的を記録し、既存の安全の性質を維持する。

## 環境と版

### 実機で確認した構成

| 項目 | 確認値 |
|---|---|
| OS | Ubuntu 26.04.1 LTS、x86_64 |
| CPU | Core i5-12400、6 コア・12 スレッド、AVX2 対応 |
| GPU | NVIDIA GeForce RTX 3060、12,288 MiB |
| NVIDIA ドライバ | 595.99.02 |
| RAM | 約 123 GiB。構築開始時の利用可能量は約 121 GiB |
| ストレージ | ホームを置く NVMe に約 1.8 TB の空き |
| 取得用のコマンド | `wget`、`sha256sum`、`tar`。`curl` は未導入 |
| 常駐の条件 | ユーザーの `Linger` は無効。OS 再起動後の自動起動は設定しない |

CPU の当初の申告は i5-12500 だったが、実機の `lscpu` は i5-12400 を報告した。以後の測定では実機の値を使う。この環境で課題の成功率や、長い入力を繰り返したときの安定性を評価した結果は、まだない。

### 導入した版と設定

| 項目 | 値 |
|---|---|
| Strata の配布元 | GitHub Releases の `v0.2.4` |
| Strata Terminal | 0.2.4.2、ビルド 0.1.49.2 |
| 推論エンジン | 0.1.48、`linux-x86_64-cuda-cuda13` |
| モデルのリポジトリ | `ISTA-DASLab/Qwen3.8-Flash-Next-GSQ-RCO-GGUF` |
| モデルのリビジョン | `ed59f92082b1e93c0e96d60a8b11aab089b52f09` |
| 量子化版 | IQ3_S のみ |
| 文脈長 | 262,144 トークン（32K、64K、128K、192K を経て変更） |
| BenitoiteHarness の effort | `low`。`reasoning_effort` の送信を有効にして明示 |
| 同時要求 | 1 |
| 画像入力 | 無効 |
| MTP | 有効 |
| チューニング | 行わない |
| プリセット名 | `qwen-iq3s-256k`（以前のプリセットも残す） |

Strata のカタログが指定するモデルのリビジョンを使い、版の固定を外す設定は加えない。Terminal とエンジンの版は別に記録する。参照した Strata のソースは `Niko1221/Strata` のタグ `v0.2.4`（コミット `96ca438`）である。[Terminal の仕様](https://github.com/Niko1221/Strata/blob/v0.2.4/docs/TERMINAL_LAUNCH.md)と[インストーラ](https://github.com/Niko1221/Strata/blob/v0.2.4/packaging/install.sh)を参照した。

IQ3_S は本体 54.8 GB と n-gram 表 28.8 GB の二つの分割ファイルで、計 83.6 GB である。今回の `strata launch --plan` は MTP 補助ファイル約 5.2 GB を含め、約 88.8 GB の取得を報告した。変換済みファイルやエンジンも保存するため、モデルの取得量だけを空き容量の条件にしない。[配布者のファイル構成と容量](https://huggingface.co/ISTA-DASLab/Qwen3.8-Flash-Next-GSQ-RCO-GGUF/blob/ed59f92082b1e93c0e96d60a8b11aab089b52f09/README.md#available-files)

### 保存先と接続先

| 場所 | 内容 |
|---|---|
| 推論サーバの `~/strata-models/models/IQ3_S/` | IQ3_S の分割ファイル二つ |
| 推論サーバの `~/strata-models/` 配下 | モデルの変換済みファイルと補助ファイル |
| 推論サーバの `~/.local/share/strata/` | Terminal とエンジンなど、Strata が管理するファイル |
| 推論サーバの `~/strata-install/` | インストーラ、起動・再開・確認の補助スクリプト、版の記録 |
| 推論サーバの `~/strata-logs/` | 起動ログ、自動確認の結果とログ |
| 両機の `~/.config/strata/api-key` | API キー。ファイルの権限は `0600` |

サーバは `127.0.0.1:8080` で待ち受ける。手元の機械の `127.0.0.1:18080` から SSH で転送し、`benitoiteh` のベース URL を `http://127.0.0.1:18080/v1` とする。LAN 向けの待ち受けや外部公開は設定しない。

## 推論サーバの構築

以下の `llm-server` は SSH の別名の例である。利用者が設定した別名に置き換える。サーバではホーム内だけを変更し、ドライバ、システムパッケージ、Linger の設定を変えない。

### 前提の確認と Terminal の導入

手元から `ssh llm-server` で接続し、サーバ上で確認する。

```sh
cat /etc/os-release
lscpu
nvidia-smi --query-gpu=name,driver_version,memory.total,memory.free --format=csv
free -h
df -h "$HOME"
```

Terminal 版は、対応するエンジンを取得して使う。今回の構築では CUDA Toolkit やコンパイラをシステムに導入していない。一般の必要条件は [Strata の導入資料](https://github.com/Niko1221/Strata/blob/v0.2.4/docs/INSTALL.md#what-you-need)で確認する。

```sh
mkdir -p "$HOME/strata-install/tmp" "$HOME/strata-models" "$HOME/strata-logs"
export TMPDIR="$HOME/strata-install/tmp"
export STRATA_FEED="https://github.com/Niko1221/Strata/releases/download/v0.2.4"
cd "$HOME/strata-install"
wget -O install.sh "$STRATA_FEED/install.sh"
less install.sh
```

取得元とインストーラの処理を確認してから実行する。

```sh
sh install.sh
export PATH="$HOME/.local/bin:$PATH"
strata --version
strata launch --help
```

ハッシュと署名の検証に失敗した場合は、その検証を外して続けない。これは取得物の整合性を確かめる手順であり、配布元自体の信頼性を保証するものではない。

### API キーと起動設定

サーバで API キーを作る。既存のキーがある場合は作り直さず、権限を確認して使う。

```sh
mkdir -p "$HOME/.config/strata"
chmod 700 "$HOME/.config/strata"
python3 - <<'PY'
import os
import secrets
from pathlib import Path

path = Path.home() / ".config/strata/api-key"
fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, "w") as f:
    f.write(secrets.token_urlsafe(32) + "\n")
PY
```

次の内容を `~/strata-install/start.sh` として保存し、`chmod 700 ~/strata-install/start.sh` を実行する。

```sh
#!/bin/sh
set -eu
export PATH="$HOME/.local/bin:$PATH"
export TMPDIR="$HOME/strata-install/tmp"
export STRATA_UPDATE_FEED="https://github.com/Niko1221/Strata/releases/download/v0.2.4"
export STRATA_API_KEY="$(cat "$HOME/.config/strata/api-key")"
exec "$HOME/.local/bin/strata" launch \
  --family qwen --model iq3_s \
  --data-dir "$HOME/strata-models" \
  --context 256k --parallel 1 --vision off --tune skip \
  --access local --port 8080 --api-key-env STRATA_API_KEY \
  --save-preset qwen-iq3s-256k --feed "$STRATA_UPDATE_FEED" \
  --yes --no-tui "$@"
```

最初に `--plan` を付け、指定の配布者の IQ3_S、保存先、取得量、対応エンジンを確認する。

```sh
"$HOME/strata-install/start.sh" --plan
```

確認後にバックグラウンドで起動する。取得中の同じ処理がないことを確かめてから実行する。

```sh
nohup "$HOME/strata-install/start.sh" \
  >"$HOME/strata-logs/launch.log" 2>&1 </dev/null &
tail -f "$HOME/strata-logs/launch.log"
```

`--no-tui` は起動完了後にサーバをバックグラウンドに残す。取得や変換が長くても経過時間だけでは失敗と判断せず、ログと取得済みチャンクの増加を調べる。再開は同じ起動設定で行い、部分ファイルを消さない。[非対話での起動仕様](https://github.com/Niko1221/Strata/blob/v0.2.4/docs/AI_SETUP.md#4-set-up-and-serve-without-questions)

停止には `~/.local/bin/strata launch --stop` を使う。`nohup` だけであらゆる OS のログアウト処理を回避できるとは扱わず、別の接続から継続稼働を確認する。OS 再起動後は手動で起動する。

## BenitoiteHarness の接続

### SSH のポート転送と認証

手元の機械で転送を開始し、利用中は維持する。

```sh
ssh -N \
  -o ExitOnForwardFailure=yes \
  -o ServerAliveInterval=30 -o ServerAliveCountMax=3 \
  -L 127.0.0.1:18080:127.0.0.1:8080 \
  llm-server
```

API キーは SSH で手元へ複製し、権限 `0600` で `~/.config/strata/api-key` に保存する。値を端末へ表示したり、設定ファイルへ直接書いたりしない。初回の複製例は次のとおりである。既存のキーは上書きしない。

```sh
mkdir -p "$HOME/.config/strata"
chmod 700 "$HOME/.config/strata"
(
  set -eu
  umask 077
  set -C
  ssh llm-server 'cat "$HOME/.config/strata/api-key"' \
    >"$HOME/.config/strata/api-key"
)
export STRATA_API_KEY="$(cat "$HOME/.config/strata/api-key")"
```

手元に `curl` がある場合の接続確認は次のとおりである。

```sh
curl -fsS -H "Authorization: Bearer $STRATA_API_KEY" \
  http://127.0.0.1:18080/v1/models
```

モデルが読み込まれ、`qwen3.8-flash-next-iq3_s` が返ることを確認する。接続できないときはモデルの読み込み状態と SSH 転送を確認する。

### 提供者の設定と最初の依頼

既存の利用者設定をバックアップし、`~/.config/benitoiteh/config.toml` に次を追加する。`XDG_CONFIG_HOME` を指定している場合は、その下の `benitoiteh/config.toml` を使う。既定の提供者は変えない。

```toml
[providers.strata]
kind = "openai-chat"
base_url = "http://127.0.0.1:18080/v1"
model = "qwen3.8-flash-next-iq3_s"
context_window = 262144
api_key = { env = "STRATA_API_KEY" }
effort = "low"
capabilities = { reasoning_effort = true }
```

これは既存の `openai-chat` アダプタで `POST /v1/chat/completions` を使う設定であり、専用アダプタの追加は要らない。設定全体と Benitoite の処理系の指定は [CLI と設定ファイル](../design/05-interface/05-01-cli.md#設定ファイル)を参照する。

`capabilities.reasoning_effort = true` は、設定や `--effort` の値を API の `reasoning_effort` に付ける指定である。これがない場合、`openai-chat` は effort を要求に付けない。サーバ全体の既定値と OpenCode の選択は変更せず、BenitoiteHarness の要求だけを low にする。

最初の依頼は空の確認用ディレクトリで行う。以下の実行ファイルのパスを、用意した `benitoiteh` に置き換える。

```sh
BENITOITEH_BIN=/path/to/benitoiteh
mkdir -p "$HOME/benitoiteh-local-llm-check"
"$BENITOITEH_BIN" exec \
  --mode restricted --provider strata --max-turns 1 --max-run-tokens 32768 \
  --project "$HOME/benitoiteh-local-llm-check" \
  '道具を使わず、日本語で短く挨拶してください。'
```

挨拶に続いて、Benitoite のソースを応答の本文として返させる。依頼文の「道具を使わない」という指定自体は実行の拒否を保証しないため、スクリプトの実行を承認せず、サンドボックスを外さない。今回の初期確認に、生成コードの実行結果や課題の成功率は含めない。

## 取得後の確認と再開

### BenitoiteHarness からの接続確認の結果

2026-10-25 22:05〜22:07 JST に、`benitoiteh` 0.0.1 と Benitoite 0.0.7 を使い、SSH 転送を通じて IQ3_S へ接続した。この時点のサーバと提供者設定の文脈長は 32K である。利用者の普段の設定とセッションの記録を混ぜないよう、確認専用の `XDG_CONFIG_HOME`・`XDG_STATE_HOME`・`TMPDIR` と空のプロジェクトのディレクトリを用意した。利用者・プロジェクトの Skill とプラグインは読み込まない設定とした。処理系が返す Benitoite の Skill は通常どおりシステムプロンプトに含めた。

制限モードと OS のサンドボックスを有効にし、スクリプト実行のエフェクトは許可していない。`benitoiteh` の実行ファイルの SHA-256 は `bc1af0106905e36ffdacc2640dd16770a02855dd1d795cfa26ce1548762b9c31` である。

| 確認 | 結果 | 起動から終了までの時間 |
|---|---|---:|
| 日本語の挨拶 | 指定した挨拶が返り、1 番で正常終了。道具の呼び出しは 0 回 | 約 2.28 秒 |
| 整数を加算する関数と結果を表示する `main` の生成 | ソースが返り、1 番で正常終了。道具の呼び出しは 0 回 | 約 8.58 秒 |
| 型診断を返して修正を要求 | `benitoite_reference` を 2 回呼び、1 番の上限で停止 | 約 4.30 秒 |
| 同じ修正セッションの再開 | 参照結果を保持して修正ソースを返し、正常終了。追加の道具の呼び出しは 0 回 | 約 7.74 秒 |

初回ソースは `Console.writeLine` に整数を直接渡しており、実行しない `benitoite check --report=json` が型の不一致 `E0401` を報告した。診断をモデルに返し、参照結果を保持したセッションを再開すると、整数を `Integer.toString` で文字列へ変換するソースが返った。そのソースは同じ静的検査で終了コード 0、`complete: true`、診断なし、`executed: false` となった。宣言したエフェクトの上限は `Console.Write` だけである。

挨拶の最初の試行では、確認側の `--max-run-tokens 8192` が初期指示と道具の定義を含む入力約 9,350 トークンを下回り、応答を受けた後に上限到達で停止した。上表は `--max-run-tokens 32768` に直してからの結果である。修正の再開時には `--max-turns 3` を指定した。初期の上限到達を含めて、今回送った LLM への要求は計 5 回であり、外部の有料 API は呼んでいない。

この確認で、`benitoiteh` の要求送信、ストリーミングの受信、参照用の道具の呼び出しと結果の返却、セッション再開後の応答を確かめた。生成コードの実行、ファイルの編集を伴う課題、課題の成功率の評価は行っていない。上表の時間は確認時の個別の値であり、比較のための性能測定ではない。生成ソース、API の応答、セッションの生の記録はリポジトリ外の確認用ディレクトリに残し、公開文書にはこの要約だけを記録する。

### 構築時に配置した補助コマンド

今回の環境には、取得処理と別に `~/strata-install/verify.py` をバックグラウンドで配置した。モデルの読み込みを最大48時間待ち、ローカルの推論 API に五つの要求を送る。確認する内容は、キーなしの要求の拒否、挨拶、本文・使用量・終了通知を含むストリーミング、道具の呼び出し形式、模擬の結果を返した後の応答、Benitoite のソース生成である。道具の結果は固定の値であり、モデルの出力をコマンドとして実行しない。

以下は今回の環境に置いた補助コマンドであり、Strata の標準コマンドでも、このリポジトリに同梱するスクリプトでもない。別の環境を作り直すときは、上の構築手順に加え、同じ確認を行う補助を別途用意する。

| 場所 | 用途 |
|---|---|
| 推論サーバの `~/strata-install/status.py` | 取得・自動確認プロセス、完了済みチャンク、自動確認の状態を表示 |
| 推論サーバの `~/strata-install/resume.py` | 同じ取得処理が動いていないことを確認して起動・再開 |
| 推論サーバの `~/strata-install/verify.py` | 読み込み後の API の自動確認 |
| 推論サーバの `~/strata-install/stop.sh` | 読み込み済みサーバの停止 |
| 推論サーバの `~/strata-install/deployment.json` | Terminal・エンジン・モデルの版と設定の記録 |
| 手元の `~/.local/bin/strata-local` | SSH 転送、キーの読み込み、状態表示、クライアントの起動 |

手元から状態と結果を確認する。

```sh
~/.local/bin/strata-local status
ssh llm-server 'cat ~/strata-logs/verification.json'
```

結果の `state` は `waiting`（読み込み待ち）、`checking`（確認中）、`passed`（API の確認に合格）、`failed`（失敗）のいずれかである。`failed` では `error` と `~/strata-logs/verification.log` を調べる。待機の期限を過ぎた場合は、取得・起動を再開したうえで `verify.py` を再実行する。

```sh
ssh llm-server 'python3 ~/strata-install/resume.py'
```

`passed` は `benitoiteh` の動作確認の完了を表さない。次に `benitoiteh` からの挨拶とソース生成を確かめる。今回配置した補助コマンドでは、次のように起動できる。

```sh
~/.local/bin/strata-local benitoiteh
```

補助コマンドは提供者 `strata` を選び、API キーを環境変数から渡す。手元の `benitoiteh` の場所は補助コマンド内の設定なので、実行ファイルを移した場合は更新する。

### 64K への拡張確認

2026-10-25 22:14〜22:17 JST に、利用者の指示により文脈長を 32K から 64K（65,536 トークン）へ変更した。`strata launch --plan` がモデルの再取得なしと報告し、処理中の要求がないことを確認してから再起動した。モデル、MTP、同時要求数、画像入力、チューニングの条件は変えていない。

サーバの `start.sh` を `--context 64k` とプリセット `qwen-iq3s-64k` に変更し、手元の `providers.strata.context_window` と OpenCode のモデル設定の `limit.context` も 65,536 に揃えた。`--max-run-tokens` は一連の処理で使う入力と出力の累計の上限なので、文脈長とは別に扱う。今回の挨拶の再確認では 32,768 のまま使った。

| 確認 | 結果 |
|---|---|
| 読み込みと API の応答 | `/health.max_context` と `/v1/models` の `meta.n_ctx` がともに 65,536。`loaded: true` |
| API の自動確認 | 期待する文脈長を 65,536 に更新して再実行し、`passed` |
| 32K を超える入力 | 推論 API が入力 39,941 トークン、出力 8 トークンを報告。先頭に置いた確認用マーカーを正しく返し、正常終了 |
| 長い入力の応答時間 | 最初の本文まで約 37.52 秒、要求全体で約 37.73 秒。入力のキャッシュ利用は 0 トークン |
| 長い入力の処理後のメモリ | VRAM 11,561 / 12,288 MiB。RAM は使用中約 54 GiB、利用可能約 68 GiB |
| `benitoiteh` からの挨拶 | 64K の提供者設定で 1 番、終了コード 0。道具の呼び出しは 0 回。起動から終了まで約 13.01 秒 |

長い入力の確認には、番号付きの無害な文を並べたデータを使い、末尾で先頭のマーカーを返すよう求めた。Anthropic 互換の計数 API は入力を 39,981 トークンと数えたが、実際に使った Chat Completions API の報告は 39,941 トークンだった。上表では実際の推論要求の値を使う。結果は推論サーバの `~/strata-logs/context-64k-long-input.json` に保存した。

この確認は、64K 設定で読み込めることと、旧上限の 32K を超える一つの入力を処理できることを確かめたものである。64K の上限近くの入力、長文での道具の呼び出し、繰り返しの安定性やモデルの精度は未評価である。32K と 64K の挨拶ではキャッシュの利用状況が違うため、個別の所要時間を設定の速度差とは扱わない。

変更前の起動・確認スクリプト、版の記録、API の確認結果、手元の設定は、元のファイル名に `.before-64k-<日時>` を付けて保存した。32K へ戻す場合は、起動設定を `--context 32k` とプリセット `qwen-iq3s-32k` に、両クライアントの文脈長を 32,768 に戻して再起動する。自動確認の期待値も戻し、サーバとクライアントの値を一致させる。

### 128K への拡張確認

2026-10-25 22:21〜22:24 JST に、利用者の指示により文脈長を 64K から 128K（131,072 トークン）へ変更した。モデルの再取得は不要で、処理中の要求がないことを確認してから再起動した。モデル、MTP、同時要求数、画像入力、チューニングの条件は変えていない。

サーバの起動設定を `--context 128k` とプリセット `qwen-iq3s-128k` に変更し、手元の `providers.strata.context_window` と OpenCode の `limit.context` を 131,072 に揃えた。API の自動確認の期待値も更新して再実行した。`benitoiteh` からの短い挨拶では、文脈長とは別の予算である `--max-run-tokens` は 32,768 のまま使った。

| 確認 | 結果 |
|---|---|
| 読み込みと API の応答 | `/health.max_context` と `/v1/models` の `meta.n_ctx` がともに 131,072。`loaded: true` |
| API の自動確認 | `passed` |
| 64K を超える入力 | 推論 API が入力 78,237 トークン、出力 9 トークンを報告。先頭に置いた確認用マーカーを正しく返し、正常終了 |
| 長い入力の応答時間 | 最初の本文まで約 75.67 秒、要求全体で約 75.84 秒。入力のキャッシュ利用は 0 トークン |
| 長い入力の処理後のメモリ | VRAM 11,561 / 12,288 MiB。RAM は使用中約 55 GiB、利用可能約 67 GiB |
| `benitoiteh` からの挨拶 | 128K の提供者設定で 1 番、終了コード 0。道具の呼び出しは 0 回。起動から終了まで約 13.55 秒 |

長い入力には 64K の確認と同じ形の無害な番号付きデータを使い、量を増やした。計数 API は入力を 78,277 トークンと数えたが、上表は実際の Chat Completions の報告値である。結果は推論サーバの `~/strata-logs/context-128k-long-input.json` に保存した。

この確認は、128K 設定で読み込めることと、旧上限の 64K を超える一つの入力を処理できることを確かめたものである。128K の上限近くの入力、長文での道具の呼び出し、繰り返しの安定性やモデルの精度は未評価である。各段階の入力の長さやキャッシュの条件が異なるため、所要時間を文脈長の設定だけによる速度差とは扱わない。

変更前の設定と API の確認結果は `.before-128k-<日時>` の接尾辞で保存した。64K へ戻す場合は、起動設定を `--context 64k` とプリセット `qwen-iq3s-64k` に、両クライアントの文脈長と自動確認の期待値を 65,536 に戻して再起動する。

### 192K への拡張確認

2026-10-25 22:27〜22:32 JST に、利用者の指示により文脈長を 128K から 192K（196,608 トークン）へ変更した。`strata launch --plan` でこの長さを指定でき、モデルの再取得が不要であることを確認した。処理中の要求がないことを確認してから再起動し、モデル、MTP、同時要求数、画像入力、チューニングの条件は変えていない。

サーバを `--context 192k` とプリセット `qwen-iq3s-192k` で起動し、手元の `providers.strata.context_window` と OpenCode の `limit.context` を 196,608 に揃えた。API の自動確認の期待値も更新して再実行した。`benitoiteh` からの短い挨拶では、文脈長とは別の予算である `--max-run-tokens` は 32,768 のまま使った。

| 確認 | 結果 |
|---|---|
| 読み込みと API の応答 | `/health.max_context` と `/v1/models` の `meta.n_ctx` がともに 196,608。`loaded: true` |
| API の自動確認 | `passed` |
| 128K を超える入力 | 推論 API が入力 158,737 トークン、出力 9 トークンを報告。先頭に置いた確認用マーカーを正しく返し、正常終了 |
| 長い入力の応答時間 | 最初の本文まで約 163.29 秒、要求全体で約 163.51 秒。入力のキャッシュ利用は 0 トークン |
| 長い入力の処理後のメモリ | VRAM 11,573 / 12,288 MiB。RAM は使用中約 56 GiB、利用可能約 66 GiB |
| `benitoiteh` からの挨拶 | 192K の提供者設定で 1 番、終了コード 0。道具の呼び出しは 0 回。起動から終了まで約 14.39 秒 |

長い入力には前段階と同じ形の無害な番号付きデータを使い、量を増やした。計数 API は入力を 158,777 トークンと数えたが、上表は実際の Chat Completions の報告値である。結果は推論サーバの `~/strata-logs/context-192k-long-input.json` に保存した。

この確認は、192K 設定で読み込めることと、旧上限の 128K を超える一つの入力を処理できることを確かめたものである。192K の上限近くの入力、長文での道具の呼び出し、繰り返しの安定性やモデルの精度は未評価である。長い入力の内容量やキャッシュの条件が異なるため、個別の所要時間を文脈長の設定だけによる速度差とは扱わない。

変更前の設定と API の確認結果は `.before-192k-<日時>` の接尾辞で保存した。128K へ戻す場合は、起動設定を `--context 128k` とプリセット `qwen-iq3s-128k` に、両クライアントの文脈長と自動確認の期待値を 131,072 に戻して再起動する。

### 256K への拡張確認

2026-10-25 22:35〜22:43 JST に、利用者の指示により文脈長を 192K から 256K（262,144 トークン）へ変更した。モデルの再取得は不要で、処理中の要求がないことを確認してから再起動した。モデル、MTP、同時要求数、画像入力、チューニングの条件は変えていない。

サーバの起動設定を `--context 256k` とプリセット `qwen-iq3s-256k` に変更し、手元の `providers.strata.context_window` と OpenCode の `limit.context` を 262,144 に揃えた。API の自動確認の期待値も更新して再実行した。`benitoiteh` からの短い挨拶では、文脈長とは別の予算である `--max-run-tokens` は 32,768 のまま使った。

| 確認 | 結果 |
|---|---|
| 読み込みと API の応答 | `/health.max_context` と `/v1/models` の `meta.n_ctx` がともに 262,144。`loaded: true` |
| API の自動確認 | `passed` |
| 上限近くの入力 | 推論 API が入力 256,027 トークン、出力 9 トークンを報告。先頭に置いた確認用マーカーを正しく返し、正常終了 |
| 長い入力の応答時間 | 最初の本文まで約 283.28 秒、要求全体で約 283.46 秒。入力のキャッシュ利用は 0 トークン |
| 長い入力の処理後のメモリ | VRAM 11,561 / 12,288 MiB。RAM は使用中約 57 GiB、利用可能約 66 GiB |
| `benitoiteh` からの挨拶 | 256K の提供者設定で 1 番、終了コード 0。道具の呼び出しは 0 回。起動から終了まで約 15.08 秒 |

長い入力には前段階と同じ形の無害な番号付きデータを使い、末尾で先頭のマーカーを返すよう求めた。計数 API は入力を 256,067 トークンと数えたが、上表は実際の Chat Completions の報告値である。入力だけで設定した文脈長の約 97.7% を使い、入力と出力の合計は 256,036 トークンだった。結果は推論サーバの `~/strata-logs/context-256k-long-input.json` に保存した。

これで 32K から 256K までの段階的な読み込みと短い接続確認、各段階で旧上限を超える入力の処理を確認した。最終段階では上限近くの入力も処理できたが、262,144 トークンちょうどの境界、長い文脈での道具の呼び出し、繰り返しの安定性やモデルの精度は未評価である。長い入力の内容量やキャッシュの条件が異なるため、個別の所要時間を文脈長の設定だけによる速度差とは扱わない。

変更前の設定と API の確認結果は `.before-256k-<日時>` の接尾辞で保存した。192K へ戻す場合は、起動設定を `--context 192k` とプリセット `qwen-iq3s-192k` に、両クライアントの文脈長と自動確認の期待値を 196,608 に戻して再起動する。

### 256K でのソース生成の再確認

2026-10-25 22:46〜22:48 JST に、文脈長を 256K のまま維持し、32K での最初の確認と同じ依頼文でソース生成を再確認した。依頼は、整数の引数 `a` と `b` の和を返す純粋関数 `add` と、`add(a: 1, b: 2)` の結果を標準出力に一行で出す `main` を、応答本文のコードブロックとして生成するものである。初回の記録と依頼文が一致することをバイト列で確認した。

確認専用の設定と状態の保存先、空のプロジェクトのディレクトリを使い、制限モードと OS のサンドボックスを有効にした。Benitoite の処理系は 0.0.7、`benitoiteh` の実行ファイルは 32K の確認時と同じ SHA-256 である。サーバの再起動や、利用者の提供者設定の変更は行っていない。

| 確認 | 結果 |
|---|---|
| 初回生成 | 1 番、終了コード 0。道具の呼び出し 0 回。起動から終了まで約 7.95 秒 |
| 初回の静的検査 | 整数を直接 `Console.writeLine` へ渡しており、32K の初回と同じ型の不一致 `E0401`。終了コード 2、`executed: false` |
| 診断を返した修正 | 同じセッションを再開し、文字列補間を使う修正ソースを受信。1 番、終了コード 0、道具の呼び出し 0 回。約 8.77 秒 |
| 修正後の静的検査 | 終了コード 0、`complete: true`、診断なし、`executed: false`。宣言したエフェクトの上限は `Console.Write` |

初回生成の入力は 9,439 トークン、出力は 266 トークン、修正の入力は 9,616 トークン、出力は 327 トークンだった。設定した文脈長は 256K だが、この課題自体は長い入力を要しない。長い入力の処理確認とは分けて扱う。

今回の LLM への要求は計 2 回であり、初回の `--max-run-tokens` は 32,768、修正では参照の往復に備えて 65,536 とした。修正の `--max-turns` は 3 としたが、実際には 1 番で完了し、道具は呼ばれなかった。これらは確認用の累計予算と番数の上限であり、文脈長の変更ではない。

生成コードの実行、モデルによるファイルの作成・編集、課題の成功率の評価は行っていない。初回の型誤りも含めて記録し、文脈長を増やすとコード生成の精度が上がるとは、この結果から判断しない。生成ソースと生の記録はリポジトリ外の確認用ディレクトリに残す。

### effort の設定と low でのソース生成

23:10 JST の変更前は、BenitoiteHarness の Strata 用設定に effort の指定も `capabilities.reasoning_effort = true` もなかった。要求に effort を付けず、上書きのない Strata の既定値である high 相当を使っていた。稼働中のテンプレートでは、省略時と API の `high` がともに内部の `xhigh` に対応することも確認した。Strata の API は `none`・`low`・`medium`・`high` を受け付ける。[思考レベルの仕様](https://github.com/Niko1221/Strata/blob/v0.2.4/docs/DETAILS.md#using-it)

これまでの条件は、次のように区別する。

| 確認の種類 | effort |
|---|---|
| 変更前の `benitoiteh` からの挨拶・ソース生成・修正 | 要求では省略。Strata の既定の high 相当 |
| サーバへの API 自動確認と長い入力の確認 | `none` を明示 |
| 以下の `benitoiteh` によるソース生成 | `low` を明示 |

2026-10-25 23:10〜23:11 JST に、利用者の指示で手元の Strata 用提供者設定をバックアップし、`effort = "low"` と `capabilities = { reasoning_effort = true }` を加えた。文脈長は 256K のままで、サーバを再起動していない。Strata 全体の既定値と OpenCode の effort の設定は変更していない。

最初と同じ依頼文でソースを 1 回生成した。依頼文は、直前の high 相当の確認の記録とバイト列で一致した。確認専用の設定にも low と送信機能の指定を加え、制限モードと OS のサンドボックスを有効にした。`benitoiteh` の実行ファイルと Benitoite 0.0.7 は、前の確認と同じものを使った。

| 確認 | low での結果 |
|---|---|
| ソース生成 | 1 番、終了コード 0。道具の呼び出しは 0 回。起動から終了まで約 30.75 秒 |
| 初回の静的検査 | 終了コード 0、`complete: true`、診断なし、`executed: false` |
| ソースの形 | `add` は整数を返し、`main` は文字列補間を使って表示する形 |
| 宣言したエフェクトの上限 | `Console.Write` |
| 修正要求・生成コードの実行 | どちらも行っていない |

今回の low と、直前の 256K の high 相当での初回生成は次の条件だった。修正の要求は比較表に含めない。

| 項目 | high 相当 | low |
|---|---:|---:|
| 入力トークン | 9,439 | 9,429 |
| キャッシュから再利用した入力 | 9,055 | 0 |
| 新たに処理した入力 | 384 | 9,429 |
| 入力処理時間 | 約 1.82 秒 | 約 13.07 秒 |
| 出力トークン（推論を含む） | 266 | 935 |
| 生成時間 | 約 6.00 秒 | 約 17.53 秒 |
| 起動から終了まで | 約 7.95 秒 | 約 30.75 秒 |
| 実効 prefill | 約 210.8 tok/s | 約 721.5 tok/s |
| decode | 44.3 tok/s | 53.3 tok/s |
| 初回ソースの静的検査 | 型の不一致 `E0401` | 診断なし |

この low の試行は初回の生成で静的検査を通ったが、総時間は長くなった。キャッシュの利用状況と出力トークン数が異なるため、この 1 回ずつの結果から effort による速度や精度の優劣は判断しない。low は思考の深さを指示する値であり、生成するトークン数の厳密な上限ではない。今回も、low の出力トークン数は high 相当の初回より多かった。

今回の LLM への要求は計 1 回で、`--max-turns 1`・`--max-run-tokens 32768` の範囲で完了した。メトリクスの読み取りと静的検査は追加の推論要求ではない。生の記録と計測値はリポジトリ外に残した。変更前の提供者設定は `config.toml.before-low-<日時>` に保存している。high を明示して使う場合は、送信機能を有効のまま `effort = "high"` に変える。

### medium でのソース生成とキャッシュの確認

2026-10-25 23:17〜23:20 JST に、利用者の指示により BenitoiteHarness の Strata 用設定を low から medium へ変更した。変更前の設定は `config.toml.before-medium-<日時>` に保存した。文脈長 256K と `reasoning_effort` の送信機能は維持し、Strata 全体と OpenCode の effort は変更していない。

最初と同じ依頼文を 1 回送り、初回の生成で静的検査を通った。ただし、キャッシュ再利用は 0 トークンだったため、再利用の有無を確かめる目的で、medium のまま同じ依頼をもう 1 回送った。依頼文が一致することはバイト列で確認した。各試行には別の空のプロジェクトのディレクトリとセッションを使い、実行ファイル、処理系、制限モードとサンドボックスの条件は前の確認と揃えた。

| 項目 | medium の初回 | medium の繰り返し |
|---|---:|---:|
| 入力トークン | 9,407 | 9,407 |
| キャッシュから再利用した入力 | 0 | 9,017 |
| 新たに処理した入力 | 9,407 | 390 |
| 出力トークン（推論を含む） | 193 | 218 |
| 入力処理時間 | 約 13.08 秒 | 約 1.70 秒 |
| 生成時間 | 約 3.94 秒 | 約 4.91 秒 |
| 起動から終了まで | 約 17.17 秒 | 約 6.73 秒 |
| 実効 prefill | 約 719.3 tok/s | 約 229.7 tok/s |
| decode | 48.9 tok/s | 44.4 tok/s |
| 初回ソースの静的検査 | 診断なし | 診断なし |

両方とも 1 番・終了コード 0、道具の呼び出し 0 回で正常終了し、文字列補間を使ったソースを返した。Benitoite 0.0.7 の静的検査は両方とも終了コード 0、`complete: true`、診断なし、`executed: false` だった。宣言したエフェクトの上限は `Console.Write` である。修正要求や生成コードの実行は行っていない。LLM への要求は計 2 回で、各試行を `--max-turns 1`・`--max-run-tokens 32768` の範囲で行った。

medium の初回は入力を全て新規に処理し、同じ medium の繰り返しでは大半を再利用した。Strata の資料は、effort の指示を既定ではプロンプトの先頭に置くため、effort の切り替えで読み直しが生じると説明している。今回はこの指示の位置を変えていない。[effort とキャッシュの説明](https://github.com/Niko1221/Strata/blob/v0.2.4/docs/DETAILS.md#using-it)

繰り返しの総時間は短くなったが、生成速度自体が上がったとは扱わない。実際には新規入力が減って入力処理が短くなり、出力は少し増え、生成時間も長くなった。新規入力が小さい要求の実効 prefill も、全入力を処理した要求と単純には比較しない。各 effort の試行は、出力の量とキャッシュの条件が揃った比較実験ではなく、この課題の個別の確認結果として記録する。

### high を明示したソース生成 2 回

2026-10-25 23:25〜23:26 JST に、利用者の指示により BenitoiteHarness の Strata 用設定を medium から high へ変更し、同じ課題の生成を 2 回行った。変更前の設定は `config.toml.before-high-<日時>` に保存した。文脈長は 262,144 トークンのまま維持し、`reasoning_effort` の送信機能を有効にした。Strata 全体と OpenCode の effort は変更していない。

両試行の依頼文は最初の確認とバイト列で一致した。各試行には別の空のプロジェクトのディレクトリとセッションを使い、実行ファイル、処理系、制限モードとサンドボックスの条件は前の確認と揃えた。

| 項目 | high の 1 回目 | high の 2 回目 |
|---|---:|---:|
| 入力トークン | 9,440 | 9,442 |
| キャッシュから再利用した入力 | 9,055 | 9,055 |
| 新たに処理した入力 | 385 | 387 |
| 出力トークン（推論を含む） | 431 | 289 |
| 入力処理時間 | 約 1.69 秒 | 約 1.70 秒 |
| 生成時間 | 約 9.05 秒 | 約 6.83 秒 |
| 起動から終了まで | 約 10.84 秒 | 約 8.69 秒 |
| 実効 prefill | 約 227.5 tok/s | 約 227.1 tok/s |
| decode | 47.6 tok/s | 42.3 tok/s |
| 初回ソースの静的検査 | 診断なし | 診断なし |

両方とも 1 番・終了コード 0、道具の呼び出し 0 回で正常終了した。返されたコードは両方とも同じで、整数の和を返す `add` を定義し、`main` で結果を文字列補間して `Console.writeLine` に渡す形だった。Benitoite 0.0.7 の静的検査は両方とも終了コード 0、`complete: true`、診断なし、`executed: false` であり、宣言したエフェクトの上限は `Console.Write` だった。修正要求や生成コードの実行は行っていない。

今回は high へ切り替えた 1 回目から、両方とも 9,055 トークンを再利用した。effort の変更後に必ずキャッシュが無効になるとは、この確認結果からはいえない。再利用元の要求はメトリクスから特定していないため、過去のどの要求のキャッシュを使ったかは断定しない。

2 回目は総時間が短かったが、入力処理時間はほぼ同じで、出力トークン数と生成時間が減った。decode の tok/s 自体は 2 回目の方が低く、総時間の短縮を生成速度の改善とは扱わない。medium との比較も、出力量と入力全体の条件を揃えた比較実験ではないため、effort ごとの速度や精度の優劣は判断しない。

LLM への要求は指示どおり計 2 回で、各試行を `--max-turns 1`・`--max-run-tokens 32768` の範囲で行った。静的検査とメトリクスの読み取りは追加の推論要求ではない。生の記録と計測値はリポジトリ外に残した。

### low に戻したソース生成 2 回

2026-10-25 23:32〜23:33 JST に、利用者の指示により BenitoiteHarness の Strata 用設定を high から low に戻し、同じ課題を 2 回測定した。変更前の設定は `config.toml.before-low-repeat-<日時>` に保存した。文脈長は 262,144 トークンのままで、サーバの再起動やキャッシュの消去は行わず、既存のキャッシュがある状態で測定した。

両試行の依頼文は最初の確認とバイト列で一致した。各試行には別の空のプロジェクトのディレクトリとセッションを使い、実行ファイル、処理系、制限モードとサンドボックスの条件は前の確認と揃えた。送信機能を有効にした low の設定を各試行にも明示した。

| 項目 | low に戻した 1 回目 | low に戻した 2 回目 |
|---|---:|---:|
| 入力トークン | 9,434 | 9,431 |
| キャッシュから再利用した入力 | 9,043 | 9,043 |
| 新たに処理した入力 | 391 | 388 |
| 出力トークン（推論を含む） | 923 | 846 |
| 入力処理時間 | 約 1.73 秒 | 約 1.73 秒 |
| 生成時間 | 約 17.74 秒 | 約 16.00 秒 |
| 起動から終了まで | 約 19.60 秒 | 約 17.88 秒 |
| 実効 prefill | 約 225.6 tok/s | 約 224.2 tok/s |
| decode | 52.0 tok/s | 52.9 tok/s |
| 初回ソースの静的検査 | 診断なし | 診断なし |

両方とも 1 番・終了コード 0、道具の呼び出し 0 回で正常終了し、文字列補間を使って和を表示するソースを返した。Benitoite 0.0.7 の静的検査は両方とも終了コード 0、`complete: true`、診断なし、`executed: false` であり、宣言したエフェクトの上限は `Console.Write` だった。修正要求や生成コードの実行は行っていない。LLM への生成要求は指示どおり計 2 回で、各試行を `--max-turns 1`・`--max-run-tokens 32768` の範囲で行った。

今回も切り替え後の 1 回目から入力を再利用した。ただし、再利用元の要求は特定していない。以前の high 相当のキャッシュを今回の high 明示で使った可能性や、以前の low のキャッシュを今回使った可能性は、この計測値だけでは確定しない。再利用は入力処理に対するものであり、今回の `engine_generated` は各回の出力と同じ 923・846 トークンだった。過去の応答本文をそのまま返したという計測結果ではない。

最初の low では新規入力が 9,429 トークンで入力処理に約 13.07 秒かかったが、今回はキャッシュの利用により約 1.73 秒だった。一方、直前の high 明示の 2 回と比べると、今回は decode の tok/s は高かったが、出力トークン数も多く、総時間は長かった。出力量の異なる個別の試行から、low が常に速い、または high が常に速いとは判断しない。生の記録と計測値はリポジトリ外に残した。

### 32K と 256K のソース生成時間

最初の 32K と、256K での再確認の初回生成を比べると、起動から終了までの時間は次のとおりだった。どちらも effort を要求では省略した high 相当である。修正を依頼した要求と、その後に low・medium・high を明示した要求は、この表に含めない。

| 文脈長の設定 | 入力トークン | キャッシュから再利用した入力 | 出力トークン | 起動から終了まで |
|---|---:|---:|---:|---:|
| 32K | 9,420 | 9,055 | 305 | 約 8.58 秒 |
| 256K | 9,439 | 9,055 | 266 | 約 7.95 秒 |

この確認では、256K 設定での所要時間は 32K と同程度だった。ただし、各設定で 1 回ずつの試行であり、出力トークン数も違うため、文脈長による速度の優劣は判断しない。起動から終了までの時間には入力処理やクライアントの処理も含まれるので、純粋な decode の速度とも分けて扱う。

### prefill・decode の計測値と取得方法

2026-10-25 に、稼働中の Strata の `GET /metrics` から完了済み要求の計測値を読み取った。新しい推論要求は送っていない。`/metrics` は Monitor と同じ情報を返し、実行中の値と完了済み要求の記録を含む。[Strata のメトリクス資料](https://github.com/Niko1221/Strata/blob/v0.2.4/docs/DETAILS.md#using-it)（参照したソース: `Niko1221/Strata`、`v0.2.4`、コミット `96ca438`）。

prefill は入力の新しい部分を処理する速度、decode は出力のトークンを生成する速度である。今回の表の prefill は、要求ごとの `prompt_read` と `prompt_ms` から計算した実効値であり、decode はエンジンが報告した `decode_tok_s` の値である。出力には推論の文も含まれるため、コード本文だけの生成速度とは扱わない。

```text
prefill tok/s = prompt_read / (prompt_ms / 1000)
decode tok/s  = decode_tok_s
```

| 256K 設定での要求と effort | 新たに処理した入力 | キャッシュから再利用した入力 | 出力トークン | prefill（tok/s） | decode（tok/s） |
|---|---:|---:|---:|---:|---:|
| 初回のソース生成（high 相当） | 384 | 9,055 | 266 | 約 210.8 | 44.3 |
| 診断を返したソース修正（high 相当） | 182 | 9,434 | 327 | 約 135.9 | 44.9 |
| ソース生成（low） | 9,429 | 0 | 935 | 約 721.5 | 53.3 |
| ソース生成（medium、初回） | 9,407 | 0 | 193 | 約 719.3 | 48.9 |
| ソース生成（medium、繰り返し） | 390 | 9,017 | 218 | 約 229.7 | 44.4 |
| ソース生成（high 明示、1 回目） | 385 | 9,055 | 431 | 約 227.5 | 47.6 |
| ソース生成（high 明示、2 回目） | 387 | 9,055 | 289 | 約 227.1 | 42.3 |
| ソース生成（low に戻した 1 回目） | 391 | 9,043 | 923 | 約 225.6 | 52.0 |
| ソース生成（low に戻した 2 回目） | 388 | 9,043 | 846 | 約 224.2 | 52.9 |
| 上限近くの長い入力の確認（none） | 256,027 | 0 | 9 | 約 904.2 | 43.7 |

high 相当の初回のソース生成の入力処理時間は 1,821.7 ms、生成時間は 5,999.1 ms だった。prefill は `384 / 1.8217` から約 210.8 tok/s と求める。入力全体の 9,439 トークンを分子にすると、キャッシュから再利用した 9,055 トークンまで新規に処理した扱いになり、速度を過大に見積もる。

新規入力が少ない要求は、固定の処理時間や計測のばらつきの影響を受けやすい。ソース生成の prefill と長い入力の prefill を、そのまま機材の性能差として比べない。また、長い入力の確認は出力が 9 トークンだけなので、その decode の値も短い標本として扱う。表の decode は各要求の個別の値であり、あらゆる課題の生成速度とはしない。32K の要求の分離した prefill・decode の値は、今回の取得対象には含めていない。

手元の機械では、既存の SSH 転送と API キーを使って次のように直近 5 件を確認できる。キーや要求・応答の本文は表示しない。ここで使う `/metrics` は、OpenAI 互換 API のベース URL の `/v1` の下ではなく、サーバの直下にある。

```sh
~/.local/bin/strata-local tunnel

python3 - <<'PY'
import json
from pathlib import Path
from urllib.request import Request, urlopen

key = (Path.home() / ".config/strata/api-key").read_text().strip()
request = Request(
    "http://127.0.0.1:18080/metrics",
    headers={"Authorization": "Bearer " + key},
)
with urlopen(request, timeout=20) as response:
    metrics = json.load(response)

for record in metrics.get("requests", [])[:5]:
    seconds = (record.get("prompt_ms") or 0) / 1000
    new_tokens = record.get("prompt_read")
    prefill = new_tokens / seconds if seconds and new_tokens is not None else None
    print(json.dumps({
        "request_id": record.get("id"),
        "new_input_tokens": new_tokens,
        "reused_input_tokens": record.get("reused"),
        "output_tokens": record.get("output_tokens"),
        "prefill_tok_s": round(prefill, 1) if prefill is not None else None,
        "decode_tok_s": record.get("decode_tok_s"),
    }, ensure_ascii=False))
PY
```

実行中の値は `live.prefill_tok_s_mean` や `live.tok_s_mean` で参照できる。待機中は `null` や 0 になることがあるため、完了済みの要求は `requests` を参照する。今回の表は当日の取得結果であり、再取得時の履歴の一覧は変わりうる。後で比べる値は、モデル・エンジンの版、文脈長、MTP、入力と出力の量、キャッシュの利用状況と合わせて記録する。

### 測定と安全の範囲

API の自動確認は短い応答の時間と使用量を残すが、課題達成率の測定ではない。評価に進むときは[評価の手順](../design/07-quality/07-02-evaluation.md#評価の手順)に従い、課題、試行回数、文脈長、MTP、チューニング、モデルとエンジンの版を固定する。生の記録は公開せず、要約だけを評価結果として残す。

文脈長をさらに変える場合もサーバの設定と `providers.strata.context_window` を揃え、メモリ使用量と入力の処理時間を測る。未測定の長さや速度を、この構成で使える値として報告しない。

モデルのファイル解析と、生成コードの実行は別に扱う。SSH 転送と API キーは推論サーバへの接続を制限するものであり、推論エンジンや生成コードを隔離するサンドボックスではない。今回、Strata 自体を OS のサンドボックスに閉じ込める設定は行っていない。生成コードを実行する段階では、`benitoiteh` の確認と[OS のサンドボックス](../design/03-safety/03-02-sandbox.md)を使う。

## 付随作業: ほかのハーネスからの接続

ほかのハーネスへの対応は、この推論サーバを共用するための付随作業である。BenitoiteHarness の確認を進める条件にはしない。

| クライアント | API | 今回の到達点 |
|---|---|---|
| OpenCode | OpenAI Chat Completions、`/v1/chat/completions` | 設定を追加し、モデル一覧への反映と応答の受信を確認。道具の実行は未確認 |
| Codex CLI | OpenAI Responses、`/v1/responses` | 資料と導入済みサーバの実装でエンドポイントを確認。設定・推論は未実施 |
| Claude Code | Anthropic Messages、`/v1/messages` | 資料と導入済みサーバの実装でエンドポイントを確認。設定・推論は未実施 |

OpenCode は `@ai-sdk/openai-compatible` と `http://127.0.0.1:18080/v1` を使う。今回の `strata-local opencode` は、主の処理と補助の処理の両方にローカルモデルを指定し、API キーを渡し、SSH 転送を確認する。補助コマンドが追加していた権限の上書きは、利用者の指示で削除済みである。現在は `permission`・`permissions` を追加せず、呼び出し元で指定された権限設定もそのまま保持する。[クライアントの設定変更の範囲](#クライアントの設定変更の範囲)に従い、今後の Codex CLI・Claude Code 向けの接続設定でも、権限などを上書きしない。[OpenCode の提供者設定](https://opencode.ai/docs/providers/#custom-provider)

OpenCode の Strata 用設定は `apiKey = "{env:STRATA_API_KEY}"` に相当する環境変数参照を使う。キーは手元の `~/.config/strata/api-key` に保存しているが、通常の `opencode` 起動時に環境変数が未設定なら、このファイルは自動では読み込まれない。構築済みの補助コマンドから起動すると、SSH 転送を確認し、保存済みキーを子プロセスの環境変数へ設定する。

```sh
~/.local/bin/strata-local opencode
```

`Error: missing or wrong API key` が出た場合は、まずこの起動方法を使う。すでに開いている OpenCode には後から環境変数を渡せないため、終了して補助コマンドから起動し直す。新しいキーの発行や、設定ファイルへのキーの直接記入は必要ない。

2026-10-25 の認証確認では、同じ `/v1/models` への要求がキーなしで HTTP 401、保存済みキー付きで HTTP 200 を返し、対象モデルの ID を取得できた。これは保存済みキーと API の認証の確認であり、OpenCode からの推論成功の確認には含めない。

Codex CLI 用の設定は `wire_api = "responses"` とし、ベース URL は `http://127.0.0.1:18080/v1` を使う。Strata の Responses 実装は会話の全履歴を毎回送る方式であり、`previous_response_id`、バックグラウンド実行、応答の取得・削除・キャンセル用 API は対応しない。[Strata の Responses 対応](https://github.com/Niko1221/Strata/blob/v0.2.4/docs/DETAILS.md#the-responses-api-and-codex-cli)、[Codex の提供者設定](https://learn.chatgpt.com/docs/config-file/config-advanced)

Claude Code 用の `ANTHROPIC_BASE_URL` は、`/v1` を付けない `http://127.0.0.1:18080` とする。Strata はこの接続方法を案内しているが、Anthropic は Claude Code から非 Claude モデルへの接続を公式にはサポートしていない。API の存在だけで実接続の成功とは扱わず、クライアントの版ごとに確認する。[Strata の接続資料](https://github.com/Niko1221/Strata/blob/v0.2.4/docs/AI_SETUP.md#8-connect-the-users-apps)、[Claude Code のゲートウェイ資料](https://code.claude.com/docs/en/llm-gateway)

現在の自動確認は Chat Completions だけを対象とする。Codex CLI・Claude Code の確認を行う場合は、各 API のストリーミングと道具の結果の返却を含め、別に実施して記録する。
