# テスト知識管理のGit-nativeモデル：統合版

### A Version-Aware Knowledge Graph Model for Git-Native Test Knowledge Management

**位置づけ**：本資料は「テストケース管理手法*研究テーマ検討まとめ」(検討経緯・不採用案の記録)と「テスト知識管理のGit-nativeモデル*実践論文ドラフト」(v1〜v10)を1本に統合したもの。検討経緯は付録Aにまとめ、本編(第1〜8章)は現時点で確定している設計・評価計画のみを記載する。

**論文種別**：設計提案＋リファレンス実装のレポート(ツール・実践論文ドラフト)。第5章に示す被験者実験による実証的評価(RQ1の検証)は未実施であり、Future Work(第7章・第8章)として明記する。
**想定投稿先**：ESEM / ICSME / SANER 等の実践寄りトラック、または国内ではSES / JSSST。ただし上記は実証的評価の完了を前提とする投稿先であり、現状の未実施の段階ではツール・アーキテクチャ提案トラック(Tool Demo等)への投稿、または実験完了後の投稿が現実的な選択肢になる。

---

## 0. 経緯サマリー

1. **初期案(A案)**：機能構造・テストケース・実行結果・マイルストーンを統合する情報モデル。研究テーマとしては要件⇔テスト⇔実装のトレーサビリティ研究(Cleland-Huang, 2011等)と重なり新規性が弱いが、「バージョン軸を第一級概念とする派生関係の追跡」は既存TMSにない差別化余地として残った(付録A・検討まとめ第1章)。
2. **Git階層・グラフ構造案との統合**：テスト知識(Requirement/Feature/Behavior/Condition/ExpectedResult)を木構造+横断的観点(Axis、グラフ構造)で管理し、テストケースをその派生物として扱うモデルに統合(検討まとめ第2〜3章)。
3. **LLM活用への全面ピボット案(不採用)**：「AI専用知識グラフ」への転換を検討したが、(a)クエリ速度・使いやすさの懸念は対象がLLMになっても解消されない、(b)新規性の主張として弱い、(c)評価方法が根本的に変わり単独では査読耐性が下がる、との理由で不採用(付録A.1)。
4. **部分ピボットと段階的な設計修正**：人間向けモデルを土台に、LLM角度は将来課題として切り出した上で、研究テーマを「案1：構造表現」単独に絞り込み(検討まとめ第4章)、以降10回以上の技術的指摘を受けて以下を確定させた。
   - 系譜キーを人間の手動整数からGitのコンテンツアドレス(blob SHA)＋祖先探索(`git merge-base`)に変更(第3.2節)。
   - 開発中のリアルタイム照会と、研究評価対象である永続的な版履歴DAGを、別のグラフ(構造的生成グラフ vs 版履歴DAG)として明確に分離(第3.2節、貢献の範囲を限定)。
   - id解決をコミット対象の単一ファイルからGitの`commit-graph`と同じ設計思想の非コミットキャッシュに変更し、内容アドレス方式のキャッシュキーと破棄条件を明記(第3.3節)。
   - 系譜確定のタイミングをコミット単位からマイルストーン境界単位に変更し、ブランチ戦略(merge/rebase/squash)非依存にした(第3.4節)。
   - 既存の大規模リポジトリへの移行を可能にする、マイルストーン限定・非同期・Git notes・遅延計算によるバックフィルアーキテクチャを本編に組み込んだ(第4章)。
   - 実験の対照群を、自作の疑似TMSや人工的な単一ツール比較ではなく、対象組織が実際に使う複合運用に統合(第5.2節)。
   - タスクを「既存運用でも対応可能な浅い変更」と「既存運用が原理的に対応不能な深い変更」に層別化し、正答率(特に深い変更層)を主指標とした(第5.3節)。
   - 正解データの構築を、記憶に依存する聞き取りから、当時の成果物(co-change等)に基づく機械的な再構成に変更し、その際のノイズ除去基準を明記した(第5.4節)。

以下、本編。

**注(実装状況について)**：本編は当初の設計を記す。CLI実装(`markharness`、本リポジトリ)は中核アイデア(tree SHAベースの系譜キー・TestCase派生管理・ChangeEventのマイルストーン境界自動生成)を検証する段階にあり、設計の一部は実装時に簡略化・変更されている。主な相違点は各該当節に注記し、§3.6に一覧をまとめた。詳細な突き合わせは別紙[gap-analysis-mm-folder.md](./gap-analysis-mm-folder.md)を参照。

---

## 1. Introduction

### 1.1 動機

ソフトウェア開発における仕様変更は頻繁に発生し、その都度「どのテストケースを再確認すべきか」を判断する必要がある。既存のテスト管理ツール(TestRail・Zephyr Scale・Xray・qTest等)は、要件・機能・テストケース間の静的なトレーサビリティ(現時点のスナップショット)やマイルストーン管理機能を備えるが、いずれも現時点のスナップショット中心の設計であり、複数リリースにまたがる派生関係を第一級のクエリ対象として扱う設計にはなっていない(検討まとめ第1.3章)。素朴なGit運用(Markdown/YAMLでのテストケース管理)も実務に存在するが、実行追跡・変更影響の系統的な追跡機能を欠く。

**図1：現状運用から提案モデルへ**

```mermaid
flowchart LR
  subgraph BEFORE["現状運用（対照群）"]
    direction TB
    B1["TestRail等 TMS\n(現時点のスナップショット)"]
    B2["Jira等\n(課題とテストの紐付け)"]
    B3["git log / git blame\n(手動での履歴確認)"]
  end
  GAP["原理的に答えられない問い：\n『過去のある変更は、今のどのテストに影響するか』"]
  subgraph AFTER["提案モデル（実験群）"]
    direction TB
    A1["knowledge/\n(Feature / Condition / ExpectedResult)"]
    A2["derived_from DAG\n（マイルストーン境界、第3章）"]
    A3["ChangeEvent 自動生成\n→ 影響TestCase特定"]
  end
  BEFORE --> GAP --> AFTER
```

現状運用(TestRail/Jira/git検索の組み合わせ)は、いずれも「過去のある変更が今のどのテストに影響するか」という問いに、版間の派生関係を保持していないため原理的に答えられない(第1.3節・第2.4節)。本研究はこの空白を、Git自身のオブジェクトモデルを土台にした版履歴DAGで埋める(第3章)。

### 1.2 研究課題(RQ)

> RQ1: 明示的な版履歴(derived_from)を持つテスト知識モデルは、対象組織のテスターが実際に使用している現状の運用(TMS・課題管理ツール・git検索等の組み合わせ)と比較して、特に**複数世代にわたる変更影響の識別タスク**において、正答率・所要時間を改善するか。

本研究はRQ1を中心課題とし、単一の研究課題に絞り込む。構造からのテストケース自動生成、Git粒度分割によるレビュー性向上等の関連課題は将来課題とする(第7章)。LLMによる文脈供給・手順書自動生成への応用は、検討の結果、本研究のスコープから完全に除外した(付録A)。

**RQ1の現状の位置づけ**：RQ1は第5章で評価計画(タスク層別化・正解データ構築方法・被験者割当)まで設計済みだが、被験者実験そのものは本ドラフト時点で未実施である。したがって「正答率・所要時間を改善するか」は現時点では**検証済みの結果ではなく、設計と評価計画によって裏付けられた仮説**として扱う。本文中の「改善する」「特定できる」等の記述は、明記のない限りモデルの設計上の期待(第3章のモデル構造から論理的に導かれる性質)を指し、被験者実験による実証結果を指すものではない。実証は第8章 Conclusion に記す通りFuture Workである。

### 1.3 貢献

1. **核心的貢献**：Gitのコンテンツアドレス(blob SHA)・非コミットのid解決キャッシュ・コミットグラフの祖先探索(`git merge-base`)を組み合わせ、ブランチ運用に依存せずマイルストーン境界で版履歴を導出するモデルの設計(第3章)。
2. 横断的観点(Axis)を物理ディレクトリ構造から独立させ、木構造を保持したまま多対多関係を表現する設計パターン(第3.5節)。
3. 既存の大規模リポジトリへの段階的な導入を可能にする、マイルストーン単位の非同期バックフィルアーキテクチャ(第4章)。
4. 対象組織の実際の現状運用を対照群とし、正解データを当時の成果物から再構成した、実データに基づく評価設計(第5章)。

