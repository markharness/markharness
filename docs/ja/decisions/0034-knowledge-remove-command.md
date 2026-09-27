# 0034: Knowledge要素を物理削除する`knowledge remove`

## ステータス

Accepted(2026-09-27決定、2026-09-27実装完了)。`knowledge remove`サブコマンド本体、`identity::recovery`/`feature_ops::roll_forward`のクラッシュ耐性のある削除拡張、`generate`が追加実装なしで既に対応することを示す回帰テスト、およびパストラバーサル/symlink祖先に対する回帰テストをすべて満たした。詳細は`checklist-knowledge-remove.md`を参照。

## 背景

[0027](0027-declarative-knowledge-reconciliation.md)§4は削除を意図的に先送りした。`knowledge reconcile`は削除を伴わない`merge`のみをサポートし、`exact`モードや`--delete`/`--allow-retire`フラグは「誤ったスコープやAIの見落としが破壊的な結果を招く」ことと「初期版には、そのリスクを正当化する具体的な削除要件がない」ことを理由に未実装のまま残された。

その具体的要件が今、存在する。作者は誤って作成した、または不要になったRequirement・Feature・Behavior・Scenarioを削除する必要がある(そして[0017](0017-scenario-case-revision-and-execution-evidence.md)§3が1 Scenario = 1 TestCaseと定めているため、これはTestCaseを削除する手段でもある)。基盤となる意味論は[0021](0021-identity-retire-simplification.md)がすでに決着させている——「FeatureまたはTestCaseが`knowledge/`から削除されると、それは単にKnowledgeツリーから消える。UID再利用の禁止や専用の`retired`状態遷移は追跡しない」——ため、本ADRはretire/tombstoneモデルを新たに考案する必要はなく、削除をどう起動し、どの範囲に及ぼし、どうクラッシュ安全にするかだけを決める。

Knowledgeツリーには削除時の振る舞いが異なる2種類の参照形状がある。

- **任意の多対多参照** — `Feature.requirement_uids`と`Scenario.requirement_uids`([0031](0031-scenario-level-requirement-contribution.md)) — は値コレクション([0027](0027-declarative-knowledge-reconciliation.md)§5)であり、削除されたUIDを単に省略できる。
- **必須の単一親参照** — `Behavior.feature`と`Scenario.behavior`(`src/knowledge.rs`) — は必須の`String`フィールドで、「親なし」を表現できる状態が存在しない。親の削除がこれらを未解決のまま残すことは許されない。

RequirementとFeatureの`id`はプロジェクト全体で一意(`.markharness/knowledge/requirements/<id>/`、`.markharness/knowledge/features/<id>/`)だが、BehaviorとScenarioの`id`は親の配下でのみ一意(`features/<feature_id>/<behavior_id>/`、`features/<feature_id>/<behavior_id>/<scenario_id>/`)であり、異なる親の下に同じslugが正当に存在し得る。

既存の複数ファイルにわたるKnowledge変更([0027](0027-declarative-knowledge-reconciliation.md))は作業ツリーへ直接書き込まれない。`identity::recovery`のステージングプロトコル(`begin_batch_with_payload` → `commit_batch` → `feature_ops::roll_forward` → `finish`)を経由するため、1つのファイルを書いてから別のファイルを書くまでの間でクラッシュしても、変更前または変更後の状態のどちらかに必ず収束し、混在状態にはならない。`identity::recovery::PendingKnowledgeFile`は現在「このファイルの全内容を書く」ことしか表現できず、「このファイルはもう存在しない」ことを表現する手段がない。

## 決定

### 1. `markharness knowledge remove <type> <key>`を独立したサブコマンドとして追加する

```text
markharness knowledge remove <requirement|feature|behavior|scenario> <key>
    [--feature <id>] [--behavior <id>] [--dir <path>] [--json]
```

これは`Reconcile`と並ぶ新規`KnowledgeCommand::Remove`バリアント(`src/cli.rs`)であり、`reconcile`の`exact`/`--delete`モードではない。単一要素をID指定で明示的に削除する操作は、ADR 0027§4が懸念した「大きな宣言的Intentのうち名前を挙げなかった要素すべてを暗黙に、省略ベースで退役させる」というリスクを共有しない。したがって別個の、より狭い操作として別のInterfaceを持つ。`reconcile`の「`merge`のみ・削除なし」という契約は変更しない。

CLIの既存の非対話的な運用方針([0028](0028-consolidate-knowledge-authoring-commands.md)§3)と、`axes prune`の「明示フラグが必要(プロンプトなし)」という先例に合わせ、確認プロンプトは追加しない。操作は常に明示的に指定された単一要素に対するものなので、即時実行する。

### 2. `<key>`はまずslugとして解決し、必要な場合のみ曖昧性解消を行う

