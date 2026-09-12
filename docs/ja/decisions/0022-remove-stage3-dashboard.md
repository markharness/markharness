# 0022: Stage 3で実装したdashboard(`server.rs`・`ui/`)を廃止する

## ステータス

Accepted(2026-09-11決定、2026-09-12実装完了。`checklist-v2-core.md`参照)。[0008](0008-verification-plan-product-roadmap.md) Stage 3で実装したRelease Verification Dashboardを廃止する決定であり、[0008](0008-verification-plan-product-roadmap.md)のその他の内容(Stage 0〜2の実装記録、モジュラーモノリス方針)はそのまま有効。

## 背景

[0008](0008-verification-plan-product-roadmap.md) Stage 3では、localhost限定のread-only Release Verification Dashboard、Feature History、Rustバイナリへのfrontend同梱を実装した(`src/server.rs`、`ui/`、`markharness serve`、`tests/server.rs`)。

現行UI(`ui/app.js`)は`plan`の出力、すなわちEvidence適用可能性の判定結果を表示する構造になっている。[0020](0020-execution-status-lightweight-model.md)が`plan.rs`のEvidence突合ロジックを`ExecutionStatus`の有無参照へ縮小するため、この表示内容そのものが失われる。dashboardを残すには、Change Impact/Release Coverage向けに表示を作り直す必要がある。

一方、[markharness-v2-design.md](../design/markharness-v2-design.md)§8はMVPの出力をCLI/JSONに限定し、ビューは別ツールに委ねる方針を採っている。dashboardを作り直すことは、MVPの命題(North Starの4問)に寄与しないUIの保守コストを恒常的に負うことを意味する。

## 決定

`src/server.rs`、`ui/`、frontendのバイナリ同梱、`markharness serve`コマンド、および`tests/server.rs`等の関連テストを削除する。

削除は[0020](0020-execution-status-lightweight-model.md)に基づく`plan.rs`の縮小と同じ実装タイミングで行う。表示が必要な場合は、CLI/JSON出力(`impact`・`coverage`)を別ツールが読む構成とする。

## 影響範囲

- `markharness serve`は廃止する。CLIマニュアル(`docs/ja/cli-manual.md`・`docs/en/cli-manual.md`)の該当節も同時に削除する。
- リポジトリ外のviewerが`plan`出力を参照している場合、Change Impact/Release Coverageの出力への切替が必要になる。
- [0008](0008-verification-plan-product-roadmap.md)のステータス欄に本ADRへの参照を追記する。ADR本文は歴史的決定記録として書き換えない。

## 検討したが採用しない選択肢

- **Change Impact/Release Coverage向けに作り直す**：表示はあると便利だが、v2のMVPはCLI/JSONのみで命題を満たせる。UIの需要が具体的に生じた時点で、その時点の出力契約に対して設計する方が妥当である([CLAUDE.md](../../../CLAUDE.md)のYAGNI原則)。
- **削除せず放置する**：`plan`縮小と同時にビルドまたは表示が壊れ、壊れたまま残るコードとテストになる。判断を先送りする利点がない。