**注**：開発者が作業ブランチ上で即座に行える差分照会(第3.2節の構造的生成グラフを使う実装上の利便機能)は、版履歴DAGを使わないため本研究の核心的貢献・RQ1の評価対象には含めない(検討経緯は付録A参照)。

---

## 2. Related Work

### 2.1 要件・テストのトレーサビリティモデル

Agile Traceability Information Model(Cleland-Huang et al., 2011)をはじめとする既存研究は、要件⇔テスト⇔実装の静的なトレーサビリティモデルを確立している。本研究はこれらのモデルと競合するものではなく、これらが扱わない「バージョン軸に沿った複数世代の派生関係の追跡」を対象とする点で補完的である。

### 2.2 知識グラフによるテスト管理

知識グラフを用いたテストデータ管理(Software Test Data Management Based on Knowledge Graph)、システムズエンジニアリング領域のオントロジーベース知識グラフなど、類似の知識グラフ応用研究が存在する。これらは主にデータ管理・モデル管理を対象としており、Gitのバージョン管理機構と統合した「版履歴の第一級モデル化」は扱っていない。

### 2.3 分類木によるテスト設計技法

Classification Tree Method(CTM)は、分類木からのテストケース生成という点で本研究のFeature+Condition→TestCase生成と発想を共有する。ただし、CTMはテスト設計技法であり、Git管理・バージョン履歴・実行結果追跡を含むライフサイクル管理は範囲外である。本研究はテスト設計技法ではなく、設計後のライフサイクル管理を主眼とする点でCTMと立場が異なる。

### 2.4 既存テスト管理ツール・Git-native運用との比較

TestRail・Zephyr Scale・Xray・qTest等の主要製品はマイルストーン機能・トレーサビリティ機能を備えるが、いずれも現時点のスナップショット中心であり、複数世代にわたる派生関係を辿るクエリ機能は一般的に存在しない(TestRailは2026年時点でオンプレミス版の提供を終了しクラウド専用。Xrayは Jira Data Center 経由で自己ホスト可能)。無料・自己ホスト可能な既存選択肢(Kiwi TCMS、TestLink、Klaros Test Management)も同様の制約を持つ。実務では、これら単体のツールではなく、TMS・課題管理ツール(Jira等)・git検索を組み合わせて運用するのが一般的だが、いずれの組み合わせも過去の世代の変更を体系的には辿れない。本研究はこの構造的な欠落を埋める位置づけにあり、評価設計(第5章)はこの実態を反映する。

---

## 3. Model Design

### 3.1 テスト知識の構造

木構造(Requirement → Feature → Behavior → Condition → Expected Result)を基本とし、以下を追加する。

- `AXIS`：横断的観点(例：Gameplay / Animation / AI / Network)。`FEATURE`と多対多で交差を表現(グラフ構造部分)。
- `TESTCASE`：`FEATURE`と`CONDITION`から生成される派生物(一次管理対象ではない)。
- `TESTEXECUTION` / `MILESTONE`：実行結果とリリース単位の管理。
- `CHANGEEVENT`：`FEATURE`の変更が`TESTCASE`へ伝播する経路(変更影響分析の対象)。
- `FEATURE`の自己参照関係(2種類に分離)：
  - `derived_from`：同一Featureの版履歴。Gitのtree SHAと祖先探索から、マイルストーン境界で導出する(3.2〜3.4節、本モデルの核心)。
  - `forked_from`：異なるFeature間の概念的派生(例：double-jumpがground-jumpの仕様を土台に設計された、という設計上の依存関係)。Git履歴には現れないドメイン知識であり、手動記述が必須。実装では`feature.yml`のfront matterの任意フィールドとして提供済み(第3.6節)。

**実装注記**：CLI実装は`REQUIREMENT`を`requirement.yml`として明示ファイル化し、`knowledge/<requirement>/<feature>/...`という階層でFeatureをその直下に置く(第3.5節のディレクトリ構造も参照)。`feature.yml`は親を`requirement: <requirement_id>`で参照する。`requirement.yml`は`source`(要件の出所、任意)・`related_issues`(外部issueトラッカーへの参照配列、任意)も持てる(製品化提案、論文本文には明記なし)。両フィールドとも人間が手動で記入する参照情報であり、これを読んで検証・生成を行うロジックは実装していない。

#### ER図(Mermaid)

```mermaid
erDiagram
  REQUIREMENT ||--o{ FEATURE : decomposes
  FEATURE ||--o{ FEATURE : "derived_from (git tree-hash + ancestor search, milestone-scoped)"
  FEATURE }o--o{ FEATURE : "forked_from (manual, cross-entity)"
  FEATURE ||--o{ BEHAVIOR : has
  BEHAVIOR ||--o{ CONDITION : has
  CONDITION ||--o{ EXPECTEDRESULT : has
  AXIS }o--o{ FEATURE : crosscuts
  FEATURE ||--o{ TESTCASE : generates
  CONDITION ||--o{ TESTCASE : generates
  TESTCASE ||--o{ TESTEXECUTION : executed_as
  MILESTONE ||--o{ TESTEXECUTION : contains
  CHANGEEVENT }o--|| FEATURE : affects
  CHANGEEVENT ||--o{ TESTCASE : impacts

  REQUIREMENT { string requirement_id PK }
  FEATURE { string feature_id PK
            string label }
  BEHAVIOR { string behavior_id PK }
  CONDITION { string condition_id PK }
  EXPECTEDRESULT { string result_id PK }
  AXIS { string axis_id PK }
  TESTCASE { string case_id PK }
  TESTEXECUTION { string execution_id PK
                  string result }
  MILESTONE { string milestone_id PK }
  CHANGEEVENT { string event_id PK }
```

`FEATURE`は版番号を人間が手動で管理するフィールド(`version`整数)を持たない。系譜計算に使う識別子は、front matterに書く値ではなく、**Gitのオブジェクトストアが既に保持している識別子**であり、`label`は表示専用(系譜計算には使わない)。

**実装注記(blob SHA→tree SHAへの変更)**：当初`feature.yml`単体のblob SHAで変更検知する設計だったが、これだと`feature.yml`自体は不変のままConditionやBehavior、ExpectedResultだけが変更された場合に検知漏れが起きる不具合があった。CLI実装はこれを修正し、Featureディレクトリ配下(`feature.yml`＋その下のbehavior/condition/expected一式)を含む**Gitツリーオブジェクトのtree SHA**を比較する方式に変更している(`id_cache::resolve_feature_versions`、旧`resolve_feature_blobs`)。以降、本節の「blob SHA」は特記ない限りこの「Featureディレクトリのtree SHA」を指す。

**図2：Featureの派生関係（derived_from と forked_from）**

```mermaid
flowchart LR
  F1["player-jump\n(milestone 1, tree A)"] -->|derived_from（自動）| F2["player-jump\n(milestone 2, tree B)"]
  F2 -->|derived_from（自動）| F3["player-jump\n(milestone 3, tree C)"]
  F3 -.->|forked_from（手動記述）| F4["player-double-jump\n(概念的な派生、新規Feature)"]
```

同一Featureの版が進む`derived_from`はマイルストーン境界でCIが自動導出する(第3.2〜3.4節)のに対し、`player-double-jump`のように別のFeatureとして分岐する`forked_from`は、Git履歴に現れないドメイン知識のため手動記述する(第3.1節)。

### 3.2 版履歴の導出：2つのグラフと役割分担

本モデルには目的の異なる2種類のグラフが存在し、これを区別することが実装・評価の両面で重要である。