- `requirement`と`feature`: `<key>`は常にプロジェクト全体で一意な表示`id`。曖昧になることはない。
- `behavior`と`scenario`: `<key>`は表示`id`。一致が1件だけならそれが対象。2件以上一致する場合(異なる親の下に同じslug)、コマンドは全一致の完全な親パスを列挙した診断で失敗し、何も削除せずに停止する。
- 曖昧性を解消するには、呼び出し側は`--feature <id>`(`behavior`用。`scenario`では`--behavior <id>`も併せて必須)で検索対象を1つの親に絞るか、既知であれば`<key>`自体を(不変識別子である)`uid`([0013](0013-immutable-identity-model.md))として渡す。`uid`は常に使えるわけではない(`identity migrate`未実行のプロジェクトではBehavior/Scenarioの`uid`が`None`のことがある)。そのため、`uid`だけに頼らず常に使える第二の曖昧性解消手段として親パス形式(`--feature`/`--behavior`)を用意する。

解決には新規のディレクトリ走査を書かず、`identity::knowledge_walk::find_by_id`/`find_by_uid`(`src/identity/knowledge_walk.rs`)を再利用する。

### 3. 必須単一親参照を持つ子は連鎖削除し、任意の逆参照はデタッチする

- Requirementの削除は、`requirement_uids`にそのUIDを含むすべてのFeature・Scenarioに連鎖するが、それらのFeature・Scenario自体を削除するのではなく、`requirement_uids`を書き換えてそのUIDを除去する(値コレクションの置き換え、[0027](0027-declarative-knowledge-reconciliation.md)§5)。Feature・Scenario自体は残る。
- Featureの削除は、`feature`でそれを指すすべてのBehaviorを物理削除し、Behaviorの削除は、`behavior`でそれを指すすべてのScenarioを物理削除する——`Behavior.feature`/`Scenario.behavior`が必須フィールドで「親なし」を表現する値を持たないため、これは再帰的に行われる。したがってRequirement・Feature・Behaviorの削除は、それを根とする部分木全体を常に取り除く: Requirement → Feature/Behavior/Scenario、Feature → Behavior/Scenario、Behavior → Scenario。
- FeatureまたはBehaviorを削除する前に、その子を手動で先に削除しておく必要はない。コマンドが完全な連鎖を計算し、1回の操作でまとめて削除する。

### 4. この操作は正本のKnowledgeファイルのみに範囲を限定する

`knowledge remove`は`.markharness/knowledge/`配下のファイルのみを削除する。以下には触れない。

- `generated/testcases/` — §5を参照。
- `.markharness/case-definitions/`(追記のみの履行記録、[0017](0017-scenario-case-revision-and-execution-evidence.md))、実行バインディング、削除されたScenarioのCase UIDを参照している可能性のあるrelease-scope選択。これらはダングリング参照になる。その検出や警告は本ADRの範囲外として明示的に扱う。具体的な必要性が生じた場合は将来のADRで対応する可能性があり、これは[0021](0021-identity-retire-simplification.md)がidentityモデル全般に対して既に取ったYAGNIの立場と一致する。

### 5. `generate`には新たなプルーニングロジックは不要

`generate::load_knowledge_snapshot`は`.markharness/knowledge/`を呼び出しごとに新たに走査し、見つけたScenarioファイルごとに1つの`TestCase`を構築する。`application::generate_testcases`は、その都度そのスナップショットから`generated/`ディレクトリ全体を毎回置き換える([0017](0017-scenario-case-revision-and-execution-evidence.md)§3: 1 Scenario = 1 TestCase)。このため、`knowledge remove`によって(直接または連鎖により)削除されたScenarioは、追加のプルーニングコードなしに、次の`generate`実行の出力から既に欠落する。本ADRは、[CLAUDE.md](../../../CLAUDE.md)のYAGNI原則に従い、その既存動作を証明する回帰テストのみを追加し、新規プロダクションコードは追加しない。

### 6. `identity::recovery`のステージングプロトコルを削除を表現できるよう拡張する

`PendingKnowledgeFile`と並ぶ`PendingKnowledgeDelete { relative_path: String }`を追加し、新規`IntentPayload::KnowledgeRemove { deletes: Vec<PendingKnowledgeDelete>, files: Vec<PendingKnowledgeFile> }`バリアント(`identity::recovery`)を追加する——`deletes`は連鎖削除される要素のファイル/ディレクトリ、`files`は逆参照の書き換えを表し、既存の`IntentPayload::KnowledgeReconcile { files, moves }`の形に倣う。

[0021](0021-identity-retire-simplification.md)§4により、削除はidentity eventを発行しない(UID発行とrenameのみが追跡対象)。したがって`knowledge remove`は、内容のみのreconcileパッチと同様に、`batch_events`を空にして`begin_batch_with_payload(root, Vec::new(), Some(payload))`を呼ぶ。その論理的なコミットポイントはeventファイルではなく、`commit_batch`が書く`commit_marker_path`である。

