# 0035: `knowledge remove`が残す空ディレクトリを削除する

## ステータス

Accepted(2026-09-29決定)。未実装。`checklist-knowledge-remove-empty-dirs.md`を参照。

## 背景

[0034](0034-knowledge-remove-command.md)は`fs_safety::remove_file_no_follow`を使ってKnowledge YAMLファイル(`requirement.yml`、`feature.yml`、`behavior.yml`、`scenario.yml`)を削除するが、これは指定したファイルのみを取り除く。そのファイルを収めていたディレクトリは、削除(またはその連鎖)によって中身が何も残らなくなった場合でも一切削除されない。葉のScenarioを`knowledge remove`したり、Behavior・Feature部分木全体を空にする連鎖削除を行ったりすると、`.markharness/knowledge/`配下にどんどん深い空ディレクトリの痕跡が残り、それをコードベースの他のどこも掃除しない: `identity::knowledge_walk::list_entities`と`generate::load_knowledge_snapshot`はいずれもマーカーファイル(`requirement.yml`/`feature.yml`/`behavior.yml`/`scenario.yml`)の有無しか見ないため、空ディレクトリは他のどのコマンドからも見えないが、ツリーを見る人間やGUIには実際に残骸として見えてしまう。

`markharness init`(`src/init.rs`)は`.markharness/`直下にちょうど6つのディレクトリ(`knowledge`、`axes`、`generated`、`executions`、`changes`、`schema`)を作成し、そのうち空になりうるもの(`ensure_default_schemas`が常に中身を作る`schema`を除く)には`.gitkeep`を書き込んでGitが追跡できるようにする。`.markharness/knowledge/requirements/`や`.markharness/knowledge/features/`は作成しない——これらは、その下に最初のRequirement/Featureが書き込まれた時点で遅延的に作られる。空ディレクトリを検出・削除するコードは現在プロジェクト内のどこにも存在しない。

## 決定

### 1. その`knowledge remove`実行が触ったディレクトリのみを掃除する

掃除は、`knowledge remove`の各実行がファイル削除と逆参照書き換えを完了した直後に、自動的に行われる。別途`cleanup`/`prune`コマンドは設けず、有効/無効を切り替えるフラグも設けない。`.markharness/knowledge/`全体を対象に、他の原因(手動`rm`、将来のGUI、本ADR以前の中断された操作等)で残った空ディレクトリまで含めて一般的に掃引することは範囲外とする。現状、`knowledge remove`自身以外にそのようなディレクトリを生み出すものは存在しないため、掃引を設計する具体的な根拠がない([CLAUDE.md](../../../CLAUDE.md)、および[0021](0021-identity-retire-simplification.md)がidentityの退役に対して取ったのと同じ理由によるYAGNI)。

### 2. 完全に空である場合のみ掃除対象とする

「空」とは、中に何もエントリが無いことを指す——まだ空でない他のサブディレクトリすら無いことを意味する。サブディレクトリ(空かどうかを問わず)がまだ残っているディレクトリは、この段階では手を付けない。そのサブディレクトリ自身が既に削除されて初めて、対象候補になる。これにより規則は単純(`read_dir`を1回確認するだけで、「この部分木全体が不活性かどうか」という再帰的な計算は不要)かつ正しい: それ自身が削除可能なエントリは、いずれその番が来たときに削除される。

### 3. 削除は上位方向へ連鎖し、固定された保護ルート群で停止する

あるファイルを削除した後、その親ディレクトリを確認する。そのディレクトリが完全に空になっていれば削除し、続けてその親ディレクトリを確認する——これを、空でないディレクトリに達するか、保護ルートに達するまで繰り返す。保護ルートは、完全に空になっても決して削除しない:

- `.markharness/knowledge/`自体(`init`が作成する)。
- `.markharness/knowledge/requirements/`と`.markharness/knowledge/features/`——2つのコレクションルート。`init`はどちらも作らない(遅延的に生成される)が、両方とも通常のコンテンツディレクトリではなく、Knowledgeツリーの恒久的な構造上の備品として扱う。例えば最後のRequirementを削除した後に`requirements/`を削除してしまうと、次の`knowledge reconcile`や`knowledge remove`がそれをゼロから再作成することになり——`init`が6つのトップレベルディレクトリに使っている`.gitkeep`のような仕組みが無い以上——最後の削除から次の作成までの間、Gitは空の`requirements/`を一切追跡しなくなってしまう。両ルートを恒久扱いとすることで、この揺れ動きを避ける。

個々の要素自身のディレクトリ——`requirements/<id>/`、`features/<feature_id>/`、`features/<feature_id>/<behavior_id>/`、`features/<feature_id>/<behavior_id>/<scenario_id>/`——は保護されず、空になれば削除される。これこそが削除後の実際の掃除であり、例えばFeatureを丸ごと削除すれば、その`feature.yml`と配下のすべてのBehavior/Scenarioディレクトリが消えた時点で、`features/<feature_id>/`の痕跡が一切残らないことが期待される。