**(A) 構造的な生成グラフ(静的、版に依存しない)**：`FEATURE`/`CONDITION`→`TESTCASE`という`generates`関係。現在のFeature/Conditionからどのテストケースが生成されるかを表す静的な構造であり、版履歴を必要としない。開発者が作業ブランチ上で「今この変更で、どのTestCaseが再生成されるか」を知りたい場合、必要なのはこの生成グラフと、現在の変更内容(HEADと基準点の単純な差分)だけであり、これは実質的に`git diff`をスコープした処理である。**この機能は実装上の利便機能であり、研究上の核心的貢献・RQ1の評価対象には含めない**(既存運用にはこの機能自体が存在しないため実務上の価値はあるが、版履歴DAGを一切使わないため評価軸としては切り分ける)。

**(B) 版履歴DAG(derived_from、マイルストーン境界で確定)**：同一Featureが世代を経てどう変化してきたかを表す、本研究の核心的なモデル。既存TMS・素朴なGit運用のいずれも持たない機能であり、RQ1が検証する対象はこちらに限定する。

版履歴DAG(B)の導出は以下の通り。

- **tree SHAが担うこと**：Featureディレクトリ内容の衝突しない識別。内容が異なれば必然的に異なる値になるため、ブランチ分岐時の番号衝突(人間が手動で整数を上げる場合に起こりうる)は起きない。ただし、これだけでは「どのtreeがどのtreeから派生したか」という親子関係は一切わからない。
- **祖先探索が担うこと**：マージコミットMの親P1・P2から、マージベース(共通祖先)Bを特定するには`git merge-base P1 P2`によるコミットグラフの探索が必要であり、ハッシュの比較だけで済む処理ではない(Gitのcommit-graphファイル・世代番号による最適化により実務上は効率的だが、明示的なグラフアルゴリズムの実行である)。
- 対象idについて、tree(B)・tree(P1)・tree(P2)・tree(M)を取得し、以下のように場合分けする。
  - tree(P1) == tree(B) かつ tree(P2) != tree(B)：P2側でのみ変更。線形履歴として扱う。
  - tree(P1) != tree(B) かつ tree(P2) != tree(B) かつ tree(P1) != tree(P2)：両ブランチが独立に変更した真の分岐。`derived_from`は[tree(P1), tree(P2)]の2親として記録する。
  - tree(P1) == tree(P2)：1親として扱う。

この機構(祖先探索を伴う詳細な系譜再構築)は、監査用途の副次機能として提供し、研究評価で使う主系譜は次節のマイルストーン境界方式を用いる。

**実装状況**：`markharness changes compute`(マイルストーン境界の主系譜)は、指定した2つのマイルストーンタグ(`from_milestone`/`to_milestone`)間で各Featureのtree SHAを直接比較する処理を基本としており、これは設計上の意図的な選択である(第3.4節、RQ1の評価対象はマイルストーン境界の線形比較)。本節で述べた`git merge-base`による祖先探索・2親分岐の判定自体は、監査用途の副次機能として`markharness changes lineage --commit <merge-sha>`に独立実装されている(`src/lineage.rs`)。指定したマージコミットの2親(P1・P2)と`git merge-base`によるマージベース(B)のtree SHAを比較し、各Featureごとに「線形(linear)」「真の分岐(true_divergence)」「1親相当(single_parent)」を判定して出力する。

**統合(2026-08追記)**：`changes compute`は、`from_milestone..to_milestone`の区間全体を`git rev-list --ancestry-path`で走査し、区間内に存在する全ての2親マージコミットそれぞれについて上記の`lineage`判定ロジックを内部で呼び出す。当該Featureがいずれかのマージで`true_divergence`と判定された場合、`ChangeEvent`に新設した`true_divergences: Vec<TrueDivergence>`フィールド(`TrueDivergence`は監査用の`merge_commit`と`parent_tree_shas: [tree(P1), tree(P2)]`を持つ)へ、区間内で発生した順(古い順)に記録する。同一Featureが区間内で複数回真の分岐を起こした場合も、マージごとに1エントリずつ蓄積されるため取りこぼさない。この統合は加算的な変更であり、`changes/*.yaml`の既存レコード(`true_divergences`を持たない)は`#[serde(default)]`によりそのまま読み込める。当初は`to_milestone`タグが直接マージコミットを指す場合のみの部分統合だったが、区間内の任意の位置でのマージを検出できるよう一般化した。

### 3.3 id解決：非コミットキャッシュとキャッシュキー・破棄条件

`id`はパスに依存しない設計(第3.5節)のため、「あるコミット時点でid Xのファイルがどのパスにあったか」を知るには、単純には全木走査が必要になり、大規模リポジトリでは計算量が破綻する。かといって、id→パスの対応をコミット対象の単一マニフェストファイルとして持つと、複数ブランチが同時にテスト知識を追加するたびにこのファイルがマージコンフリクトを起こし、Gitの並行開発の強みを殺してしまう。

**対応方針**：Gitが同種の問題(コミットグラフ上の祖先探索の高速化)を`commit-graph`ファイル(バージョン管理対象外の補助キャッシュ)で解決しているのと同じ設計思想を採る。id解決の結果を**コミット対象から外し**、各開発者のローカル環境・各CIランナーが必要に応じて独自に再構築する非コミットキャッシュとして扱う。

**キャッシュキーの構成**

```
cache_key = hash(
  tree_sha(knowledge/ 配下のGitツリーオブジェクトSHA),
  canonicalization_rule_version(正規化ルールのバージョン),
  id_index_schema_version(id-indexのフォーマット自体のバージョン),
  tool_version(id解決ツールのバージョン)
)
```

`tree_sha`はコミットSHAではなく`knowledge/`サブツリーのGitツリーオブジェクトSHAを使うことで、無関係なディレクトリの変更による無駄な再計算を避ける。作業ツリーの未コミット変更は`git hash-object`で仮想的に計算しキーに含める。

**破棄条件(インバリデーション)**

1. `tree_sha`の変化：`knowledge/`配下の内容変化。
2. `canonicalization_rule_version`の変化：正規化ルール(どのフィールドを意味的な変更とみなすか)自体の改訂。
3. `id_index_schema_version`の変化：id-indexのフォーマット変更。
4. `tool_version`の変化：id解決アルゴリズム自体の変更。
5. 明示的な手動破棄：`--no-cache`オプション・`rebuild`コマンドによるフェイルセーフ。
6. TTLによる安全網：内容アドレス方式のキー計算に見落としがあった場合の保険として、CI側の共有キャッシュストレージに最大保持期間(例：30日)を設定する。

読み込み時は格納されているキーが現在の状態と完全に一致するかを検証し、不一致なら静かに再計算する。これにより異なるCIランナー間でキャッシュを共有しても、古い/破損したキャッシュを誤って信頼するリスクを避ける。

**実装状況**：CLI実装の`.markharness-cache/<ref>.json`は、本節で述べた内容アドレス方式キャッシュキー(`tree_sha(knowledge/)` + `canonicalization_rule_version` + `id_index_schema_version` + `tool_version`の合成)を実装している(`src/id_cache.rs`の`CacheKey`/`compute_cache_key`)。読み込み時に格納されているキーを再計算した現在のキーと比較し、不一致なら静かに再計算・上書きする。`tree_sha`は`git rev-parse <ref>:knowledge`で取得し、`tool_version`はビルド時のcrateバージョン(`CARGO_PKG_VERSION`)を用いる。ただし`canonicalization_rule_version`・`id_index_schema_version`は現状固定値"1"で、これらのバージョンを実際に上げる正規化ルール改訂・フォーマット改訂はまだ発生していない。作業ツリーの未コミット変更を`git hash-object`で仮想的にキーへ含める処理、およびCI共有ストレージ側のTTL安全網は未実装(そもそもCLI単体の責務外)である。また、id自体も「idはパスに依存しない」という設計方針に沿って改修され、現状は**feature.ymlの`id:`フィールドを正準ソースとする**(id_cache.rsがディレクトリ名ではなくfeature.ymlの内容を`git show`で読んでidを決定する)。これにより、Featureディレクトリをリネームしても`id:`フィールドが変わらなければ同一Featureとして追跡できる(最小限のリネーム追跡)。同一idを持つ複数のFeatureディレクトリが存在する場合はエラーとして検出する。ただし、id⇔pathの汎用的な独立インデックス層(パス変更を伴わない任意のid変更の追跡等)までは実装しておらず、目標設計の完全な実現ではない(第3.6節)。

