# Strata の推論サーバと 3 つの Qwen3.8-Flash-Next モデル

- 状態: 草稿

## 目的と確認状況

この文書は、Debian 13 の推論サーバに Strata Terminal 0.2.12 を導入し、Qwen3.8-Flash-Next の 3 つの量子化版をホストして、DeepSeek Harness（以下 DSH）と `benitoiteh` から切り替えて使うまでの手順と、その環境で確かめたことを記録する。3 つは Unsloth の UD-IQ4_XS-v31、ISTA-DASLab の IQ3_S、SC117 の abliterated IQ3_S である。

この環境は Debian 13 で、OS の更新によりサーバのホームが作り直され、前のモデルと設定は残っていない。

確認の状況は次のとおりである。確認の方法と結果の詳細は「取得後の確認と再開」にまとめる。

| 段階 | 状況 | 記録 |
|---|---|---|
| エンジンの導入と UD-IQ4_XS-v31 の取得 | 完了 | 「取得と再開」 |
| モデルの読み込みと推論 API の応答 | 完了 | 「読み込みと API の応答」 |
| `benitoiteh` からの接続とソース生成 | 完了 | 「`benitoiteh` からの接続確認」 |
| IQ3_S と abliterated IQ3_S の追加 | 完了 | 「IQ3_S と abliterated IQ3_S の追加」 |
| サーバ側の切り替え | 完了 | 「サーバ側の切り替え」 |
| DSH からの切り替え | 完了 | 「DSH の設定」 |

この環境で課題の成功率や、長い入力を繰り返したときの安定性を評価した結果は、まだない。

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
| OS | Debian GNU/Linux 13（trixie）、x86_64 |
| カーネル | 6.12.115+deb13.1-amd64 |
| C ライブラリ | glibc 2.41 |
| CPU | Core i5-12400、6 コア・12 スレッド、AVX2 対応 |
| GPU | NVIDIA GeForce RTX 3060、12,288 MiB |
| NVIDIA ドライバ | 620.70.06 |
| RAM | 約 125 GiB |
| ストレージ | ホームを置く 1.7 TB のボリュームに約 1.6 TB の空き |
| 取得用のコマンド | `wget`、`sha256sum`、`tar`、`jq`。`curl` は未導入 |
| 常駐の条件 | 利用者の `Linger` は無効。OS 再起動後の自動起動は設定しない |

Secure Boot は無効である。DKMS はカーネルモジュールをビルドし、`/var/lib/dkms/mok.key` の鍵で署名する。

### 導入した版と設定

| 項目 | 値 |
|---|---|
| Strata の配布元 | GitHub Releases の `v0.2.12` |
| Strata Terminal | 0.2.12、ビルド 0.1.58 |
| 推論エンジン | 0.1.58、`linux-x86_64-cuda-cuda13`（取得量 723 MB） |
| 文脈長 | 262,144 トークン（3 モデル共通） |
| BenitoiteHarness の effort | `low`。`reasoning_effort` の送信を有効にして明示 |
| 同時要求 | 1 |
| 画像入力 | 無効 |
| 補助の投機的復号 | DFlash2（unsloth と iq3s で共用）。abliterated は使わない |
| チューニング | 行わない |

