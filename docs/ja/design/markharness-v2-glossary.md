# markharness v2 の用語

markharnessは、外部で定義されたテストケースの版を比較し、要件・テスト・証跡のつながりを追跡する。

## Language

**Artifact**：要件、テストケース、実行可能テストなど、版を越えて同じ対象として追跡するもの。

**ArtifactVersion**：あるArtifactの比較対象となる内容を固定した版。

**Case revision**：テストケースの検証内容の版。対象製品の版やテスト実装の版とは異なる。
_Avoid_：Git commitを無条件にケース版と呼ぶこと。

**Implementation revision**：実行可能テストと、その動作に必要な依存物を固定した版。

**VerificationTarget**：どのケース版を、どの実装版・対象製品・環境・要件との関係で検証すべきかを固定した検証単位。

**Verification context**：ケースが検証する要件の版と、それに至る関係を固定した文脈。

**Verification Plan**：比較した版の間で必要になる検証と、追跡上の不足を示す計画。

**Execution Contract**：外部実行ツールと交換する、検証対象および返却証跡の取り決め。

**Evidence**：外部実行ツールが報告した結果と、その結果が何を検証したかを示す不変の記録。

**Evidence Applicability**：証跡が、指定されたVerificationTargetに適用できるかという判定。合否とは独立する。

**Coverage gap**：検証が必要な要件や変更について、ケースまたは実装への追跡が成立していない状態。

**Evidence Selection**：計画の各検証単位に採用する証跡を明示した記録。