### 3.4 マイルストーン境界での系譜確定

系譜の確定タイミングはコミットごとではなく、マイルストーン確定時(リリースタグ等)にのみ行う。各idについて「前回マイルストーン時点のtree」と「今回マイルストーン時点のtree」をid解決経由で比較し、差分があれば`derived_from`を記録する。merge/rebase/squashいずれのブランチ戦略にも依存しない。

**実装状況**：`markharness changes compute`自体は`from_milestone`/`to_milestone`を明示引数として受け取り、その2点間のtree SHA差分を計算する処理であり、「直前のマイルストーン」を自動判定する機能はコマンド自体には無い。「直前のマイルストーンと自動的にペアリングする」という運用は、第4章のバックフィルワーカー(`markharness backfill run`)側が`executions/<milestone>/`をタグの日時順に並べて隣接ペアに適用することで実現しており、この2つは別のレイヤーである。

**図3：Version DAG（ブランチ分岐・マージを含む版履歴）**

```mermaid
flowchart TB
  M1["Milestone n-1\nblob B（共通の基点）"] --> BR1["Branch A で変更\nblob P1"]
  M1 --> BR2["Branch B で変更\nblob P2"]
  BR1 --> M2["Milestone n\nblob M（マージ後）\nderived_from: [P1, P2]"]
  BR2 --> M2
  M2 --> M3["Milestone n+1\nblob N"]
```

第3.2節の場合分けの通り、両ブランチが独立に同一idを変更していれば`derived_from`は2親(P1・P2)を持つノードとして記録され、片方のみの変更であれば線形履歴として扱われる。マイルストーン境界でのみ確定するため、中間のコミット粒度やマージ戦略には依存しない(第3.4節)。

### 3.5 ChangeEventの自動生成とディレクトリ構造

`ChangeEvent`は、マイルストーン境界で`derived_from`の差分が検出されたFeatureについて自動生成する。変更種別(`change_type`：仕様変更／バグ修正等)のみ、人間がコミットメッセージまたはPRテンプレートで入力する。

**実装状況**：CLI実装の`ChangeEvent`構造体は`change_type: Option<ChangeType>`フィールドを持つ(`event_id` / `feature_id` / `from_milestone` / `to_milestone` / `from_tree_sha` / `to_tree_sha` / `impacted_testcases` / `change_type` / `true_divergences` / `related_events`。後2者の詳細は本節末尾・第3.2節を参照)。`ChangeType`は`SpecChange` / `BugFix` / `Refactor` / `Other`の固定enum(snake_caseでシリアライズ)であり、コミットメッセージ・PRテンプレートからの自動抽出ではなく、`markharness changes compute`実行後に人間が`markharness changes annotate <event_id> --type <spec-change|bug-fix|refactor|other>`を実行して`changes/*.yaml`を書き換える方式で入力する(設計意図通り、計算では埋めない)。`annotate`はevent_idを`changes/`配下の全ファイルから横断的に検索するため、呼び出し側がどのマイルストーン区間のファイルかを事前に知る必要はない。

**related_events(2026-08追記、製品化提案)**：`ChangeEvent`は`related_events: Vec<String>`(他の`event_id`の配列、`#[serde(default)]`で加算的)も持つ。複数のFeatureにまたがる変更が実は同じ論理変更の一部だった、という関連付けを人間が事後的に記録できるフィールドで、`markharness changes annotate <event_id> --related <他のevent_id>...`(複数指定可)で追記する。`ChangeEvent`がFeature単位・自動計算という原子性を保つ(§3.2)ための設計上の選択であり、複合ChangeEventのような自動計算ロジック自体の変更は行わない。

**図4：変更影響の伝播（Change propagation）**

```mermaid
flowchart LR
  CE["ChangeEvent\n(Feature X: milestone n-1 → n)"] --> FX["FEATURE X"]
  FX --> C1["CONDITION A\n(変更された条件)"]
  FX --> C2["CONDITION B\n(影響なし)"]
  C1 --> TC1["TESTCASE 1"]
  C1 --> TC2["TESTCASE 2"]
  C2 --> TC3["TESTCASE 3（影響なし）"]
  TC1 --> R["再確認が必要な\nTestCase集合"]
  TC2 --> R
```

`ChangeEvent`は`FEATURE`の変化を起点に、構造的な生成グラフ(第3.2節(A)：`CONDITION`→`TESTCASE`)を辿ることで、影響を受ける`TESTCASE`集合を特定する。この特定処理自体は静的な生成関係を使うため版履歴を必要としないが、「そもそも`FEATURE`が過去のどの時点からどう変化したか」を検知するには第3.2〜3.4節の版履歴DAGが必要であり、両者は組み合わさって初めて「複数世代にわたる変更影響の特定」(RQ1)を可能にする。

物理ディレクトリ構造は**階層(木)のみを表現し、横断的観点(Axis)はメタデータ＋生成インデックスで表現する**。ファイルシステムは多対多関係を自然に表現できないため、木構造と同じ場所にグラフ構造を無理に押し込まない。

```
repo/
├── knowledge/                  # ソース・オブ・トゥルース(木構造)
│   └── player/                 # REQUIREMENT(requirement.yml、実装で追加された明示階層)
│       ├── requirement.yml
│       └── jump/                # FEATURE(feature.ymlは requirement: player で親を参照)
│           ├── feature.yml
│           └── jump-behavior/
│               ├── behavior.yml
│               ├── ground/
│               │   ├── condition.yml
│               │   └── expected/001-lands-safely.yml
│               ├── air/
│               └── double-jump/
├── axes/                        # 横断的観点の定義(レジストリ)
│   ├── gameplay.yml
│   ├── animation.yml
│   ├── ai.yml
│   └── network.yml
├── generated/                   # 生成物(コミット対象、CIで再生成一致を検証)
│   └── testcases/ground-001.yml # 1 Condition = 1ファイル(実装、UC2/UC3)
├── executions/                  # マイルストーンごとの実行結果
│   └── 2026-08-release/
│       ├── milestone.yml
│       └── results.yml
├── changes/                     # ChangeEventログ(マイルストーン境界で自動生成)
│   └── 2026-08-release.yaml     # 1マイルストーン区間=1ファイル、複数ChangeEventを配列で保持(実装)
└── schema/                      # フォーマット定義(JSON Schema。`markharness init`が既定スキーマ一式を配置、実装済み)

# 注：id解決キャッシュは非コミット化し、.gitignore対象とする(第3.3節)。実装上の配置は
# リポジトリ直下の .markharness-cache/ (旧 generated/id-index.json という案からの変更)。
```

**実装状況**：上記は当初設計図(REQUIREMENTを暗黙化、ChangeEventを日付+スラッグの1イベント1ファイル)からの修正版であり、実際の`markharness init`が作成する構造・`markharness`の各コマンドが読み書きする形式に合わせてある。差分の要点は次の通り。

- `REQUIREMENT`をディレクトリ直下の`requirement.yml`として明示ファイル化し、`FEATURE`はその配下に置く(第3.1節)。
- `changes/`は「1イベント1ファイル」ではなく「1マイルストーン区間1ファイル、複数`ChangeEvent`を配列で保持」する形式(拡張子は`.yaml`)。
- `schema/`は`markharness init`実行時に既定のJSON Schemaファイル一式(`requirement.schema.json` / `feature.schema.json` / `behavior.schema.json` / `condition.schema.json` / `expected_result.schema.json` / `axis.schema.json`)で初期化される(既存ファイルは上書きしないため、プロジェクトごとにスキーマをカスタマイズできる)。`markharness validate`が`knowledge/`・`axes/`配下の全YAMLをこれらのスキーマで構造検証し、加えてJSON Schema単体では表現しにくい相互参照制約(`axis`タグが`axes/*.yml`に登録されているか、`forked_from`が実在するFeature idを指しているか)をRust側のクロスリファレンスチェックとして実装している(第3.6節)。
- `expected_result.schema.json`は`generated_by`(enum: `manual`/`llm`/`auto_combination`、任意)・`verified_by`(`{ human_review: boolean }`、任意)も持てる(製品化提案、論文本文には明記なし)。いずれも省略可能で、`generated_by`の省略は「生成手段が不明」を意味し「手動作成」とは解釈しない。`model`名・`prompt_version`・`confidence_score`のような揮発性の高いメタデータは、「`knowledge/`配下は検証済みの確定知識である」という本スキーマ群の前提と相性が悪いため採用していない。