保護ルートの判定は、`.gitkeep`の有無ではなく、`project_root::MARKHARNESS_DIR`から組み立てた固定のパスリストによって行う。`.gitkeep`は手動で削除され得るものであり、それがあったディレクトリが構造上の役割を失ったことを意味するとは限らないため、この判定基準として使うと、まさに信頼性が求められる場面でもろい判定になってしまう。

### 4. 削除したディレクトリはすべて報告する

`RemoveOutcome`に`removed_directories: Vec<String>`フィールド(`deleted`/`detached`のパスと同様、root相対・forward-slash正規化)を追加し、その実行で実際に削除したディレクトリを、深い方から順にすべて格納する。`--json`は`deleted`/`detached`と並べてこれを含み、human-readable側の出力も1件ずつ列挙する。

### 5. この掃除ステップはベストエフォートであり、crash-recoverableなバッチには含めない

[0034](0034-knowledge-remove-command.md)§6は、ファイル削除と逆参照書き換えを`identity::recovery`のcrash-recoverableなステージングプロトコルの一部とし、通常実行時とクラッシュリカバリ時の両方で`feature_ops::roll_forward`が同一に再生する。本決定では、このプロトコルをディレクトリ掃除にまで意図的に拡張しない: 掃除は、recoverableなバッチが既にコミット・ロールフォワードされた後の、`knowledge_remove::remove_element`内の単純なステップとして実行し、「掃除がまだ保留中である」ことを示す永続的な記録も、後続コマンドの起動時リカバリスキャンでの再生も行わない。

これが安全なのは、残された空ディレクトリがデータを一切持たず、コードベース内のどの読み手からも見えない(§背景)ためである。recoverableなバッチの完了とこのステップの実行との間でクラッシュが起きた場合、最悪でも、同じ部分木に触れる後続の`knowledge remove`が副次的に掃除してしまう可能性が高い空ディレクトリが残るか、人間が手で(何の影響もなく)削除できる空ディレクトリが残るだけである。recoverableなプロトコルをこのステップまで拡張することは、実質的なコストを伴う(新しいpayloadの形、あるいは`feature_ops::roll_forward`——既に他の2つのADRのpayloadバリアントを1つの関数に通している——の中で`deletes`から掃除対象を導出する)割に、その不在を誰も観測し得ない保証を買うだけである。

## 不変条件

- `.markharness/knowledge/`、`.markharness/knowledge/requirements/`、`.markharness/knowledge/features/`は、どれだけ空になっても`knowledge remove`によって削除されない。
- この掃除によってディレクトリが削除されるのは、確認した瞬間にエントリが一つも無い場合に限る。
- `knowledge remove`がこの方法で削除したディレクトリは、すべてその実行結果に報告される。

## 影響

- `src/fs_safety.rs`: 新規`remove_dir_if_empty_no_follow`プリミティブ(「空の場合のみ削除」プリミティブは現状存在せず、モジュールが提供するのは無条件の単一ファイル削除と無条件再帰的ディレクトリ削除のみ)。
- `src/knowledge_remove.rs`: `remove_element`内に新規の上位方向への掃除ステップ、`RemoveOutcome`への新規`removed_directories`フィールド。
- `src/cli.rs`: `report_remove_outcome`がhuman/`--json`両方の出力で`removed_directories`を報告する。

## 検討し、採用しなかった選択肢

- **`axes prune`のreport-onlyデフォルト+`--delete`という形を踏襲した、一般的な`knowledge cleanup`/`prune`コマンド**: 今回は不採用。現状`knowledge remove`自身以外に空ディレクトリを残すものは無く、一般的な掃引を必要とする具体的なケースが、本ADRの自動・限定的な掃除で既にカバーされていない部分は無い(YAGNI)。空ディレクトリの具体的な発生源(将来のGUIや手動でのファイルシステム編集等)が現れたら再検討する。
- **その実行が触ったディレクトリだけでなく、`.markharness/knowledge/`ツリー全体を掃引する**: 不採用。具体的な問題より範囲が広く、今のところ他に空ディレクトリの残骸を生むものが無い以上、遅くなるだけで利点がない。
- **空に見えないディレクトリにも再帰的に入り込み、すべての子孫が削除可能かどうかを確認して、不活性な部分木をまとめて一度に削除する**: 不要な複雑さとして不採用。上位方向への1階層ずつの走査で、各階層の番が来るたびに正しく同じ終着状態に達する。
- **固定パスリストではなく、`.gitkeep`の存在を「このディレクトリは保護されている」という合図として扱う**: 不採用。`.gitkeep`は手で削除され得るし、本ADRで保護したいディレクトリ(`init`が`.gitkeep`を一切与えない`requirements/`/`features/`)に存在しないこともある。信頼性そのものが目的のチェックの境界として使うには不確実すぎる。
- **crash-recoverableなバッチをディレクトリ掃除にまで拡張する**: 不採用。決定§5を参照。即座に削除されたディレクトリと、クラッシュにより一時的に残ったディレクトリの違いを下流の誰も観測できないため、余分な仕組みを足しても観測可能な保証は何も得られない。