Strata のカタログが指定するモデルのリビジョンを使い、版の固定を外す設定は加えない。Terminal とエンジンの版は別に記録する。参照した Strata のソースは `Niko1221/Strata` のタグ `v0.2.12` である。[インストールの条件](https://github.com/Niko1221/Strata/blob/v0.2.12/docs/INSTALL.md)と[非対話での起動](https://github.com/Niko1221/Strata/blob/v0.2.12/docs/AI_SETUP.md)を参照した。

UD-IQ4_XS-v31 は四つの分割ファイルで計 93.7 GB である。うち第 2 のファイルが 49.6 GB、第 4 のファイルが 28.8 GB で、第 4 のファイルは `engram/` に置かれた engram 表（`Qwen3.8-Flash-Next-engram-IQ4_NL`）と SHA-256 が一致する。残りの第 1 と第 3 は 10.9 MB と 15.3 GB である。このため `strata launch --plan` は、モデル 93.7 GB と DFlash2 の補助ファイル 1.2 GB を合わせて約 94.9 GB の取得を報告した。変換済みファイルとエンジンも保存するため、モデルの取得量だけを空き容量の条件にしない。[配布者のファイル一覧](https://huggingface.co/unsloth/Qwen3.8-Flash-Next-GGUF/tree/d96201194f103c906994f283f504209dc8d2a10c/UD-IQ4_XS)

### 3 つのモデル

| 名前 | 配布元 | リビジョン | 量子化版 | 大きさ | ポート | プリセット | サーバが名乗る id |
|---|---|---|---:|---:|---:|---|---|
| unsloth | `unsloth/Qwen3.8-Flash-Next-GGUF` | `d96201194f103c906994f283f504209dc8d2a10c`（2026-11-04） | UD-IQ4_XS-v31 | 93.7 GB | 8080 | `qwen-unsloth-iq4xs-256k` | `qwen3.8-flash-next-unsloth-ud-iq4_xs-v31` |
| iq3s | `ISTA-DASLab/Qwen3.8-Flash-Next-GSQ-RCO-GGUF` | カタログの指定に従う | IQ3_S | 83.6 GB | 8081 | `qwen-iq3s-256k` | `qwen3.8-flash-next-iq3_s` |
| abliterated | `SC117/Qwen3.8-Flash-Next-GSQ-RCO-abliterated-GGUF` | `90a3350cd800ec1fe0d1660a38414ad9e985782f`（2026-10-24） | IQ3_S | 83.9 GB | 8082 | `qwen-ablit-iq3s-256k` | `swift-1.5-iq3_s` |

3 つは同じ Qwen3.8-Flash-Next の重みを別々に量子化したもので、アーキテクチャ（`qwen4exp`）とチャットテンプレートは共通である。文脈長はいずれも 262,144 トークン、同時要求 1、画像入力なしである。補助の投機的復号は unsloth と iq3s が DFlash2 を使い、abliterated は使わない（「IQ3_S と abliterated IQ3_S の追加」）。abliterated は無検閲の派生モデルで、確認の範囲は挨拶にとどめる。

### NVIDIA ドライバの更新

CUDA 13 向けのエンジンは、NVIDIA ドライバ 580 以上を要求する（[インストールの条件](https://github.com/Niko1221/Strata/blob/v0.2.12/docs/INSTALL.md#1-check-the-computer)の「CUDA 13 packages require NVIDIA driver 580 or newer」）。OS の更新直後のドライバは Debian の 550 系で、この条件を満たさない。NVIDIA の CUDA 用リポジトリ（`debian13`）から 620.70.06 を導入した。この版は 2026-11-04 時点の Production Branch である。

導入の途中で二つの問題が起きた。どちらもパッケージの構成に原因があり、手順の誤りではない。

第一に、`nvidia-driver` を入れただけでは `nvidia-smi` が現れない。620 系の `nvidia-smi` は中身を持たない移行用のパッケージで、実行ファイルは `nvidia-driver-cuda` に入っている。`nvidia-driver` は `nvidia-driver-cuda` に依存しないため、次のコマンドで別に導入する。

```sh
sudo apt install nvidia-driver-cuda=620.70.06-1
```

第二に、カーネルモジュールが読み込まれず、`nouveau` が GPU を掴んだままになる。Debian の 550 系が残した alternatives のリンクが壊れており、`/etc/modprobe.d/nvidia-blacklists-nouveau.conf` が存在しないファイルを指していた。そのため nouveau の blacklist が効いていなかった。DKMS のビルドと署名は成功しており、モジュール自体は `/lib/modules/$(uname -r)/updates/dkms/` にあった。

blacklist と起動時の読み込みを明示し、壊れたリンクを持つパッケージを外してから、initramfs を作り直して再起動する。

```sh
printf 'blacklist nouveau\noptions nouveau modeset=0\n' \
  | sudo tee /etc/modprobe.d/blacklist-nouveau.conf
printf 'nvidia\nnvidia-modeset\nnvidia-uvm\n' \
  | sudo tee /etc/modules-load.d/nvidia.conf
sudo apt purge glx-alternative-nvidia glx-alternative-mesa \
  glx-diversions update-glx nvidia-settings
sudo update-initramfs -u
sudo reboot
```

再起動後、`nvidia-smi --query-gpu=driver_version,memory.total --format=csv,noheader` が `620.70.06, 12288 MiB` を返し、`lsmod` に `nvidia`、`nvidia_modeset`、`nvidia_uvm`、`nvidia_drm` が並ぶことを確認した。

### 保存先と接続先

| 場所 | 内容 |
|---|---|
| 推論サーバの `~/strata-models/models/unsloth-UD-IQ4_XS-v31/` | UD-IQ4_XS-v31 の分割ファイル四つ |
| 推論サーバの `~/strata-models/models/IQ3_S/` | IQ3_S（ISTA-DASLab）の分割ファイル二つ |
| 推論サーバの `~/strata-models/models/sc117-abliterated-IQ3_S/` | abliterated IQ3_S の分割ファイル二つ |
| 推論サーバの `~/strata-models/` 配下 | モデルの変換済みファイルと補助ファイル |
| 推論サーバの `~/.local/share/strata/` | Terminal とエンジンなど、Strata が管理するファイル |
| 推論サーバの `~/strata-install/` | インストーラ、起動・停止・状態表示の補助スクリプト |
| 推論サーバの `~/strata-logs/` | 起動ログ |
| 両機の `~/.config/strata/api-key` | API キー。ファイルの権限は `0600` |

サーバはモデルごとに別のポートで待ち受ける（8080・8081・8082、前節の表を参照）。手元の機械の 18080・18081・18082 からそれぞれ SSH で転送し、クライアントのベース URL を対応するポートへ向ける。`benitoiteh` は 8080 のモデルを使うため `http://127.0.0.1:18080/v1` とする。LAN 向けの待ち受けや外部公開は設定しない。

## 推論サーバの構築

以下の `llm-server` は SSH の別名の例である。利用者が設定した別名に置き換える。サーバではホーム内だけを変更する。ドライバの更新は前節のとおり、OS の更新に伴って別に行った。

### 前提の確認と Terminal の導入

手元から `ssh llm-server` で接続し、サーバ上で確認する。

```sh
nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader
nproc
free -g
df -h "$HOME"
```

この環境では `NVIDIA GeForce RTX 3060, 620.70.06, 12288 MiB`、12 スレッド、RAM 125 GiB、空き 1.6 TB であった。

Terminal は配布元のインストーラで導入する。`v0.2.12` の `install.sh` を取得し、リリースの `SHA256SUMS.txt` と照合してから実行する。

```sh
mkdir -p "$HOME/strata-install" && cd "$HOME/strata-install"
wget -q "https://github.com/Niko1221/Strata/releases/download/v0.2.12/install.sh"
wget -q "https://github.com/Niko1221/Strata/releases/download/v0.2.12/SHA256SUMS.txt"
grep ' install.sh$' SHA256SUMS.txt | sha256sum -c -
sh ./install.sh
```

取得した `install.sh` の SHA-256 は `d47754af1f74c95bf763e0b8d484232443466decd1a22d0be3208760a2f53822` であった。インストーラは配布物の署名を検証する。導入後の `strata --version` は `strata 0.2.12 (build 0.1.58; python 3.12.15 ...)` を返す。

### API キーと起動設定

API キーは端末へ表示せず、権限 `0600` のファイルに作る。値はランダムな 32 バイトを base64 で表した 44 文字である。

```sh
mkdir -p "$HOME/.config/strata" && chmod 700 "$HOME/.config/strata"
( umask 077; head -c 32 /dev/urandom | base64 > "$HOME/.config/strata/api-key" )
```

起動の設定は `~/strata-install/start.sh` にまとめる。鍵の値を引数に書かず、環境変数で渡す。

```sh
#!/bin/sh
set -eu
export PATH="$HOME/.local/bin:$PATH"
export TMPDIR="$HOME/strata-install/tmp"
export STRATA_UPDATE_FEED="https://github.com/Niko1221/Strata/releases/download/v0.2.12"
export STRATA_API_KEY="$(cat "$HOME/.config/strata/api-key")"
exec "$HOME/.local/bin/strata" launch \
  --family unsloth --model UD-IQ4_XS-v31 \
  --data-dir "$HOME/strata-models" \
  --context 256k --parallel 1 --vision off --tune skip \
  --access local --port 8080 --api-key-env STRATA_API_KEY \
  --save-preset qwen-unsloth-iq4xs-256k --feed "$STRATA_UPDATE_FEED" \
  --yes --no-tui "$@"
```

`--family unsloth` と `--model UD-IQ4_XS-v31` の組み合わせは、0.2.11 以降の Terminal が受け付ける。導入先の `--data-dir` は、既定の場所ではなくホーム直下の `~/strata-models` にした。

最初に `--plan` を付け、指定の配布者の UD-IQ4_XS-v31、保存先、取得量、対応エンジンを確認する。

```sh
"$HOME/strata-install/start.sh" --plan
```

この環境では、モデル 94 GB、文脈長 256K、画像入力なし、同時要求 1、取得量 約 94.9 GB、DFlash2、チューニングなし、`127.0.0.1:8080`、API キー有効と報告され、何も変更しなかった。

### 取得と再開

確認後にバックグラウンドで起動する。同じ取得処理が動いていないことを確かめてから実行する。

```sh
mkdir -p "$HOME/strata-logs"
nohup "$HOME/strata-install/start.sh" \
  >"$HOME/strata-logs/launch.log" 2>&1 </dev/null &
```

エンジンは最初に取得される。この環境では `linux-x86_64-cuda-cuda13` の 723 MB を取得し、署名と全ファイルを検査したうえで、テスト起動を通してから切り替えた。続いてモデルの取得に移る。

この環境では 21:43 に開始し、エンジンとモデル 93.7 GB、DFlash2 の補助ファイルの取得を 00:02 までに終えた。所要は約 2 時間 19 分、平均で約 11 MiB/s であった。回線の上限は生の `wget` でも 5〜11 MiB/s 程度で、Strata の設定で速くできる余地はなかった。

取得は分割ファイルごとに進み、途中のファイルは `.part` と `.part.chunks` に残る。中断しても同じ起動設定で再開でき、取得済みのチャンクを再利用する。部分ファイルを消さない。経過時間だけでは失敗と判断せず、取得済みの量の増加とログで確かめる。

状態は補助スクリプトで表示する。値の取得と表示だけを行い、何も変更しない。

```sh
"$HOME/strata-install/status.sh"
```

停止には `strata launch --stop` を使う。取得中の処理があるときは、`~/strata-install/stop.sh` が止める前に知らせる。OS 再起動後は手動で起動する。

### IQ3_S と abliterated IQ3_S の追加

ISTA-DASLab の IQ3_S は Strata のカタログに載っている。取得と準備だけを行う `strata install`（内部では `setup.py --no-start`）を使えば、稼働中のモデルを止めずに追加できる。`setup.py` を直接叩くと、端末の Python に `pip` がなく、CUDA の鍵の導入で `sudo` を求めて止まる。Terminal の `strata install` から呼ぶ。

```sh
strata install --family qwen --model IQ3_S --data-dir "$HOME/strata-models" \
  --context 262144 --parallel 1 --vision no --drafter dflash2 \
  --feed "$STRATA_UPDATE_FEED" --yes --no-start
```

ファイルは `~/strata-models/models/IQ3_S/` に、配布元の名前のまま二つ置かれる。54.8 GB の本体と、28.8 GB の PLE 表（per-layer embedding）である。

SC117 の abliterated はカタログにないため、配布元から二つの分割ファイルを取得し、`--model-path` でフォルダを渡す。リビジョンは `90a3350c…` に固定する。

| ファイル | 大きさ | SHA-256 |
|---|---:|---|
| `Qwen3.8-Flash-Next-GSQ-RCO-abliterated-IQ3_S-00001-of-00002.gguf` | 55,063,448,064 | `40f369eca301ec6ea318dbb671b3132c26a2f0b89aa5458a7051b33215ee296c` |
| `Qwen3.8-Flash-Next-GSQ-RCO-abliterated-IQ3_S-00002-of-00002.gguf` | 28,800,138,432 | `316b46f3a2dbd68c900f43136ab9449f9dcc3725dfd8c794847c204bc161e113` |

第 2 のファイルの SHA-256 は ISTA-DASLab 版の PLE 表と同一である。同じ内容なので、ISTA 側の取得物からハードリンクして 28.8 GB の再取得を省いた。ハッシュが一致しなければ通常の取得に切り替える。

この配布元は Strata を明示的に対象にしており、`strata/rt`（786 MB、MTP の実行時レイアウト）を同梱している。同梱の README は、公式 GSQ-RCO のパックを流用せず、このリポジトリの GGUF から別のパックを作るよう指示している。移植でテンソルの位置が変わり、`native_experts.txt` が一致しなくなるためである。パックはモデル名で決まる。`--family qwen --model iq3_s` を明示すると、ISTA-DASLab 版のために作った `packs/iq3_s` を再利用しようとし、次の行で起動に失敗する。

```text
strata generate: ArenaExpertSource: the pack's native_experts.txt does not match the model:
blk.0.ffn_gate_exps.weight in .../Qwen3.8-Flash-Next-GSQ-RCO-abliterated-IQ3_S-00001-of-00002.gguf
```

`--data-dir` を別にしても解決しない。起動の記録（`~/.local/share/strata/install/strata-iq3_s.json`）が持つパックの場所が使われるためである。実際に別のフォルダを指定して試すと「準備済みのパックは `~/strata-models` から読む。何も写していない」と告げられ、同じ行で失敗した。

そこで、Strata が自動で判定した族（`swift`）のまま起動し、`packs/swift-iq3_s` を別に作らせた。判定の根拠は本体のファイルの大きさで、abliterated 版の 55,063,448,064 バイトは Swift 1.5 の IQ3_S と一致する。アーキテクチャとチャットテンプレートは Qwen と共通なので、生成の質には影響しないと見ているが、族の名前が変わること自体は記録しておく。

補助の投機的復号は使っていない（`--mtp off`）。DFlash2 は Qwen3.8-Flash-Next 専用と判定され、同梱の MTP（`strata/rt`）は Swift 用ではないためである。速度は落ちるが、挨拶程度の確認には足りる。

取得は `wget` では進まなかった。1 本の接続が絞られ、0.5 MB/s まで落ちて 1 時間で 1 GB しか増えなかった。範囲要求で 16 本に分ける小さな道具（`~/strata-install/hfget.py`）に切り替えると 10〜11 MiB/s になり、残り 48 GB を 1 時間半で取り終えた。

| 対象 | 取得量 | 終了時刻 |
|---|---:|---|
| IQ3_S（ISTA-DASLab）本体 | 54.8 GB | 15:27 |
| 同 PLE 表 | 28.8 GB | 16:15 |
| abliterated 本体 | 55.1 GB | 17:23 |

同日 14:01 の開始から、合計 138.7 GB を 3 時間 22 分で取得した。PLE 表をハードリンクで共有したため、二重に取った量はこの値に含まれない。ディスクの使用量は 231 GB になった（うちモデル 218 GB、パック 4.4 GB）。

## BenitoiteHarness の接続

### SSH のポート転送と認証

手元の機械で転送を開始し、利用中は維持する。

```sh
ssh -N \
  -o ExitOnForwardFailure=yes \
  -o ServerAliveInterval=30 -o ServerAliveCountMax=3 \
  -L 127.0.0.1:18080:127.0.0.1:8080 \
  -L 127.0.0.1:18081:127.0.0.1:8081 \
  -L 127.0.0.1:18082:127.0.0.1:8082 \
  llm-server
```

3 つのモデルを切り替えて使うため、転送は 3 本まとめて張る。起動していないモデルのポートは接続が拒否されるだけで、ほかのモデルには転送されない。

API キーは SSH で手元へ複製し、権限 `0600` で `~/.config/strata/api-key` に保存する。値を端末へ表示したり、設定ファイルへ直接書いたりしない。既存のキーは上書きしない。

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

`curl` がある場合の接続確認は次のとおりである。この環境のサーバには `curl` がないため、確認は Python の `urllib` で行った。

```sh
curl -fsS -H "Authorization: Bearer $STRATA_API_KEY" \
  http://127.0.0.1:18080/v1/models
```

モデルが読み込まれ、`qwen3.8-flash-next-unsloth-ud-iq4_xs-v31` が返ることを確認する。この名前は、`--family` に指定した名前 `unsloth` と、量子化版の名前 `UD-IQ4_XS-v31` を小文字にして繋いだものである。

### 提供者の設定と最初の依頼

既存の利用者設定をバックアップし、`~/.config/benitoiteh/config.toml` に次を追加する。`XDG_CONFIG_HOME` を指定している場合は、その下の `benitoiteh/config.toml` を使う。既定の提供者は変えない。

```toml
[providers.strata]
kind = "openai-chat"
base_url = "http://127.0.0.1:18080/v1"
model = "qwen3.8-flash-next-unsloth-ud-iq4_xs-v31"
context_window = 262144
api_key = { env = "STRATA_API_KEY" }
effort = "low"
capabilities = { reasoning_effort = true }
```

これは既存の `openai-chat` アダプタで `POST /v1/chat/completions` を使う設定であり、専用アダプタの追加は要らない。設定全体と Benitoite の処理系の指定は [CLI と設定ファイル](../design/05-interface/05-01-cli.md#設定ファイル)を参照する。

`capabilities.reasoning_effort = true` は、設定や `--effort` の値を API の `reasoning_effort` に付ける指定である。これがない場合、`openai-chat` は effort を要求に付けない。サーバ全体の既定値は変更せず、BenitoiteHarness の要求だけを low にする。

最初の依頼は空の確認用ディレクトリで行う。以下の実行ファイルのパスを、用意した `benitoiteh` に置き換える。

```sh
BENITOITEH_BIN=/path/to/benitoiteh
mkdir -p "$HOME/benitoiteh-local-llm-check"
"$BENITOITEH_BIN" exec \
  --mode restricted --provider strata --max-turns 1 --max-run-tokens 32768 \
  --project "$HOME/benitoiteh-local-llm-check" \
  '道具を使わず、日本語で短く挨拶してください。'
```

## 取得後の確認と再開

### 読み込みと API の応答

取得が終わると、同じ起動がそのまま読み込みに移る。この環境では起動から読み込みの完了まで 15 秒であった。`/v1/models` は `qwen3.8-flash-next-unsloth-ud-iq4_xs-v31` を返し、`meta.n_ctx` は 262,144 であった。

API キーを付けない要求は 401 で拒否された。鍵は `Authorization: Bearer` で渡し、値はファイルから読む。

### 補助の確認コマンド

この環境には `~/strata-install/verify.py` を置いた。読み込み後の推論 API に要求を送り、結果を `~/strata-logs/verification.json` と `~/strata-logs/long-input.json` に書く。内容は、キーなしの要求の拒否、挨拶のストリーミング、道具の呼び出し、道具の結果を返した後の応答、Benitoite のソース生成である。`--long` を付けると、先頭に目印を置いた長い入力を送る。要求を送るだけで、サーバの設定は変えない。

この節に挙げた `status.sh`、`stop.sh`、`verify.py` は、この環境のために置いた補助であり、Strata の標準コマンドでも、このリポジトリに同梱するスクリプトでもない。別の環境を作るときは、上の手順に加えて同じ確認を行う補助を別に用意する。

```sh
python3 ~/strata-install/verify.py
python3 ~/strata-install/verify.py --long --chars 440000
```

ストリーミングの確認は、`/v1/chat/completions` に `stream: true` を付け、`data:` 行を読んで本文を組み立て、最初の本文が届くまでの時間を測った。

### 素の API での確認結果

モデルを直接呼び、サーバとモデルが期待どおり働くかを確かめた。`reasoning_effort` は `low` を明示した。指定しない場合の既定は `xhigh` である。出力の上限を付けずにこの既定のまま送ったところ、1 件の要求が 10 分を超えて終わらなかったため、打ち切って `low` と上限を付けた条件でやり直した。以後の値はその条件での結果である。

| 確認 | 結果 |
|---|---|
| キーなしの要求 | 401 で拒否 |
| 挨拶（ストリーミング） | 本文は「こんにちは！お元気ですか？」。最初の本文まで 3.13 秒、要求全体で 3.30 秒。入力 51、出力 50 トークン |
| 道具の呼び出し | `get_time` を `{"city":"東京"}` の引数で 1 回呼び、`finish_reason` は `tool_calls` |
| 道具の結果を返した後の応答 | 返した都市と時刻をそのまま述べた応答。入力 422、出力 98 トークン。入力のうち 369 はキャッシュから再利用 |
| Benitoite のソース生成（素の API） | 構文を知らないと答えてソースを書かなかった |

最後の項目はサーバの不具合ではない。Benitoite の知識は `benitoiteh` がシステムプロンプトに含める Skill から入る。素の API にはこの Skill がないので、ソース生成は `benitoiteh` 経由で確かめる。

### `benitoiteh` からの接続確認

利用者の普段の設定と記録を混ぜないよう、確認専用の `XDG_CONFIG_HOME`・`XDG_STATE_HOME`・`TMPDIR` と空のプロジェクトのディレクトリを用意した。`[skills]` と `[plugins]` の `user`・`project` を偽にして、利用者とプロジェクトの Skill とプラグインを読まない設定とした。処理系が返す Benitoite の Skill は通常どおりシステムプロンプトに含めた。

処理系は Benitoite 0.0.12 を用いた。制限モードで OS のサンドボックスを掛け、スクリプト実行のエフェクトは許可していない。

| 確認 | 結果 | 起動から終了までの時間 |
|---|---|---:|
| 日本語の挨拶 | 挨拶と作業の申し出が返り、終了状態 0 | 約 16.7 秒 |
| 加算する関数と、結果を表示する `main` の生成 | `write` の道具で `main.bnt` を書いた後に 1 巡の上限で停止 | 約 6.4 秒 |

生成されたソースは、`Integer` を二つ受け取る `add` と、`Console.writeLine` で結果を表示する `main` からなる。`main` は `uses Console.Write` を宣言している。処理系の `benitoite check --report=json` は `status: ok`、`complete: true`、診断なし、`executed: false`、宣言したエフェクトの上限は `Console.Write` だけと報告した。

この確認で、`benitoiteh` の要求送信、ストリーミングの受信、道具の呼び出しと結果の返却、生成したソースの静的検査を確かめた。生成コードの実行、ファイルの編集を伴う課題、課題の成功率の評価は行っていない。

### 256K に近い入力

文脈長 262,144 トークンのうち、243,824 トークンの入力を送り、先頭に置いた目印を返せるかを確かめた。目印 `MARKER-7F3A` は正しく返った。

| 項目 | 値 |
|---|---:|
| 入力トークン | 243,824（うち 129,024 はキャッシュから再利用） |
| 新たに読んだ入力 | 114,800 トークンを 145.3 秒（790.1 トークン/秒） |
| 出力 | 137 トークンを 3.60 秒（38.0 トークン/秒） |
| 要求全体 | 約 149 秒 |
| 処理後の VRAM | 11,341 / 12,288 MiB |
| 処理後の RAM | 使用中約 70 GiB、利用可能約 55 GiB |

読み取りの速度はエンジンが残す `~/.local/state/strata/launch/qwen-unsloth-iq4xs-256k.engine.log` の `prompt ... tokens` の行から取った。この行は、読み取った量と要した時間、生成した量とその速度、投機的復号で採用された草案の数を報告する。

### 停止と再開

停止は `strata launch --stop --preset "qwen-unsloth-iq4xs-256k"` で行う。取得の途中で止めた場合も、同じ起動設定で再開すれば、取得済みのチャンクを再利用する。OS を再起動した後は手動で起動する。

## 3 つのモデルの切り替え

### 同時に載せない理由

3 つのモデルを同時に常駐させれば切り替えは一瞬で済むが、RAM が足りない。実測は UD-IQ4_XS-v31 が使用中 70 GiB、IQ3_S が 256K で約 57 GiB である。合計は 125 GiB を超える。文脈長を 32K まで落としても 2 つで 116 GiB 前後になり、OS とページキャッシュの余地が残らない。したがって常駐は 1 つとし、切り替えは「サーバを切り替える」「DSH でモデルを選ぶ」の 2 段にする。

### モデルごとにポートを分ける理由

Strata のサーバは、要求の `model` に入れた名前を見ない。UD-IQ4_XS-v31 を配信中のサーバに `qwen3.8-flash-next-iq3_s` を指定して送ると、200 で UD-IQ4_XS-v31 が答えた。したがって 1 つのポートに 3 つの経路を向けると、選んだつもりのモデルと違うモデルが黙って答える。ポートを分ければ、起動していないモデルを選んだときは接続拒否になり、取り違えが起きない。

| モデル | サーバのポート | 手元の転送先 |
|---|---:|---:|
| unsloth | 8080 | 18080 |
| iq3s | 8081 | 18081 |
| abliterated | 8082 | 18082 |

### サーバ側の切り替え

`~/strata-install/switch.sh` が、稼働中のモデルを止め、指定のプリセットを起動して、応答が返るまで待つ。RAM の空きが 60 GiB 未満なら起動しない。

```sh
~/strata-install/switch.sh unsloth
~/strata-install/switch.sh iq3s
~/strata-install/switch.sh abliterated
```

手元からは `strata-local` の `switch` で同じことができる。SSH 転送は 3 ポートをまとめて張る。

```sh
~/.local/bin/strata-local switch iq3s
~/.local/bin/strata-local status
```

`strata-local` は手元に置いた補助のコマンドである。転送が張られていなければ自分で張ってから、`switch` ではサーバの `switch.sh` を、`status` では 3 つのポートの `/v1/models` を呼ぶ。`status` は起動しているモデルの id と `n_ctx` を表示し、応答がないポートは「起動していない」と表示する。

`switch.sh` は、止めてから起動し、`/health` が読み込み済みを返すまで最大 20 分待つ。切り替えにかかる時間は、モデルの読み込みを含めて 1〜2 分である。

同じ手順を Agent Skill にまとめ、手元の `~/.agents/skills/strata-models/` に置いた。エージェントは `scripts/strata-server.sh` を通して `status`・`switch`・`tunnel` を呼ぶ。このスクリプトは SSH の制御ソケットを作らずに 1 回ずつ `ssh` するので、ワークスペースの外に書き込めないサンドボックスの中でも動く。`strata-local` は制御ソケットを `~/.config/strata/` に作るため、サンドボックスの中では弾かれる。

制限モードのエージェントはシェルを持たないため、この経路は通常モードに限られる。

### 3 つのモデルの挨拶

2026-11-08 に、3 つのモデルを順に起動し、同じ依頼（「道具を使わず、日本語で短く挨拶してください。」）を送った。`reasoning_effort` は `low`、出力の上限は 300 トークンである。

| モデル | 応答 | 所要 | 入力 / 出力 |
|---|---|---:|---:|
| unsloth（8080） | こんにちは！元気ですか？ | 3.86 秒 | 51 / 48 |
| iq3s（8081） | こんにちは！お元気ですか？ | 2.32 秒 | 51 / 51 |
| abliterated（8082） | こんにちは。今日もよろしくお願いします。 | 1.63 秒 | 51 / 58 |

読み込み後の VRAM は 11,327 / 12,288 MiB、RAM は使用中 56 GiB・利用可能 69 GiB であった。3 つの所要時間は、モデルの読み込み直後の値であり、比較のための測定ではない。

切り替えは、手元の `strata-local switch` から 3 つとも通った。稼働していないモデルの経路は接続拒否になり、ほかのモデルが代わりに答えることはなかった。18:01 には、エージェントのサンドボックスの中から Skill の `strata-server.sh switch iq3s` を実行し、8081 で応答することを確かめた。

### DSH の設定

`~/.dsh/profiles/web/cordis.patch.yml` の `llm-pi-ai` に 3 つの経路を置く。鍵は `~/.dsh/.credentials.yaml` の `STRATA_API_KEY` を共用する。

```yaml
    providers:
      strata-unsloth:
        displayName: Strata (Unsloth UD-IQ4_XS-v31)
        apiKeyEnv: STRATA_API_KEY
        api: openai-completions
        baseURL: http://127.0.0.1:18080/v1
        compat:
          supportsDeveloperRole: false
          maxTokensField: max_tokens
          supportsReasoningEffort: true
          supportsUsageInStreaming: false
        timeoutMs: 600000
        models:
          - id: qwen3.8-flash-next-unsloth-ud-iq4_xs-v31
            name: Qwen3.8-Flash-Next UD-IQ4_XS-v31
            contextWindow: 262144
            maxTokens: 8192
            input: [text]
            reasoningEfforts: { low: low, medium: medium, high: xhigh, xhigh: xhigh }
```

`strata-iq3s`（18081）と `strata-abliterated`（18082）も同じ形で、`displayName`、`baseURL`、モデル id だけが違う。モデル id は、起動したサーバの `/v1/models` が返す値をそのまま使う。

経路を足しただけでは、稼働中の DSH のセッションには現れない。プロファイルの設定を読むのは起動時なので、`dsh web` を起動し直す。再起動後、新しい会話でモデルを選ぶ。選択した経路のサーバが起動していなければ接続が拒否され、別のモデルが答えることはない。

既定のモデル（`agent-default-model`）は Ollama Cloud のままにし、`web`・`locale`・`sandbox-policy` の各層は変えない。`agent-default-model` は、DSH でモデルを切り替えたときに DSH 自身が書き換える。編集するときは、ほかの層を巻き込まないよう行単位で置き換える。

2026-11-08 に、DSH の 3 つの経路を順に選び、いずれも挨拶の応答を返すことを確かめた。

## 付随作業: ほかのハーネスからの接続

OpenCode などのほかのハーネスからの接続は、今回は確かめていない。