`feature.yml`のfront matter例(実装に合わせた形)：

```yaml
id: player-jump
requirement: player  # 親REQUIREMENTへの参照(実装で追加)
label: プレイヤージャンプ  # 表示専用。系譜計算には使わない
axis: [gameplay, animation]
forked_from: null # 概念的な派生元がある場合のみ手動記述(例：other-feature)
```

`axis`の命名規則が揺れると横断ビューが破綻するため、`axes/*.yml`に定義されていない値をfront matterで使えないよう、`markharness validate`がスキーマバリデーション＋クロスリファレンスチェックで縛る(実装済み、本節末尾参照)。正規化ルール(どのフィールドをハッシュ計算対象とするか)をスキーマ自体に明文化して固定するかどうかは今後の検証課題とする。

### 3.6 実装状況まとめ

第3章で述べたモデルのうち、CLI実装(`markharness`)で確認できる対応状況を以下にまとめる。詳細な突き合わせは別紙[gap-analysis-mm-folder.md](./gap-analysis-mm-folder.md)を参照(ただし同資料は本節の更新以前の状態を反映したものである点に留意)。

| 分類 | 内容 |
|---|---|
| 実装済み・設計と一致 | 版履歴キーとしてGitオブジェクトのハッシュを使う(ただし単位はblobではなくFeatureディレクトリのtree、3.1節)、TestCaseをknowledge/から分離した派生物として管理、ChangeEventのマイルストーン境界自動計算、id解決キャッシュの非コミット化・内容アドレス方式キー化と自動破棄(3.3節)、idのfeature.yml `id:`フィールドへの統一とディレクトリリネーム耐性(3.3節)、`git notes`によるバックフィル進捗管理(第4章)、`forked_from`フィールド自体の提供、`change_type`フィールドと事後アノテーションコマンド(3.5節)、`related_events`フィールドと`changes annotate --related`(製品化提案、3.5節)、`requirement.yml`の`source`/`related_issues`フィールド(製品化提案、3.1節)、`expected_result.schema.json`の`generated_by`/`verified_by`フィールド(製品化提案、3.5節)、`schema/`のJSON Schemaバリデーションとaxis/forked_from相互参照チェック(3.5節)、`git merge-base`による祖先探索・2親分岐判定(監査用副次コマンドとして、3.2節)、マイルストーン区間内の任意の位置で発生した全マージへの`lineage`判定と`changes compute`の統合(`true_divergences`フィールド、3.2節)、`verify trace`/`verify pending`によるTestExecutionとChangeEventの自動突合・未再検証テストのpending/stale判定(3.7節) |
| 設計から簡略化 | id解決キャッシュの`canonicalization_rule_version`/`id_index_schema_version`は現状固定値で、実際の改訂運用は未検証(3.3節)。id⇔pathの汎用的な独立インデックス層(パスを変えないid変更の追跡等)までは実装していない(3.3節)。`verify trace`/`verify pending`は導入前の既存実行記録には遡及適用せず、`executions/*/results.yml`用のJSON Schemaも未整備(3.7節) |
| 未実装 | 既存TMS(TestRail/Xray等)からのインポータ(UC8) |
| 設計に無い追加要素 | `REQUIREMENT`の`requirement.yml`としての明示ファイル化と`knowledge/<requirement>/<feature>/...`階層(3.1節) |

これらのうち「設計から簡略化」の項目は、RQ1の評価(第5章)が主に必要とする「マイルストーン境界での線形な版履歴追跡」自体には影響しない。`git merge-base`による分岐検出は、マイルストーン区間内の任意の位置で発生した全マージについて`changes compute`の主系譜へ自動反映されるようになったため、複雑なブランチ運用を行う組織でのケーススタディ(第5.2節)でも`lineage`コマンドの補助的併用なしに版履歴の精度を確保できる。

### 3.7 変更検知に基づく再検証トラッキング

第3.5節・図4は`ChangeEvent`から影響`TESTCASE`集合を特定するところまでを扱うが、「その後、実際に再実行されたか」を自動判定する仕組みは当初、第7章(Future Work)相当の未確定領域だった。CLI実装ではこれを`markharness verify trace` / `markharness verify pending`として実装済みであり、本節でその設計を要約する(詳細仕様は別紙[change-event-verification-tracking-spec.md](./change-event-verification-tracking-spec.md)を参照)。

**解決する問い**：現行実装では`executions/<milestone>/results.yml`(`case_id` / `result` / `executor` / `executed_at`)と、`changes/<to_milestone>.yaml`(第3.5節の通り、ファイル名は区間の`to_milestone`のみ)の`impacted_testcases`との突き合わせを人間が目視で行っていた。以下の2つの問いを自動化する。

- **Q1(遡及)**：あるTestExecutionの結果は、Featureのどの変更を反映した状態に対する実行か。
- **Q2(前方)**：ChangeEventで`impacted_testcases`に挙がったTestCaseのうち、まだ再実行されていないものはどれか。

**データモデル拡張**：`TESTEXECUTION`(`executions/<milestone>/results.yml`の各レコード)に`verified_feature_tree_shas`フィールドを追加する。これはそのTestCaseの生成元Featureそれぞれについて、**実行時点のマイルストーンでのFeatureディレクトリ全体のtree SHA**(第3.1節で述べたfeature.yml単体のblobではなく、配下のBehavior/Condition/ExpectedResultを含むディレクトリ全体のGitツリーオブジェクトSHA)を記録するマップである。値は実行結果登録時に`id_index`キャッシュ(第3.3節)から機械的に埋まり、人間が手入力する項目ではない。`ChangeEvent`自体には「再確認済み」フラグを持たせない設計とした。`ChangeEvent`はマイルストーン境界の差分という不変の事実記録であり(第3.4節の設計思想と整合)、「再確認済みか」は`ChangeEvent`と`TESTEXECUTION`という2つの独立した事実系列を都度計算すれば導出できる派生情報だからである。

**判定アルゴリズム**：Q1は、対象レコードの`verified_feature_tree_shas`の各Feature idについて、`changes/`配下の`to_tree_sha`が一致する`ChangeEvent`を検索し、その`event_id`・`from_milestone`・`to_milestone`を「この結果が反映している変更」として返す。Q2は、対象区間の全`ChangeEvent`の`impacted_testcases`を統合した集合から、`to_milestone`以降の`results.yml`で`verified_feature_tree_shas`が一致するレコードが1件でもあるものを「再検証済み」として差し引き、残りを「未再実行」として出力する。さらに、`to_milestone`より後に対象Featureがさらに変更され`to_tree_sha`自体が古くなっている場合は、一律「未実行」とはせず**pending**(まだ一度も実行記録が無い)と**stale**(実行記録が無いまま対象がさらに変更され、古い版への確認がもはや意味を持たない)の2区分に分ける。テスターが「どの版に対して確認すればよいか」を見失わないための区別である。

**ツールインターフェース**：`markharness verify trace <case_id> --milestone <m>`(Q1)、`markharness verify pending [--from <m1> --to <m2>]`(Q2)の2コマンドを提供する。いずれも読み取り専用で、既存の`verified_feature_tree_shas`・`changes/*.yaml`・`.markharness-cache/`のみを入力とする。CI組み込み用に`--fail-on-pending`オプションを持ち、`pending`が1件でもあれば非ゼロ終了コードを返すことで、変更影響テストの再確認漏れをリリースゲートで機械的にブロックできる。

