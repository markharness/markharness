# 0012: プロジェクトルートの目印を `.markharness/config.toml` に統合し、`--dir` 明示時も実在検証する

## ステータス

Accepted

## 背景

ADR [0011](./0011-markharness-dot-directory-namespace.md) は「`.markharness/` を、markharness がプロジェクトルートに占有する唯一の名前にする」ことを決定理由に掲げていた。しかし実際には、プロジェクトルートの目印(マーカー)である `.markharness.toml` はその決定の対象外で、ルート直下に独立したファイルとして残っていた。結果として、初期化済みプロジェクトのルート直下には `.markharness.toml` と `.markharness/` という、ほぼ同名の2つのドットエントリが並ぶことになり、ADR 0011 自身の「唯一の名前」という主張と矛盾していた。名前がほぼ同一であるだけに、一見すると置き忘れられた重複ファイルのようにも見え、`Cargo.toml`+`target/`、`package.json`+`node_modules/` のような「無関係な名前を持つマーカー+ツール専用ディレクトリ」というよくある分離パターンとも異なっていた。

あわせて、`project_root::resolve()` は `--dir` が明示された場合、上位探索(`find_root`)を一切行わず、渡されたパスをそのままプロジェクトルートとして信頼していた。マーカーの実在確認はしていなかったため、存在しない/未初期化のディレクトリを `--dir` に渡すと、後続処理内の分かりにくい汎用的なファイルI/Oエラーとして失敗していた。これは `cargo --manifest-path`(指定した manifest の実在を検証してから使う)のような一般的なCLIの慣習から外れていた。

## 検討した選択肢

マーカー配置について:

1. 現状維持: `.markharness.toml` をルート直下の独立ファイルのままにする。`Cargo.toml`+`target/` 型の分離パターンとして正当化する。
2. `.markharness/config.toml` に統合し、ルートマーカー判定を `.markharness/` 名前空間内のファイル実在チェックに変更する。

`--dir` 明示時の検証について:

1. 現状維持: 明示パスは検証なしに信頼する。
2. `cargo --manifest-path` 型に倣い、明示パスにもマーカー実在チェックを追加し、なければ `markharness init` を促すエラーで終了する。

## 決定

マーカー配置は選択肢2、`--dir` 検証も選択肢2を採用した。

**判断理由(マーカー配置)**:

- `Cargo.toml`/`target/` や `package.json`/`node_modules/` の分離パターンは、マーカーとツール専用ディレクトリの名前が無関係であることが前提にある。`.markharness.toml` と `.markharness/` はほぼ同名であり、この前提を満たさない。名前を揃えたまま2箇所に置く必然性は薄く、ADR 0011 自身の「唯一の名前」という狙いに合わせる方が一貫している。
- `.markharness/config.toml` は `.markharness/` 名前空間の内側にあるため、トップレベルの汎用名(例えば独立した `config.toml`)が他ツールと衝突する懸念とは無縁である。これは ADR 0011 が `knowledge/`・`schema/` 等6ディレクトリに対して既に許容している前提と同じ扱いである。

**判断理由(`--dir` 検証)**:

- `--dir` は「上位探索はせず、指定パスをそのままターゲットとして使う」という点で `cargo --manifest-path` や `npm --prefix` と同じ「明示パス=正確なターゲット」方式だが、`cargo --manifest-path` は指定ファイルの実在を検証してから使う。markharness の `--dir` だけが検証を省いていたのは他CLIの慣例から外れた不整合であり、実在しないディレクトリを渡した際のエラーメッセージも分かりにくかった。
- マーカーをルート直下の独立ファイルから `.markharness/` 名前空間内に移す本変更と合わせて実施することで、`resolve()` 内の検証ロジックを一本化できる(`find_root` の上位探索・`--dir` 明示時の検証のどちらも同じ `MARKER_FILE` 定数を見る)。

これは移行手段・後方互換シムを伴わない破壊的変更である。`.markharness.toml` を使っていた既存プロジェクトは、内容を `.markharness/config.toml` に手作業で移す必要がある。本決定時点で自動移行コマンドは存在しない。

`import --source junit` のように、対象がそもそもプロジェクトルートを必要としない(`--dir` を受け取っても未使用の)コマンド経路は、この検証の対象から外した(`src/cli.rs`)。`--dir` 検証の目的は「渡されたパスがプロジェクトルートとして正しいか」を確認することであり、プロジェクトルートを必要としない処理にまで検証を課すのは本決定の趣旨(他CLI慣例との整合)を超える。

## 対応内容

- `src/project_root.rs`: `MARKER_FILE` を `.markharness.toml` から `.markharness/config.toml`(`MARKHARNESS_DIR` 配下の文字列リテラル。`Path::join` は `const` 文脈で使えないため手動で合成し、テストで同期を検証する)に変更した。
- `src/project_root.rs`: `resolve()` は `--dir` 明示時にも `MARKER_FILE` の実在を検証し、なければ `find_root` 失敗時と同じ「`markharness init` を促す」`NotFound` エラーを返すようにした。
- `src/init.rs`: `ensure_project_root_marker` の書き込み先を `.markharness/config.toml` に変更した(`.markharness/` は `run_init`内で先にサブディレクトリ作成が走るため、書き込み時点で親ディレクトリは既に存在する)。
- `src/cli.rs`: `import --source junit` はプロジェクトルートを使わないため、`project_root::resolve()` の呼び出しを `--source native` の分岐内に限定した(以前は `Import` コマンド全体で無条件に呼んでおり、マーカー実在検証を追加すると本来ルート不要なjunitインポートまで巻き込んで失敗する不具合を誘発するところだった)。
- テストフィクスチャ(`tests/knowledge_cli.rs`・`tests/plan_cli.rs`)のうち、`markharness init` を経由せず手作業で `.markharness/knowledge`・`axes` 等を組み立てていた箇所に、マーカーファイルの書き込みを追加した(実プロジェクトなら `init` 済みであるはずの状態を正しくシミュレートするための修正であり、検証ロジック側を緩めるものではない)。
- ドキュメント(`docs/ja/cli-manual.md`、`docs/en/cli-manual.md`、`docs/ja/knowledge-from-code.standalone.md`)の `.markharness.toml` への言及を `.markharness/config.toml` に更新した。
