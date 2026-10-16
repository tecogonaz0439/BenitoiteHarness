# 提供者の層と処理系への要望

対象の設計は [02-01 提供者の層](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/02-providers/02-01-provider-layer.md)、[01-04 文脈](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/01-agent/01-04-context.md)、[04-01 処理系との接続](https://github.com/tecogonaz0439/BenitoiteHarness/blob/09dde24f92ec4768f9288e1b0448ced0f921ec08/docs/design/04-benitoite/04-01-benitoite-integration.md) です。

公式文書の確認日は **2026-10-16** です。以下の「確認済み」は文書上の確認であり、APIキーを使った送信、課金、実機での相互接続試験を行った意味ではありません。提供者ごとのモデル・アカウント・互換サーバの差は残ります。

## 1. BH-030: OpenAI Responses API

### 1.1 要検証欄への回答

設計の基本形は妥当です。公式SDKの型とガイドから、次のように整理できます。[S-O01] [S-O02] [S-O06]

| 設計の項目 | 文書で確認できること | 設計へ反映すること |
|---|---|---|
| `store: false` | 保存を無効にする要求の形として有効 | 明示指定を維持する。ただし、これだけであらゆるログ保持がゼロになるとは表現しない |
| `stream: true` | ストリーム応答に対応 | HTTPの完了と応答の意味上の完了を別に扱う |
| `include: ["reasoning.encrypted_content"]` | 暗号化された推論データの取得指定として有効 | 現行の移行ガイドで既定の返却も説明されているため、「必ず付けなければ返らない」という普遍的説明にはしない。明示指定自体を誤りとする必要はない |
| `instructions` | 上位の指示を送る欄として有効 | 毎要求で作る有効な指示を固定し、ローカルに履歴を持つ方針を維持する |
| 完全な履歴の再送 | ステートレスな利用の構成として可能 | 元の `response.output` を損失なく保存する。本文だけの再構成では不足する |
| `prompt_cache_key` | サポートされた入力項目 | セッション単位は一つの方針。全モデルで必須、または常に最適とはしない |
| 出力上限 | `max_output_tokens` を使い、推論も予算に関係する | 可視本文の上限だけと考えず、文脈の予約量に含める |
| 文脈の超過 | 自動の切り捨てに頼らない構成が可能 | クライアント側の圧縮と、不完全応答の扱いを先に定める |

BH-030の欄は「基本的な要求形式は文書で確認済み」に変更できます。一方、**正しい再送、エラー復旧、対象モデルの実際のキャッシュ挙動まで確認済みにはできません**。これらを別の受け入れ項目として残すのが適切です。

### 1.2 共通化で落としてはいけないもの

Responsesのfunction callの項目には、その項目自身の識別子と、道具の結果を結び付ける `call_id` があります。両者を共通の一個のIDへ潰さないでください。`function_call_output` は対応する `call_id` を使います。また、推論項目を含む元の出力列の順序を保つ必要があります。[S-O03]

保存用の形は、例えば次のように二層にすると実装しやすくなります。これは本レビューの内部形式案であり、提供者のAPIそのものではありません。

```text
ProviderTurn
  origin: {adapter, endpoint_identity, credential_scope, requested_model,
           returned_model, protocol_profile}
  native_items: [順序を維持した提供者固有の項目]
  projections:
    visible_text
    normalized_tool_calls
    usage
  id_map:
    local_id -> {wire_item_id, wire_call_id}
  completion:
    complete | incomplete(reason) | failed(error) | cancelled
```

LLMへの再送には `native_items`、UI・道具の実行には検証済みの `projections` を使います。IDの正規化が必要でも、元の値を消さないことが重要です。提供者を切り替えたときは元のopaque項目を別のAPIへそのまま渡さず、新しい会話へ移るか、意味を保てる移行だけに限定します。これは指摘13の実装案です。

### 1.3 ストリームの完了を扱う

Responsesは型付きのイベントを返します。部分的なargumentsを受け取ったことと、呼び出しが完成して実行してよいことは同じではありません。失敗・不完全な終了を正常完了へ潰さない処理が必要です。[S-O04]

設計する状態遷移は、次の程度まで明示してください。

```text
Receiving
  -> Completed     : 最終応答が完成し、各tool引数も検証済み
  -> Incomplete    : 出力上限等。未完成のtoolは実行しない
  -> Failed        : APIの意味上のエラー
  -> Interrupted   : EOF、通信断、利用者の取消
```

既に読み取り専用の道具を先行実行していても、`Interrupted` を通常のassistant応答として履歴へ確定しないことが必要です。先行結果の再利用には、指摘20の読み取り世代の検証を掛けます。

## 2. Chat Completionsと互換API

### 2.1 共通の外形は、同じ機能の保証ではない

Chat Completions互換というだけで、strict schema、推論文、使用量、`store`、出力上限の引数がすべて同じとは限りません。BH-032では、**API形式の種類** と **提供者・モデルの能力** を別に持つべきです。

例えば次の宣言を、接続プロファイルの案とします。

```text
Capabilities
  tool_schema_mode: strict | advisory
  optional_fields: omitted | nullable_required
  reasoning_replay: opaque_exact | text_required_with_tools | not_supported
  thinking_prefix_binding: none | prefix_and_account
  stream_usage: supported | absent
  output_limit_parameter: 接続先ごとの定義
  store_false: supported | unsupported | unverified
  cache_control: explicit | automatic | unsupported
  tool_choice_modes: 接続先で検証した集合
  cancellation: best_effort_transport_abort
```

`unverified` は成功扱いではありません。安全に縮退できるものと、正しい継続に必要なので接続を拒否すべきものを分けます。どの互換サーバにも全項目を試しに送る方式は避け、明示的なプロファイルから要求を作ります。

### 2.2 strictと任意引数

OpenAIのstrictなfunction schemaでは、各objectで `additionalProperties: false`、プロパティの必須指定などの制約があります。任意の値を許す場合は、API向けにはnullを含める形が必要になる場合があります。ResponsesとChat Completionsではfunctionの包み方も異なります。[S-O03]

道具の意味上の型を一つ用意し、そこから接続先別のschemaを生成するのがよいでしょう。例えば、意味上の `read(path, offset?, limit?)` について、受信した `null` を「既定値を使う」に正規化する段階を設けます。ユーザー定義の未知フィールドを黙って実行層へ渡さないようにします。

最終的な検証は、strictの有無にかかわらずハーネスで行います。schemaの保証を、path・権限・回数・サイズの保証の代わりにはしません。

### 2.3 保存方針

OpenAIの現行移行文書では、Chat Completions側の保存の既定にもアカウント条件の説明があります。Responsesだけ `store: false` にし、OpenAIのChat側を既定に任せると、同じハーネス内で利用者の意図が揃わない可能性があります。[S-O01]

**OpenAIの対応エンドポイントでは明示的な保存無効を検討する**、他の互換先では対応を確認してから送る、という規則を追加してください。未対応先へ一律に `store` を送るという意味ではありません。永続化のプライバシーと、プロンプトのキャッシュの課金・再利用も分けて説明するのがよいでしょう。

## 3. Anthropic Messages API

### 3.1 設計の方向として維持してよいもの

strict tool useや `eager_input_streaming` を使う方向を、古いAPIの知識から誤りとする理由はありません。公式文書に機能が記載されています。一方、細粒度のtool input streamingでは、途中のJSONが不完全・未検証であることを前提とする必要があります。[S-A03] [S-A04]

`content_block` のindex、テキスト・thinking・toolの順序、署名のdeltaを失わずに組み立ててください。イベントの受信順を単に「テキスト欄」「思考欄」「道具一覧」に分類して保存するだけでは、正しい応答の再構成に不足します。[S-A02]

このレビューでは、strictとeagerの組合せが全対象モデルで実際に動くことまでは試していません。逆に、両者が必ず排他的だと主張する根拠もありません。対象モデルと要求形を固定したfixture・明示的な接続試験で決着させてください。

### 3.2 再開と署名付きthinking

現行のpreserved thinking文書は、対象モデル・対象アカウントについて、thinkingが前のprefixやアカウントに結び付く制約を説明しています。すべての過去モデルへ同じ条件を一般化するのではなく、接続プロファイルへ適用条件を持たせる必要があります。[S-A01]

設計の「同じモデルなら元の推論データを再送する」だけでは、次の変更や再開実装の回帰を覆えません。現設計が日付・作業ディレクトリをsystemに入れず、変化を会話へ追記する方針である点は正しく、維持すべきです。

| 再開時の変更 | 必要な扱いの案 |
|---|---|
| ハーネス更新でsystemを新しい本文へ再生成 | 元のsystemを再現するか、新しい会話として扱う |
| 再開時に、保存した古い環境情報を現在の値へ置換 | 現設計どおり追記にする。`create_project` 後の変化も、元のprefixを書き換えない |
| 道具schemaや埋め込みSkillを更新 | 同じ提供者名でも互換とは見なさない |
| APIキーを別アカウントへ交換 | 秘密そのものではない資格情報のスコープIDを比較し、移行を明示する |
| 本文だけを要約して古いthinkingを残す | クライアント側の新しい文脈として再構成する |

アカウント境界を記録するためにAPIキーの原文を履歴へ保存してはいけません。ローカルの資格情報IDなど、再送可能性を安全に判断するための識別子に限ります。

キャッシュ境界の移動と、thinkingが依存する内容の変更は区別してください。`cache_control` を付ける位置を動かすだけで、必ず署名が無効になる、という指摘はしません。[S-A01]

### 3.3 キャッシュと使用量

Anthropicでは、キャッシュ作成・読み取り・通常入力の使用量を区別して集計する必要があります。キャッシュの区切りには個数と過去block探索の範囲があり、三点を使う現在案は適切な出発点ですが、長い道具列で最適とは限りません。[S-A05]

OpenAI側のcached inputは入力の内数として扱うため、Anthropicの三つの欄の足し算を全APIに適用しないでください。共通の `Usage` には、少なくとも「入力全体」「内訳」「提供者の生の使用量」「未報告」を持たせます。キャッシュの料金や返却欄はモデル・時期で変わるため、固定の割合を仕様へ埋め込むより、価格表の版を記録する方式を勧めます。[S-O05]

提供者のサーバ側compactionも文書化されていますが、これを今すぐ試作へ導入しなければならないとは考えません。クライアントの圧縮と意味・履歴形式が異なるため、後の独立した比較対象にするのがよいでしょう。[S-A06]

## 4. BH-031: DeepSeekの推論文

### 4.1 文書で確定できたこと

**推論モードで要求に `tools` を含める場合、過去のターンの `reasoning_content` を再送する必要があります。** 現行公式ガイドの条件は「直前のターンが道具を呼んだときだけ」ではありません。必要な推論文の欠落は400エラーの原因になります。`tools` を含めない場合の扱いとは分けて書いてください。[S-D01]

したがって、BH-031のこの部分は、文書で確認した結論を本文へ移せます。ただし、モデル更新・互換API・新規会話への移行の実際の挙動は、別の適合試験として残します。

### 4.2 圧縮で採るべき方針

01-04の「圧縮後はすべての推論文を落とす」を共通規則にしたまま、古いassistant/toolのtailを残すと、この要件と衝突します。解決案は二つです。

| 案 | 動作 | 長所 | 注意点 |
|---|---|---|---|
| 完結したターンをそのまま保持 | 残すassistantに付く推論文も維持 | 道具との対応を保ちやすい | 推論文を含む分だけ圧縮効果が小さくなる |
| 新しい会話として始め直す | 要約を新しいuser側の文脈として置き、古いassistant/tool列を持ち込まない | 推論を安全に落としやすい | 道具の証拠や未完了事項を要約の明示フィールドへ残す必要がある |

試作では後者をDeepSeek用の明確な選択肢にするのが実装しやすいと考えます。「古いassistantは残すがthinkingだけ消す」を既定の中間案にしないでください。

ここでいう要約は、提供者の推論文をユーザーへ公開するものではありません。作業の目的、実際に変更したファイル、確認済みの結果、未解決事項をハーネスの新しい文脈として渡すものです。

### 4.3 Anthropic互換エンドポイント

DeepSeekのAnthropic互換文書は、Anthropicのすべての機能が同じ意味で実装されるとはしていません。例えば、キャッシュ制御やthinking budgetに無視される項目があり、redacted thinkingには未対応の説明があります。[S-D02]

従って、`adapter=anthropic` と `provider=deepseek` を一つに潰さないでください。`strict` や `eager_input_streaming` の未確認の対応まで、本家Anthropicで動くからといって有効化しないことが必要です。深い互換性はまだ未検証として残すのが正確です。

### 4.4 適合試験の最小組

下表は提案する試験であり、今回は実行していません。最初は保存した公式例・模擬SSEで実施し、実APIの試験は利用者が承認した環境で行います。

| 試験 | 期待する結果 |
|---|---|
| 道具なしで二ターン継続 | 接続先の推論再送規則に従い、不要なフィールドを混ぜない |
| `tools` あり、応答にはtool callなし、次のターンへ | 必要な過去の推論文を保持する |
| 複数tool callを一応答で受信 | 順序とIDの対応を維持する |
| toolの引数JSONが途中で切れる | 未完成の道具を実行しない |
| 途中のsignature deltaの後に切断 | 署名済みの完成応答として保存しない |
| keep-tail圧縮後に継続 | 提供者別の保持単位を満たす |
| 要約のみの新規会話へ移行 | 古いtoolの孤立結果を送らない |
| 同じモデル名で接続先・資格情報を変更 | opaque項目の無条件再利用をしない |
| 使用量の最後のイベントが欠落 | ゼロ課金と決めつけず、未報告を記録する |

## 5. BH-022〜BH-026の優先順位

5件とも要望として妥当です。ただし、安全性と実装の簡単さへの寄与は同じではありません。

| 優先 | 要望 | 評価と範囲 |
|---|---|---|
| 最優先 | **BH-022 検査・エフェクトの構造化** | 字句解析への依存を減らす。成功だけでなく、対象・依存・完全性を報告する |
| 最優先 | **BH-023 実行しないテスト検査** | 実際のテスト発見規則と承認対象を揃えるために必要 |
| 高 | **BH-025 機械可読なversion/commit/能力** | 対応表を安全に選ぶ基盤。小さな変更として先行実装してよい |
| 高 | **BH-026 子プロセスのローダ環境制御** | 多層防御として有用。OSのexec制限の代替にしない |
| 中 | **BH-024 Skillの取得** | 配布・埋め込み・更新を簡単にする。実行の権限判断の必須要件ではない |

実装の順序としてBH-025を先に入れ、その共通のproducer情報をBH-022/023で使うのは合理的です。表は安全性への重要度の順であり、実装工数の順ではありません。

**5件に不足している独立した第6の大機能を増やすより、BH-022/023の共通契約として「対象の同一性・依存ソースの台帳・完全性・形式の版」を追加することを勧めます。** これはサーバモードや実行時エフェクト監視を前倒しする要求ではありません。

## 6. BH-022/023の出力形式案

### 6.1 設計原則

現在のv0.0.5のCLIは、人間向け表示と既存の診断出力を持っています。以下のCLI名・JSONは、**v0.0.7に向けた新しい提案であり、v0.0.5に存在するコマンドではありません**。[S-B01] [S-B05]

候補となる形は、例えば次のようなものです。

```text
benitoite check --report=json <entry>
benitoite test --check --report=json <path>...
benitoite version --format=json
```

名前自体にはこだわりません。重要なのは、正常系で人間向けの文や進捗を同じstdoutへ混ぜず、stderrの診断の見た目から成功を推測させないことです。機械向けの結果を出すモードを明示します。

出力を一個のJSONにする案を第一候補とします。大量のテストでストリームが必要なら、JSONLへ変えてもよいですが、その場合は開始・対象結果・最終集約を持ち、**最終集約を受信しなければ完全な結果と認めない**という契約が必要です。

### 6.2 単一ファイルの成功例

次は構造を示す架空の例です。ハッシュはダミーであり、実ファイルから計算した値ではありません。

```json
{
  "schema_version": 1,
  "kind": "check_report",
  "producer": {
    "name": "benitoite",
    "version": "0.0.7",
    "commit": null,
    "capabilities": ["check-report-v1", "test-plan-v1"]
  },
  "status": "ok",
  "complete": true,
  "subject": {
    "kind": "main",
    "entry_source_id": "src-1",
    "symbol": "main"
  },
  "effects": {
    "declared": ["File.Read", "Console.Write"],
    "expanded": ["File.Read", "Console.Write"],
    "complete": true
  },
  "sources": [
    {
      "source_id": "src-1",
      "display_path": "main.bnt",
      "sha256": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
    }
  ],
  "inputs": [],
  "symbols": {
    "process_shell_reference": "absent"
  },
  "diagnostics": []
}
```

`display_path` は表示用であって、親がその文字列を無条件に開く指示ではありません。出力の親読みに関する指摘01は、この形式を導入しても別途守る必要があります。

### 6.3 必須にしたい契約

| 項目 | 推奨する意味 |
|---|---|
| `schema_version` | 機械形式の版。処理系のリリース番号と独立させる |
| `status` | `ok / compile_error / input_error / internal_error` 等。用途ごとの分類を固定する |
| `complete` | 対象集合と必要な結果を最後まで確定できたか。途中停止を空集合と区別する |
| `subject` | どのentry・main・testを検査したか。pathの表示だけで対象を曖昧にしない |
| `effects.expanded` | `IO.All` 等を展開した正規の権利名。承認判定の入力にする集合 |
| `effects.declared` | 診断・説明向け。最小エフェクトを推論しているとの誤解を避ける |
| `sources` | 実際に取り込んだソースと内容ハッシュ。単なる探索候補一覧ではない |
| `inputs` | package定義、解決結果、その他の検査に影響した入力。存在しない入力も必要なら明示する |
| `diagnostics` | 既存の診断へ参照を持たせてもよい。`source_id` と位置の対応を検証できる形にする |
| `symbols` | 任意の補助情報。未対応や確定不能を `unknown` として扱う |

未知のschema、未知のエフェクト、`complete: false`、対象の不一致、欠落した末尾は、空のエフェクトとして許可してはいけません。既定は実行拒否または設計済みの保守的な扱いにし、少ない権限を勝手に推定しないことが必要です。

ソース台帳を付けても、それだけで検査後の変更を防げるわけではありません。ハーネスが実行に使う入力を同じ内容へ固定し、処理系の実体も同じものに結び付ける指摘08は残ります。「レポートにハッシュがあればTOCTOUを解決した」とはしないでください。

### 6.4 Process.shellの情報

`Process.shell` への参照は、文字列検索より、名前解決した結果に基づいて報告できると便利です。ただし、「直接の参照がない」ことは、許可プログラム全体に別言語の実行能力がない証明ではありません。

提案する意味は、例えば「検査対象のソース集合に、該当する標準APIへの解決済み参照がある／ない／調査未完了」です。実行到達性の証明まで要求すると範囲が大きくなります。警告や追加の説明に使い、OSのexec制限の代わりにしないことを勧めます。v0.0.5で `Process.shell` がsh等を使うことは、公開された標準APIの説明から確認できます。[S-B03]

### 6.5 テストの結果例

以下も新規提案の架空データです。

```json
{
  "schema_version": 1,
  "kind": "test_check_report",
  "status": "ok",
  "complete": true,
  "executed": false,
  "targets": [
    {
      "source_id": "src-1",
      "display_path": "tests/math.bnt",
      "status": "ok",
      "tests": [
        {
          "name": "sum",
          "effects": {
            "expanded": ["Console.Write"],
            "complete": true
          }
        }
      ]
    }
  ],
  "aggregate_effects": ["Console.Write"],
  "diagnostics": []
}
```

実際のテスト宣言の識別子の形は、処理系の定義に合わせてください。上の `name` は構造を示すための仮の値です。

BH-023で重要なのは、**本当に実行するtestと同じ発見・package境界・除外規則を共有すること**です。v0.0.5のtestには、ディレクトリ探索や入れ子packageの扱いなどの規則があります。ハーネスが独立に `.bnt` をglobして再現するより、処理系の共通関数で確定させる方がよいでしょう。[S-B05]

一つのファイルの検査失敗後も別ファイルの情報を出せる設計なら、個別の成功と全体の `complete/status` を区別します。ある対象のエフェクトが不明なまま、既知の部分の和集合だけで本番のtestを走らせてはいけません。

### 6.6 BH-022/023の受け入れ試験

| 分類 | 例 | 合格条件 |
|---|---|---|
| エフェクト | なし、複数、IO.All、別名を含むimport | 正規の集合を安定して返す |
| 文法 | コメント・文字列内の `uses`、main以外の宣言 | 字面に依存して誤判定しない |
| 不完全 | 構文エラー、名前解決失敗、内部エラー | 不明を空集合にしない |
| 対象 | mainなし、testなし、複数entry | 対象の種類と結果を明示する |
| 依存 | import先変更、package入力変更 | 台帳とハッシュが変わる |
| test探索 | 入れ子package、重複path、存在しないpath | 本番testと同じ対象集合になる |
| 非実行 | ConsoleやFile.Writeのあるtest | dry-runでは副作用が一切起きない |
| 形式 | stdout途中切断、未知の版、未知フィールド | 消費側が定めた互換規則に従い、未完成を受理しない |
| 診断 | 仮想名、範囲外offset、偽の外部path | 親に新しい読取権限を与えない |

## 7. BH-024/025/026の具体案

### 7.1 BH-024: Skillは「取得」と「インストール」を分ける

ハーネスのビルドや実行時の取得のために、利用者のSkillディレクトリへ一度インストールしてから読む必要がない形が望ましいです。読み取り専用のstdout出力、または資産一覧と本文を取得するモードを勧めます。

多ファイルを扱うなら、資産名は `references/stdlib/IO.File.md` のような相対的な論理名に限定し、`..`、絶対パス、重複、サイズ超過を拒否できる契約にします。返却されたファイル名を、そのまま親の任意ファイルアクセスへ使わないようにします。

必要な情報は、Skillの版、内容のハッシュ、各資産の論理名・長さ・本文です。圧縮形式やインストール先の探索を先にAPIへ固定するより、取得できる内容の同一性を決める方を優先します。圧縮を使う場合は、展開後の総量にも上限を設けます。

BH-021の埋め込み方式との関係では、**ビルド時に固定して埋め込んだ版** と **実行中の処理系の版** が異なる場合を明示してください。警告で続けてよい文書の差と、権限判断の形式が不明なので実行してはいけない差を分ける必要があります。

### 7.2 BH-025: versionに能力を加える

候補例は次の通りです。これも現行出力ではありません。

```json
{
  "schema_version": 1,
  "name": "benitoite",
  "version": "0.0.7",
  "commit": null,
  "dirty": null,
  "target": "example-target",
  "capabilities": {
    "check_report": [1],
    "test_check_report": [1],
    "skill_export": [1],
    "child_environment_policy": [1]
  }
}
```

ソースアーカイブからのビルドなどでcommitが得られない場合は、推測した値を入れずnullにします。機能の有無をSemVerの大小だけで判断しないよう、形式の版を持つ能力を明示します。

このJSONは「信頼できる実行ファイルである証明」ではありません。処理系の実体の解決、書換え可能な場所にないこと、必要なら実体ハッシュを親が記録することは別の責務です。起動時に一度だけ実行する現方針は維持できますが、その後別の実体へ差し替わる状態では、初回の結果を再利用してはいけません。

受け入れ条件は、再現可能な形式、未知の能力を許容しても必須能力の欠落を見逃さないこと、commit不明やdirty buildを正直に表すことです。

### 7.3 BH-026: ローダ環境の扱い

この要望は有用です。Linuxの動的ローダは `LD_LIBRARY_PATH`、`LD_PRELOAD`、`LD_AUDIT` 等を扱い、Appleのdyldにも `DYLD_*` 系の入力処理があります。変数が実際に有効かはOS・保護条件・実行形式で異なるため、同じ列挙だけで全環境の注入が成立すると断定しません。[S-K10] [S-M02]

責務を三つに分けるのがよいでしょう。

| 層 | 役割 |
|---|---|
| ハーネス→処理系の最初の起動 | APIキーやローダ制御を含む継承環境を除去・構成する。処理系のmainが始まる前の読み込みに備える |
| 処理系→子プロセス | 制約付きの起動ポリシーが有効なら、危険な環境の追加・上書きを拒否する |
| OSのサンドボックス | 子がさらに孫を作る場合も、実行・書込・IPCの上限を継承させる |

環境の値を「黙って無視する」より、制約付きモードでは禁止された追加を診断付きで拒否する方が、LLMが原因を理解しやすくなります。一般のBenitoiteプログラムの環境設定機能を無条件に削るのではなく、制約付きで起動したインスタンスの不変の実行コンテキストとして設定する案を勧めます。

ただし、`Process.Run` を承認したコードが別の処理系を起動する例外は、現設計で既に認められています。新しいCLIフラグだけでその子孫が必ず同じモードを選ぶ保証を作ることはできません。**この要望をOSのexec制限の代わりにしないこと**、API上の補助制約とサンドボックス上限を区別することが重要です。

また、`PATH`、プログラムの絶対パス、作業ディレクトリ、言語・ビルドツール固有の設定読み込みは、ローダ環境変数の一覧だけでは解決しません。許可プログラムの審査と実際の起動経路の試験へ回します。禁止変数名を何個増やしたら安全、という完成条件にはしないでください。

受け入れ試験には、親から継承した値、明示的な `Command.environment` の上書き、空環境、既定のPATH、子孫、静的／動的な実行形式を含めます。実際の秘密や攻撃用ライブラリではなく、専用のダミー起動プログラムを使って、環境と拒否結果を観測します。

## 8. 処理系側への依頼に添える要約

依頼文の核は、次の三点で足ります。

1. **実行しない検査で、実際の対象とエフェクト集合を機械可読に確定したい。** 対象集合・完全性・依存ソースの同一性を、checkとtestで共通化してほしい。
2. **提供する形式を版付きの能力として問い合わせたい。** Skillの取得も、インストール副作用を伴わない形にしたい。
3. **子プロセスの環境構築を、制約付き実行に適した形へ強化したい。** ただし、OSの隔離を置き換えるサーバモードまでは求めない。

これで5件の範囲を保ちつつ、ハーネス側の独自パーサ・探索規則・環境操作を減らせます。v0.0.5の暫定経路は、未知を最大権限へ倒す／拒否する現在の方針を維持し、v0.0.7の機械形式が利用できる場合に置き換える構成を勧めます。

---

## この文書の外部出典

[S-A01]: https://platform.claude.com/docs/en/build-with-claude/preserved-thinking "Anthropic: Preserved thinking"
[S-A02]: https://platform.claude.com/docs/en/build-with-claude/streaming "Anthropic: Streaming Messages"
[S-A03]: https://platform.claude.com/docs/en/agents-and-tools/tool-use/fine-grained-tool-streaming "Anthropic: Fine-grained tool streaming"
[S-A04]: https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-reference "Anthropic: Tool reference"
[S-A05]: https://platform.claude.com/docs/en/build-with-claude/prompt-caching "Anthropic: Prompt caching"
[S-A06]: https://platform.claude.com/docs/en/build-with-claude/compaction "Anthropic: Compaction"
[S-B01]: https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/docs/design/02-impl/02-10-diagnostics.md "Benitoite v0.0.5: 診断エンジン"
[S-B03]: https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/crates/benitoite/skill/references/stdlib/IO.Process.md "Benitoite v0.0.5: IO.Process"
[S-B05]: https://github.com/tecogonaz0439/Benitoite/blob/v0.0.5/docs/design/06-tooling/06-01-cli.md "Benitoite v0.0.5: CLI"
[S-D01]: https://api-docs.deepseek.com/guides/thinking_mode/ "DeepSeek: Thinking Mode"
[S-D02]: https://api-docs.deepseek.com/guides/anthropic_api/ "DeepSeek: Anthropic API互換"
[S-K10]: https://man7.org/linux/man-pages/man8/ld.so.8.html "Linux man-pages: ld.so(8)"
[S-M02]: https://github.com/apple-oss-distributions/dyld/blob/main/dyld/DyldProcessConfig.cpp "Apple dyld: 環境変数の読み取り"
[S-O01]: https://developers.openai.com/api/docs/guides/migrate-to-responses "OpenAI: Responsesへの移行"
[S-O02]: https://github.com/openai/openai-python/blob/main/src/openai/types/responses/response_create_params.py "OpenAI公式Python SDK: ResponseCreateParams"
[S-O03]: https://developers.openai.com/api/docs/guides/function-calling "OpenAI: Function calling"
[S-O04]: https://developers.openai.com/api/docs/guides/streaming-responses "OpenAI: Streaming Responses"
[S-O05]: https://developers.openai.com/api/docs/guides/prompt-caching "OpenAI: Prompt caching"
[S-O06]: https://developers.openai.com/api/docs/guides/conversation-state "OpenAI: Conversation state"

各出典の確認日・適用範囲は [出典一覧](05-sources.md) を参照してください。