**具体例**：`changes/test2.yaml`に`todo-edit`Featureの`from_tree_sha: null` / `to_tree_sha: 4f2c9a1e...`という`ChangeEvent`があり、`executions/test2/results.yml`の対応レコードが`verified_feature_tree_shas: {todo-edit: 4f2c9a1e...}`を持つ場合、両者の`tree_sha`が一致するため`markharness verify pending --from test1 --to test2`は当該TestCaseを pending 扱いにせず「再検証済み」と判定する。

**実装状況・留意事項**：本仕様導入前の既存実行記録(`verified_feature_tree_shas`を持たないもの)には遡及適用せず、判定対象外(「不明」扱い)とする。この捕捉はFeatureディレクトリ全体のtree SHA比較(第3.1節の`id_cache::resolve_feature_versions`)によって初めて成立しており、feature.yml単体のblob SHAを比較する実装では、Condition/ExpectedResultの変更を見逃すため成立しない。また、Feature自体は変わらずAxisレジストリ(`axes/*.yml`)側だけが変わるケースは追跡対象外であり、`executions/*/results.yml`用のJSON Schema(`markharness validate`対象への追加)も未実装のままである(いずれもFuture Work、第7章)。

---

## 4. Implementation：既存リポジトリへの移行アーキテクチャ

既存の大規模リポジトリに本モデルを導入する際、全履歴を遡及的に処理する「バックフィル」のコストが導入障壁になりうる。以下のアーキテクチャで対応する。

### 4.1 バックフィル対象の縮小

版履歴DAGはマイルストーン境界でのみ確定する設計(第3.4節)であるため、バックフィルも**過去のマイルストーンタグが付いたコミットのみ**を対象にすればよい。月次〜四半期リリースで数年分でも数十〜数百件程度であり、「数万ファイル×全履歴」ではなく「数万ファイル×過去のリリース数」という扱いやすい規模に縮小される。

### 4.2 非同期バックグラウンド処理

バックフィルを、開発を止める同期的な一括処理ではなく、優先度の低いバックグラウンドジョブとして実装する。直近のマイルストーンから優先的に処理する。

**実装状況**：CLI実装の`markharness backfill run`は、直近のマイルストーンから処理し中断・再開可能という性質(第4.3節のGit notesにより実現)は満たすが、コマンド自体は「1回呼び出すと未処理ペアを1パス処理して終了する」同期的な処理であり、常駐のバックグラウンドデーモンではない。「開発を止めない」という設計意図は、このコマンドをCIのスケジュール実行等から繰り返し呼び出す運用で実現する想定になっている。

### 4.3 Git notesによる進捗管理

各マイルストーンタグに対応するコミットに対し、「このマイルストーンの系譜計算は完了している」という進捗情報を`git notes`(通常のコミット履歴を書き換えず、別名前空間で任意のメタデータをコミットに付与できるGitの機能)として記録する。バックグラウンドジョブが中断・再開しても重複処理しない。Git notesは通常のブランチマージの対象外であるため、この進捗記録自体がマージコンフリクトを起こすこともない。

### 4.4 遅延(オンデマンド)計算による段階的な価値提供

バックフィルが完了していないマイルストーン区間について問い合わせがあった場合、その場で計算しキャッシュする。これにより、バックフィルが全て完了する前からツールが部分的に価値を提供でき、直近のマイルストーンから使い始め古い履歴は使われた時点で順次埋まる、段階的な導入が可能になる。

### 4.5 ツール構成

- スキーマ定義：JSON Schemaで`knowledge/`配下のYAMLフォーマットを固定。**実装済み**(`schema/*.schema.json`、`markharness init`が既定一式を配置、`markharness validate`が検証。正規化ルール自体のスキーマへの明文化は今後の課題、第3.6節)。
- 実装上の利便機能：現在のHEADと基準点の差分を、構造的な生成グラフに照らして影響TestCaseを表示するCLIコマンド(版履歴DAGは使わない)。
- id解決キャッシュ：非コミット、キャッシュキーと破棄条件は第3.3節。**実装済み**(内容アドレス方式のキャッシュキー・読み込み時の自動破棄、第3.3節)。
- 版履歴計算ツール(核心的貢献)：マイルストーンタグ間でid解決経由の各idのtree SHAを比較し`derived_from`を計算。**実装済み**(`markharness changes compute`)。
- バックフィルワーカー：第4.1〜4.4節のアーキテクチャに基づく非同期バックグラウンド処理。**実装済み**(`markharness backfill run`、ただし4.2節注記の通り単発呼び出し型)。
- 詳細系譜ツール(監査用、副次機能)：`git merge-base`を用いたコミット単位の系譜再構築。**実装済み**(`markharness changes lineage --commit <merge-sha>`。ただし判定結果は`changes/*.yaml`へは永続化されない、第3.2節・第3.6節)。
- テストケース生成ツール：`Feature + Condition`から`TestCase`を生成し、再生成結果と現在のファイルの一致をCIで検証。**実装済み**(`markharness generate` / `markharness verify`)。
- 既存ツールからのインポータ：TestRail / Xray / TestLink のエクスポート形式から本フォーマットへの変換器。**未実装**(第3.6節)。

実装の詳細は本リポジトリ(`markharness`、Rust実装)を参照。CLIの全コマンドは`docs/cli-manual.md`にまとめている。

---

## 5. Empirical Evaluation Plan(未実施)

本章は被験者実験の**計画**であり、本ドラフト時点で実験は実施していない。以下は「どう検証するか」の設計であって、「検証した結果」ではない。実施状況は第8章 Conclusion を参照。

### 5.1 目的

RQ1「明示的な版履歴を持つモデルは、対象組織の現状の複合的な運用と比較して、特に複数世代にわたる変更影響識別タスクにおいて正答率・所要時間を改善するか」を検証することを目的として、以下の評価計画を設計した。

**図5：評価フロー全体像**

```mermaid
flowchart TB
  S1["対象プロジェクトの実Git履歴からChangeEventを抽出"] --> S2["タスクの層別化\n層α(浅い変更) / 層β(深い変更)（第5.3節、事前登録）"]
  S2 --> S3["正解データ構築\nco-change抽出 → ノイズ除去 → 専門家による軽量確認（第5.4節）"]
  S3 --> S4["被験者割当\n実験群(提案ツール) / 対照群(現状運用)（第5.2節）"]
  S4 --> S5["タスク実施\n正答率・所要時間・NASA-TLX等を計測（第5.5節）"]
  S5 --> S6["層βの正答率を主指標として統計検定（第5.3節）"]
```

以下、各段階の詳細を述べる。

### 5.2 対照群：現状運用への統合

自作の疑似TMSや、実TestRail/素のGit運用への人工的な分割は、実務者が複数ツールを併用する現実を反映しない。事前調査により、対象組織(協力プロジェクト)のテスターに「変更影響を調べる際に実際に何を使っているか」を確認し、実際に日常的に使っているツールの組み合わせ(例：TestRail＋Jira課題検索＋`git log`/`git blame`)を対照群とする。研究者側が恣意的に「TestRailだけ」「Gitだけ」という条件を設定しない。実験群は本モデルの提案ツールとする。対照群を1本に統合することで、統計的検定力を単一の主比較に集中できる。

### 5.3 タスクの層別化(事前登録)

「使い慣れた実運用」対「導入直後の提案ツール」という比較は、習熟度の交絡がタスク速度に強く影響する。この交絡を無視せず、タスクを2層に分けて評価し、**この層別化は実験開始前に事前登録**する。

- **層α(浅い変更)**：直近1リリース内の変更。対照群の運用でも原理的に対応可能な範囲。速度面で対照群が有利になることをあらかじめ想定される結果として明記する。
- **層β(深い変更)**：複数世代前からの派生・複数リリースをまたぐ変更。対照群の運用は版間の派生関係を体系的に保持していないため、習熟度に関係なく正答に必要な情報自体が欠落している。本研究の核心的主張が直接効くのはこの層である。

