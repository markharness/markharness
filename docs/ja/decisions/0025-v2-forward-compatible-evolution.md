# 0025: V2の簡略モデルを将来の完全モデルと区別して発展させる

## ステータス

Accepted（2026-09-11決定、2026-09-12実装完了。`checklist-v2-core.md`参照）。V2でStrictDoc→markharness→Playwrightの縦方向の流れを実運用し、その観測結果から必要な完全モデルだけを追加する方針を定める。[0020](0020-execution-status-lightweight-model.md)の記録名を`ExecutionBinding`へ改めるが、保持する情報とMVPの責務境界は変更しない。

## 背景

V2は、RequirementとTestCaseの対応、修正漏れ、Release Scope、検証手段を小さなモデルで扱う。一方、将来設計は、選定理由、Case revision・build・環境に対する実行結果、判断の失効、完全なIdentity lifecycleまで扱い得る。V2の運用前にこれらを実装すると仮説に基づく複雑性を固定するが、V2の簡略記録を後から完全な事実として読み替えると、存在しない履歴や証拠を捏造することになる。

特に、検証手段の登録、リリースでの選定、実際の実行、合格は異なる事実である。また、V2期間中の単純な削除から、将来の`retire`・`restore`・ID予約解除の意図を決定的に復元することはできない。この区別は永続形式を実装する前に固定する必要がある。

## 決定

### 1. `ExecutionStatus`を`ExecutionBinding`へ改称する

V2が記録する`case_uid`、`mode: automated | manual`、任意の`reference`は、実行状態ではなくTestCaseと検証手段の対応宣言である。現在の意味を型名にも反映し、設計・用語・新規CLIでは`ExecutionBinding`を正とする。

`ExecutionBinding`は実行日時、結果、Case revision、build、環境、attempt、証跡を持たず、その存在を「実行済み」または「合格」と解釈しない。[0020](0020-execution-status-lightweight-model.md)の軽量化判断は維持する。

### 2. 簡略記録と将来の完全記録を別の型・別のrecord kindにする

次の関係を不変条件とする。

```text
ExecutionBinding ≠ ExecutionFact
ReleaseScope ≠ ReleasePlan
Spec-Reviewed trailer ≠ structured ImpactDecision / HumanAttestation
Git上の削除・再登場 ≠ retire / restore event
```

永続レコードと公開JSONは`schema_version`を持つ。複数種類のレコードを同じ保存領域または出力で扱う場合は`record_kind`等により種類を明示する。将来型を追加するときは旧型へ空の予約フィールドを足すのではなく、新しい型として追加する。読み取り側は旧型の不足情報を`unknown`または`legacy`として扱い、推測で補完しない。

### 3. 共通基盤だけをV2で安定させる

将来モデルから逆算してV2で維持する契約は、型付きUID、1 Scenario = 1 TestCase、Case UIDとCase revisionの分離、Requirementとの関連をCase revisionへ混ぜないこと、StrictDocの固定参照、Case UIDによるPlaywright binding、再現に必要なGit refと規則versionを結果へ含めることである。

将来のrunner、証跡、承認、Identity lifecycleのための汎用plugin機構、空のDomain型、状態遷移は先行実装しない。二つ目の実在Adapterまたは具体的な運用要求が現れた時点でseamを導入する。

### 4. StrictDocとPlaywrightの実運用を次段階の判断材料にする

V2導入後、StrictDocの変更から影響TestCaseを特定し、Case UIDでPlaywright testへ接続する流れを実際に使用する。Requirement単位解析の必要性、bindingの0件・複数件、parameterized test、Playwright project、retry、対象commit、ReleaseScopeと実行集合の差などを観測する。

観測は直ちに完全なExecution FactとしてGitへ保存することを意味しない。必要な照合条件と保存単位が実データで確認されてから、対応するDomain型とAdapterを別ADRで決定する。

### 5. 後から追加する保証にはcutoverを設ける

Execution Factの適用可能性や完全なIdentity lifecycleを将来導入する場合、保証開始commitを明示する。cutoverより前のV2記録は、保持していないCase revision・build・環境・削除意図を後から推定せず、`legacy`または`unknown`として扱う。

Identityについては[0021](0021-identity-retire-simplification.md)の単純化を維持する。将来`retire`・`restore`・ID予約を再導入する場合は、cutover時点のactive identityと必要なretired identityをmigration manifestで確定し、それ以降だけを完全なevent lifecycleとして保証する。

## 結果

- V2は将来モデルの不完全な代用品ではなく、単独で意味が閉じたMVPになる。
- StrictDoc・Playwrightの実態を観測してから、必要なRelease Plan、Execution Fact、判断記録を選択できる。
- 過去の簡略記録を強い証拠へ誤変換することを防げる。
- `ExecutionBinding`への改称により、AIや利用者が登録状態を実行状態と誤認しにくくなる。
- 将来の完全なIdentity保証はcutover以後に限定され、V2期間の意図を推測する移行を避けられる。

## 検討したが採用しない選択肢

- **将来型の全フィールドをoptionalとしてV2へ先に追加する**：一つの型が宣言、計画、事実を兼ね、欠落値の意味が不明になる。
- **V2の記録を将来型へ自動変換する**：保存されていない実行条件や削除意図を復元できず、監査上の虚偽を生む。
- **将来拡張を一切考慮せずV2を実装する**：UID・Case revision・レコード種別など、後から変更すると永続データ移行になる小さな契約まで失う。
- **完全な新設計を先に実装する**：StrictDoc・Playwrightの実運用で必要性と粒度を確認できていないDomain型を固定する。