`run_startup_recovery`のすべての呼び出し元が既に使っている単一のディスパッチャである`identity::feature_ops::roll_forward`を、`IntentPayload::KnowledgeRemove`用のmatchアームで拡張する。このアームは`deletes`の各パスを冪等に削除し(既に削除済みでもエラーとしない。既存のmove再生が同じ理由で持つ前例と同様)、`files`の各エントリを`replace_file`する。これは既存のすべての呼び出し箇所が`run_startup_recovery`に渡している、まさに同じディスパッチャなので、新規`knowledge_remove`モジュール自身以外のどの呼び出し箇所も変更する必要はない。

これにより、`knowledge remove`の連鎖削除+逆参照書き換えは、`knowledge reconcile`の複数ファイル書き込みと全く同様にクラッシュ耐性を持つ: あるファイルを削除してから別のファイルを書き換えるまでの間にクラッシュしても、次回起動時のリカバリスキャンで必ず「完全に適用された状態」に収束し、部分的に適用された状態にはならない。

## 不変条件

- 正本のKnowledge要素は、完全に存在する(Behavior/Scenarioについては有効な親の連鎖を持つ)か、完全に存在しないかのいずれかである。`knowledge remove`は必須の親参照を未解決のまま残さない。
- `knowledge remove`操作(およびそれが必要としたクラッシュリカバリ)が完了した時点で、`requirement_uids`はもはや存在しないRequirementのUIDを指すことはない。
- Requirement・Feature・Behaviorの削除は、それに依存するすべての子孫を含む連鎖全体を、1つのクラッシュ耐性のある操作として削除する。部分的な削除にはならない。
- `knowledge remove`はidentity eventを一切発行しない([0021](0021-identity-retire-simplification.md)§4は変更されない)。
- `.markharness/case-definitions/`、実行バインディング、release-scope選択は、`knowledge remove`によって書き換えられることも削除されることもない。

## 影響

- `src/identity/recovery.rs`: 新規`PendingKnowledgeDelete`型と`IntentPayload::KnowledgeRemove`バリアント。
- `src/identity/feature_ops.rs`: 上記バリアント用の`roll_forward`への新規matchアーム。
- 新規`src/knowledge_remove.rs`(または`src/knowledge_reconcile/remove.rs`): 解決(slug/uid/親パス、曖昧性診断)、連鎖の計算、逆参照のデタッチ、およびlock/recovery/commitの手順を、`knowledge_reconcile::execute::reconcile_creation`の既存の形に沿って実装する。
- `src/cli.rs`: 新規`KnowledgeCommand::Remove`バリアントとdispatchアーム。
- `generate.rs`/`application.rs`のテストスイートに、削除済みScenarioのプルーニングが既に機能することを証明する回帰テストを追加する(そこにプロダクションコードの変更はない)。
- AI向けドキュメント(`docs/knowledge-from-code.ai.md`)に、`knowledge reconcile`と並ぶ標準経路のコマンドとして`knowledge remove`を追加する。

## 検討し、採用しなかった選択肢

- **ADR 0027§4の元の計画どおり`reconcile --delete`/`exact`モードとして実装する**:不採用。`exact`モードの危険性は、大きな宣言的Intentで名前を挙げなかったものすべてを暗黙に、省略ベースで退役させることに特有のものであり、単一要素をID指定で明示的に削除する操作はその失敗モードを共有せず、`exact`が必要とするであろう同等の権限確認の儀式も必要ない。別個の、より狭いコマンドの方が、推論もテストもしやすい。
- **Behavior/Scenarioの削除に`uid`を必須とし、なければエラーとする**: 不採用。`identity migrate`未実行のプロジェクトで削除が不可能になってしまい、常に使える親パス(`--feature`/`--behavior`)という代替に対して安全性上の利点がない。
- **削除を直接のファイルシステム操作(ステージング/リカバリなし)で行う**: 不採用。連鎖削除+逆参照書き換えは、`knowledge reconcile`が既にクラッシュ耐性のある操作として扱っている複数ファイル変更と同じ形であり、構造的に同一の失敗モードに対してここだけ小さな保証で済ませることは、明確な利点なしに一貫性を欠く。
- **Scenarioとともにcase-definitions/バインディング/release-scope選択も連鎖削除する**: 今回は不採用。[0017](0017-scenario-case-revision-and-execution-evidence.md)はこれらを追記のみの履行記録として扱っており、それらに触れる具体的な必要性はまだ生じていない([0021](0021-identity-retire-simplification.md)の前例と同じYAGNI)。将来のADRのための、文書化されたダングリング参照の可能性として残す。
- **明示的な`--force`/確認フラグを追加する**: 不採用。コマンドは既に明示的に指定された単一要素に範囲を限定されているため、確認ステップが意味のある形で防ぐべき「省略による大きな爆発範囲」というリスクがなく、CLIには合わせるべき他の対話的確認の先例もない。