**主指標**は層βにおける正答率(適合率・再現率)とする。速度は補助指標とし、習熟度の交絡があることを解釈時に明記する。

### 5.4 正解データの構築方法：co-changeノイズの除去

層βの正解データを、既存運用の当事者だった人間の記憶による聞き取りだけに頼るのは自己矛盾に近い(本当に複雑で既存運用では対応できない変更であれば、人間の記憶も同様に不正確でありうる)。正解データの構築は、当時の成果物に基づく機械的な再構成を優先する。

**第一優先(成果物ベース)**：対象の仕様変更が行われた実際のコミット・PRにおいて、同じコミット/PRで変更されたテストファイル(co-change信号)、CIのテスト実行ログ、Issue管理システム上のテストケースID紐付け記録を機械的に抽出する。

**co-changeノイズの除去基準**：co-change信号は無条件には信頼できず、以下のノイズ除去を行う。

1. **無関係な同時変更(束ねられたコミット)**：コミット/PRの変更行数・変更ファイル数が対象プロジェクトの中央値の3倍を超える等、異常に大きい場合は候補から除外するか個別精査に回す。コミットメッセージ・PR説明文に複数の意図が記載されている場合も精査対象とする。
2. **機械的な変更(意味を持たない同時変更)**：diffが空白・改行のみ、既知の自動生成パターン(スナップショット更新等)に一致する場合、または同一コミットで数十〜数百ファイルが同時変更されている(一括リネーム・一括フォーマットの兆候)場合は除外する。
3. **意味的な無関連**：変更されたテストファイルが、モデル上の構造的な生成関係(第3.2節(A)：`FEATURE`/`CONDITION`→`TESTCASE`)と一致するかを確認し、一致する場合のみ強い候補として採用する。過去のコミット全体で出現頻度が極端に高いテストファイル(スモークテスト等)は特異性が低く重み付けを下げるか除外する。

