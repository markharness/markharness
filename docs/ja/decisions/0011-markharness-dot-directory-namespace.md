# 0011: 管理対象ディレクトリを単一の `.markharness/` 名前空間に集約する

## ステータス

Accepted

## 背景

`markharness init` は `knowledge/`、`axes/`、`generated/`、`executions/`、`changes/`、`schema/` という6つのディレクトリをプロジェクトルート直下に作成していた。これらは汎用的な名前であり、既存のソフトウェアプロジェクトが既に(ドメイン知識としての`knowledge/`、DB/APIの`schema/`、ビルド成果物の`generated/`等)無関係な用途で同名ディレクトリを使っている可能性がある。衝突が実際に起きなくても、リポジトリのトップレベルを見ただけでは、これら6つが `markharness` の管理下にあるのか、通常のプロジェクトコンテンツなのか判別できないという問題もある。

これは既存プロジェクトへ `markharness` を導入する際の具体的な懸念として提起された:6つの汎用的なトップレベル名を一度に占有することは、明確にスコープされた1つの名前を占有することに比べ、名前衝突の確率を明らかに高める。

## 検討した選択肢

1. 現状のフラットなトップレベル構成を維持する(何もしない)。
2. 6つのディレクトリを単一の `.markharness/` 名前空間(`.markharness/knowledge/`、`.markharness/axes/`、...)配下に集約する。
3. デフォルトを `tests/markharness/` にする(これらをテスト関連資産として扱い、一部エコシステムの `tests/`/`test/` 慣習に従う)。
4. 設定ファイルの `paths:` セクションで各ディレクトリの配置先を個別に設定可能にし、現状のフラット構成をデフォルトのまま維持する。

## 決定

選択肢2を採用した。`markharness init` は6つのディレクトリすべてを `.markharness/` 配下に作成するようになり、`.markharness/` が `markharness` がプロジェクトルートに占有する唯一の名前となる(既存の `.markharness.toml`(プロジェクトルートの目印)と `.markharness-cache/` も既にドット始まりだった)。

**判断理由**:

- ドットディレクトリは「特定のツールが所有するディレクトリ」を表す確立された慣習であり、それ単体で「ローカル管理・非コミット」と読まれるわけではない。`.github/`、`.changeset/`、`.devcontainer/`、`.husky/` はいずれもコミット対象であることが期待される、内容が広く知られたドットディレクトリの例である。特に `.changeset/` は構造的に近い前例だ:ツールが所有する名前空間の中に人間が書いた構造化された git-native な記録があり、後続のステップがそれを加工して生成物を作る、という点で `.markharness/knowledge/` から `.markharness/generated/` への流れと同じ形をしている。
- 6つの汎用的なトップレベル名を1つのドット始まりの名前に減らすことで、既存プロジェクト自身の `knowledge/`、`schema/` 等との衝突リスクを最小化しつつ、`init` の出力が自己記述的であり続ける。
- 選択肢3(`tests/markharness/`)は却下した:`knowledge/`、`axes/`、`schema/` はテストコードではなく、また pytest や Jest など多くのテストランナーは慣習的に `tests/`/`test/` 配下を広くグロブするため、導入先プロジェクトの無関係なツールが markharness 管理ファイルを誤って収集するリスクがある。また、`tests/` を採用していないプロジェクトにまで `tests/` という慣習を強制することになり、「自身の名前空間の外にディレクトリ慣習を強制しない」という `markharness` の意図に反する。
- 選択肢4(ディレクトリ単位の完全な設定可能化)は時期尚早として却下した:現状、土台となるパス解決の抽象化が存在せず(約150箇所の呼び出し元がそれぞれ直接リテラルのパスセグメントを `.join` していた)、また6つのディレクトリは今のところ互いに異なる扱いを受けていない(`generated/` や `executions/` を含む6つすべてがこの git-native モデルではコミット対象であり、gitignore 対象は `.markharness-cache/` のみ)。6系統の独立した設定可能なルートを導入することは、ツールの実際の挙動にはまだ存在しない区別(ディレクトリごとの配置・ディレクトリごとの git 上の扱い)を先取りして設計することになる。将来的に配置可能化の具体的なニーズが生じれば、その時点で本変更が導入した `MARKHARNESS_DIR`/`KNOWLEDGE_PATH_IN_REPO` 定数を、そうした設定項目が接続される唯一の場所として再検討すればよい。
- 「`.markharness/` がローカル管理と読まれる可能性」という懸念は、新たな安全チェック機能ではなくドキュメントで対応することとした(`.markharness/` が誤って gitignore された場合に警告するような `markharness doctor` 的なコマンドも検討したが、実際にそうした間違いが起きたという具体的な報告が出るまでは YAGNI として意図的に見送った)。

これは移行手段・後方互換シムを伴わない破壊的変更である。フラット構成で初期化済みの既存プロジェクトは、6つのディレクトリを手作業で `.markharness/` 配下に移動する必要がある(または新しい場所で `markharness init` をやり直し、知識を再適用する)。本決定時点で自動移行コマンドは存在しない。

## 対応内容

- `src/project_root.rs`: 6つのディレクトリの配置先を組み立てる唯一の定数として `MARKHARNESS_DIR`(`".markharness"`)と `KNOWLEDGE_PATH_IN_REPO`(`".markharness/knowledge"`。`Path::join` ではなく git pathspec 文字列が必要な箇所、例えば `git ls-tree`/`git rev-parse <rev>:<path>` の引数で使用)を追加した。
- `src/init.rs`: `SUBDIRS` は `root.join(MARKHARNESS_DIR)` 配下に作成されるようになった。`.gitignore` エントリのコメントには、`.markharness/` 本体(`.markharness-cache/` 以外)を `.gitignore` に追加してはならない旨を明記した。
- それ以外の、以前 `root.join("knowledge")` のような形でパスを組み立てていたモジュールはすべて、`root.join(MARKHARNESS_DIR).join("knowledge")`(または git pathspec 引数の場合は `KNOWLEDGE_PATH_IN_REPO` 定数)経由に変更した。
- `src/fs_safety.rs`: `replace_dir_from_staging` は、リネーム先ディレクトリの親ディレクトリを事前に作成するようになった。管理対象ディレクトリの親(`.markharness/`)は、プロジェクトルート自体とは異なり、既に存在するとは限らなくなったため。
- ユーザー向けCLI出力(`src/cli.rs`、`src/presentation.rs`、`src/validate.rs`)を `.markharness/...` パスを参照するよう更新した。
- README.md / README.ja.md を更新し、`.markharness/` 配下は `.markharness-cache/` を除きコミット対象である旨の短い注記を追加した。