**二段階の構築プロセス**：(1)上記基準による機械的フィルタリングで候補セットを作成し、(2)独立した複数の専門家(最低2名)による軽量な確認(Yes/No)を行い、評価者間一致度(Cohen's kappa等)を報告する。意見が割れた候補は正解データから除外するか部分点として扱う。

**第二優先(成果物が得られない場合)**：聞き取りに頼らざるを得ない場合も、単独担当者ではなく独立した複数の専門家に個別判断させ、一致度を報告する。層βのタスクは、可能な限り成果物ベースで正解データが再構成できる変更を優先的に選定し、聞き取りベースの割合が高い場合は結果の解釈に留保を付ける。

### 5.5 タスク・指標・サンプルサイズ

被験者に、対象プロジェクトの実際の過去の変更を提示し、影響を受けるTestCase群を特定させる。主指標は層βの正答率(適合率・再現率)。速度・被験者の主観的自信度(NASA-TLX等)は補助指標。サンプルサイズは統計的検定に耐えるよう群あたり15〜30名を目安とし、被験者の経験年数・対象プロジェクトへの熟知度・現状運用ツールへの習熟度を共変量として記録する。

### 5.6 想定される脅威(Threats to Validity)

- **内的妥当性**：課題文の設計のカウンターバランス、両群への事前練習セッション。
- **構成概念妥当性**：「深い変更」の定義を事前に固定。正解データの構築方法(成果物ベース/聞き取りベース)の内訳を明記し、聞き取りベースの割合が高い場合は結果の留保を付ける。co-changeノイズ除去基準(変更ファイル数・出現頻度の閾値)は対象プロジェクトの規模・開発文化に依存し、他プロジェクトへの直接移植には調整が必要。
- **外的妥当性**：単一組織・単一ドメインのケーススタディに留まる場合の一般化可能性の限界。対照群の「現状運用」は組織によって異なるため、他組織での追試では対照群の構成が変わりうる。

---

## 6. Threats to Validity(全体)

- 提案モデルの実装(ツール)が被験者実験の結果に影響する可能性(ツールの使いやすさとモデルそのものの有効性を混同しないよう、UIの簡素化・操作説明の標準化を行う)。
- id解決キャッシュを非コミット化したことで、CI環境が変わるたびに再計算コストが発生する可能性(ビルドキャッシュの永続化戦略に依存)。
- バックフィルアーキテクチャ(第4章)の性能は、実際の大規模リポジトリでの検証(ケーススタディ)がまだない。新規構築するデータセットでは移行コストが顕在化しない可能性があり、実際の導入コストを過小評価するリスクがある。

---

## 7. Future Work

- 実装上の利便機能(構造的生成グラフに基づくリアルタイム照会、第3.2節(A))の開発者体験・生産性への効果の検証。
- バックフィルアーキテクチャ(第4章)を実際の大規模リポジトリに適用した場合の性能実測。
- id解決キャッシュのキー設計(第3.3節)・co-changeノイズ除去基準(第5.4節)を、実装・データ収集を通じて検証・調整すること自体を今後の実証課題とする(`canonicalization_rule_version`/`id_index_schema_version`は現状固定値で、実際の改訂運用を経た検証がまだない)。
- id⇔pathの汎用的な独立インデックス層の実装(パスを変えないid変更の追跡等、第3.3節で現状は「id=feature.ymlのid:フィールド」への統一に留まると整理した項目)。
- 既存TMS(TestRail/Xray等)からのインポータの実装(第3.6節で未実装と整理した項目)。
- LLMによる文脈供給・Markdown手順書の自動生成・更新への応用可能性(検討経緯・不採用理由は付録A参照。本研究の評価対象外)。
- 構造からのテストケース自動生成の網羅率評価、Git粒度分割によるレビュー性向上の検証(検討まとめ第4章の案2・3)。
- 他ドメイン・他組織での追試による一般化可能性の検証。
- `generated_by`/`verified_by`(第3.5節)を読む将来のCIゲート(例：`generated_by: llm`かつ`verified_by`未設定の`ExpectedResult`が存在する場合に`markharness verify`が警告する)は未実装。現状は離散的な事実情報を記録するだけで、それを消費するロジックは本研究のスコープ外としている。

---

## 8. Conclusion

本研究は、Gitのコンテンツアドレス(tree SHA)・非コミットのid解決キャッシュ・コミットグラフの祖先探索(`git merge-base`)を組み合わせ、ブランチ運用に依存せずマイルストーン境界でテスト知識の版履歴(`derived_from`)を導出するモデルを設計した(第3章)。既存TMS・素朴なGit運用のいずれも、複数世代にわたるテスト知識の派生関係を第一級のクエリ対象として扱わない(第2章・第2.4節)という構造的な欠落に対し、本モデルはGit自身のオブジェクトモデルを土台にした版履歴DAGで応える設計提案である。

この設計は`markharness`(Rust実装、本リポジトリ)としてリファレンス実装され、`changes compute`によるマイルストーン境界の版履歴自動計算(区間内の任意の位置で発生した全マージへの`lineage`統合を含む、第3.2節「統合(2026-08追記)」)、`changes lineage`による`git merge-base`ベースの分岐監査、`verify trace`/`verify pending`による実行結果との自動突合(第3.7節)を含む中核機能が動作することを確認した。第3.6節にまとめた通り、設計から意図的に簡略化した箇所(id解決キャッシュのバージョン改訂運用が未検証等)と、未実装のまま残した箇所(既存TMSからのインポータ)がある。

**本研究の現時点での性質**：本ドラフトは、RQ1(「明示的な版履歴を持つモデルは、複数世代にわたる変更影響識別タスクにおいて既存の複合運用より正答率・所要時間を改善するか」)を検証する**設計提案とリファレンス実装のレポート**であり、第5章に計画した被験者実験による実証的評価は本ドラフト時点では未実施である。したがって、RQ1に対する肯定的な結論を本ドラフトでは主張しない。第3章で述べたモデル構造(版履歴の第一級化)が、既存運用にはない情報(過去世代からの派生関係)をテスターに提供しうるという設計上の期待は成り立つが、これが実際の正答率・所要時間の改善に結びつくかどうかは、第5章の評価計画に沿った被験者実験を経て初めて判断できる。

**Future Workとしての実証**：RQ1の被験者実験による検証は、本研究の直接の続編として位置づける(第7章)。第5章の評価計画(タスク層別化・正解データ構築・対照群の統合)は実験開始前の事前登録内容として確定させてあり、次段階はこの計画に沿った実験の実施と結果の報告である。あわせて、id解決キャッシュのバージョン改訂運用の実証(第3.3節)、大規模リポジトリでのバックフィル性能実測(第4章)も、モデル自体の評価とは独立に取り組むべき実装課題として残っている(`changes lineage`の主系譜統合は第3.2節「統合(2026-08追記)」の通り実装済みのため、本項からは除外した)。

---

## 付録A：検討経緯ログ(論文スコープ外、意思決定の記録)

### A.1 LLM活用へのピボット案(不採用)

「人間向けツール」ではなく「LLMに仕様変更を理解させ、手動テスト手順書を自動生成・更新させるAI専用知識グラフモデル」への全面ピボットを検討したが、以下の理由で不採用とした。

1. クエリ速度・使いやすさへの懸念は、利用者がテスターからLLMに変わっても本質的には残る(対象が移るだけ)。
2. 「LLM×知識グラフ×テスト」は既に研究例が多い領域であり、「AI専用」という打ち出し方だけでは新規性の主張として弱い。差別化ポイントは`derived_from`(版履歴)と`ChangeEvent`の影響伝播であり、これはLLMを前提にせずモデルに含まれている。
3. LLM生成精度評価を本評価に加えると、統計的に信頼できるサンプル数を被験者実験と並行して確保するのが非現実的であり、格下げしてもパイロット的な位置づけでは「なぜ論文に必要か」という弱点を作るだけと判断し、論文本体から完全撤去した。

### A.2 Markdown手順書の保護領域(override)方式とその限界

LLM生成の手順書にテスターが直接手を加えた場合の運用として保護領域方式を検討したが、テキストの上書き事故は防げても、前提条件が大きく変わった際の意味的な陳腐化は防げない。これはテキストマージの限界であり原理的に解決できず、LLM角度がFuture Workとなったため中心設計から外した。

### A.3 独自ハッシュ・独自インデックスの再発明(修正済み)

系譜キーの識別子として、独自にcontent_hashを計算し独立したインデックスファイルに記録する設計を検討したが、これはGitが既に持つコンテンツアドレス方式(blob SHA)の再発明だった。Gitのハッシュ機構を直接使う設計に修正した(第3.2節)。また、id解決の対応をコミット対象の単一ファイルとして持つ設計も、並行開発時のマージコンフリクトを招くため、Gitの`commit-graph`と同じ設計思想の非コミットキャッシュに修正した(第3.3節)。

### A.4 研究プログラムとしてのアクション一覧

1. スコープ確定：案1(構造表現、タスクベースのRQ1)を単独の中心テーマとする。案2・案3は将来課題(第7章)。
2. 関連研究の追加調査：CTM・Model-Based Testing、LLM+知識グラフによるテスト生成・トレーサビリティ研究(将来課題用)の一次文献にあたり差分を明文化する。
3. フォーマット仕様の先行設計：JSON Schemaを固定・バージョニングする。
4. ハッシュ計算・正規化ルールの実装：対象フィールドと正規化方法を先に固定する。
5. スキーマバリデーション実装：`axis`タグ・`forked_from`参照先の整合性をCIで検証する。
6. 既存ツールからのインポータ設計：TestRail / Xray / TestLink のエクスポート形式からの変換器を用意する。
7. ケーススタディ対象の選定：ゲーム・Web・業務システムのうち、実データ(または模擬データ)で評価する対象を決める。
8. 被験者実験の設計：層βの評価のため、統計的検定に耐えるNを確保できる実験計画を立てる。
9. id解決キャッシュ・系譜計算ツールの実装：第3.2〜3.3節の仕様に基づき実装する。
10. バックフィルワーカーの実装：第4章のアーキテクチャに基づき実装する。
11. co-changeノイズ除去スクリプトの実装：第5.4節の除去基準に基づき実装する。

### A.5 タイトル・打ち出し方への示唆

「AI-Native Test as Code」のような抽象的な打ち出し方は査読での期待値と実際の貢献のギャップを生みやすい。差別化ポイント(バージョン追跡・変更影響伝播)を明示したタイトル、例えば本資料冒頭のタイトルの方が、査読での期待値ギャップが小さい。

---

## 参考文献

- Cleland-Huang, J. et al. (2011). Agile Traceability Information Model.
- Software Test Data Management Based on Knowledge Graph. https://www.informatica.si/index.php/informatica/article/download/6416/3168
- Model management to support systems engineering workflows using ontology-based knowledge graphs. https://arxiv.org/html/2512.09596v1
- UOOR: Seamless and Traceable Requirements. https://arxiv.org/pdf/2502.18617
- Trust-Aware Multi-Agent Traceability. https://arxiv.org/pdf/2606.17203
- https://qtrl.ai/blog/testrail-vs-zephyr
- https://qaskills.sh/blog/test-management-tools-comparison-2026
- https://qaskills.sh/blog/best-test-management-tools-beyond-testrail-2026
- https://getautonoma.com/blog/opensource-alternative-testrail
- https://getautonoma.com/blog/testrail-vs-xray
- https://qtrl.ai/blog/testlink-vs-testrail
- https://www.practitest.com/testrail-alternatives/
- https://www.practitest.com/resource-center/blog/beyond-hierarchical-structures/

---

## 変更履歴(Changelog)

**運用ルール**：本節は2026-08-11以降、本資料に実質的な変更(記述内容の追加・修正・削除)を加えるたびに追記する。参照リンクの張り替えやファイル名の統一など、内容に実質的な変更を伴わない編集は追記しない(詳細は`CLAUDE.md`の運用ルールを参照)。2026-08-11より前の履歴は`git log --follow`で本ファイルのコミット履歴を辿れるため、以下では簡潔な要約のみ記載する。

- **2026-08-11(2)**：レビューで発覚した記述の食い違いを修正。§8 Conclusionに残っていた「`lineage`の判定結果が主系譜に自動反映されない」「`changes lineage`の主系譜統合...が実装課題として残っている」という2箇所の記述(§3.2「統合(2026-08追記)」で既に解消済みの制約を指したまま更新されていなかった)を実態に合わせて修正。§3.5のChangeEventフィールド列挙に`true_divergences`/`related_events`を追加。§3.7の`changes/<from>-<to>.yaml`という誤った命名例を実装通りの`changes/<to_milestone>.yaml`に修正(§3.5の命名規則と整合させた、今回の一連の変更とは無関係の既存の誤り)。
- **2026-08-11**：改善プロンプト項目2(`changes compute`のlineage統合をマイルストーン区間内の全マージへ一般化、`true_divergences`フィールドへ改名)・項目3(分岐・マージ検証シナリオの実地検証、§8関連)・項目7(`ChangeEvent.related_events`追加)・項目8(`Requirement.source`/`related_issues`追加)・項目9(`ExpectedResult.generated_by`/`verified_by`追加)を反映。§3.2・§3.5・§3.6・§6・§7を更新。
- **2026-08-10**：RQ1未実証への対応(論文の位置づけを「設計提案＋リファレンス実装のレポート」に修正、第8章Conclusion整備)、`verify trace`/`verify pending`を新設§3.7として反映、`changes lineage`の部分統合(`to_milestone`が直接マージコミットの場合のみ)を反映、ファイル名を`統合版V2.md`から`統合版.md`へ正式化(内容変更なし)、未実装だった5項目(idパス非依存化・キャッシュキー内容アドレス化・`change_type`・schema検証・merge-base系譜監査)の実装を反映。
- **2026-08-09**：論文を当時のmarkharness実装の状態に合わせて修正。
- **2026-08-07以前**：初版作成(検討経緯・v1〜v10ドラフトの統合)。
- https://medium.com/@nikhilmartinez/gitlab-test-case-management-5-tools-compared-e0cb6ae9a416
