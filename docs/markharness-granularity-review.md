# markharnessの「粒度問題」における立ち位置 — 論文修正提案用まとめ

## 1. 結論(現在の立ち位置の要約)

markharnessは、テスト知識の粒度(Feature単位でどこまでConditionをまとめるか)の質を**ツール側で一切検証・診断せず、完全に利用者の設計判断に委ねている**。この点において、比較対象として自ら挙げているDoorstop・StrictDoc・GTM・tmt/fmfと同じカテゴリに属する。一方、ソフトウェア工学の別分野(マイクロサービス分割研究)では、この種の粒度問題を凝集度・結合度・共変更頻度などの定量指標に基づき**構造的・アルゴリズム的に解決しようとする研究**が20年以上蓄積されているが、markharnessの設計にはこの方向の検討が一切含まれていない。したがって現時点の論文は、この既知の関連研究領域(粒度の自動決定・準最適化)に対する言及と、自らの設計がその領域を意図的に採用していない理由の両方を欠いている。

## 2. なぜmarkharnessでこの問題が顕在化するのか(技術的根拠)

- `impacted_testcases`はFeatureディレクトリ全体のtree SHA比較(Section 3.1, 3.5)を起点に、Feature配下の全TestCaseを候補として返す、意図的な保守的(safe-selection的)設計である。
- この設計はCondition/ExpectedResult単位の変更検知バグ(旧blob SHA方式)を修正した副産物として採用されており、粒度設計の質そのものへの対応として導入されたものではない(Section 3.1 Implementation noteに経緯の記載あり)。
- 結果として、1つのFeatureに束ねられたCondition数が多いほど、偽陽性(無関係なTestCaseが再検証候補に混入)の絶対数が増える。この効果はFeature粒度に対して単調に悪化する構造になっている。
- `markharness validate`はスキーマ整合性とaxis/forked_fromの参照整合性は検証するが、粒度の妥当性(Condition数の分布、共変更相関など)を診断する機能を持たない。

## 3. 類似ツールとの比較における位置づけ

| 分類 | 例 | 粒度の質保証 | markharnessとの関係 |
|---|---|---|---|
| Git管理型テスト/要件管理ツール | Doorstop, StrictDoc, GTM, tmt/fmf | なし(利用者裁量) | markharnessと**同じ立場**。論文Section 2.8で比較対象として既出。 |
| マイクロサービス分割研究(手動手法) | DDD, Service Cutter等 | 半構造化(基準セットに基づく人間の判断支援) | markharnessは未参照。粒度診断支援の中間解として参考になりうる。 |
| マイクロサービス分割研究(自動/準最適化) | Bunch, MSExtractor, GC-VCG等 | 凝集度・結合度をフィットネス関数化し探索的に最適化 | markharnessは未参照。「構造で粒度問題を解決しようとする」研究群として言及すべき先行研究。 |

重要な事実: マイクロサービス粒度決定手法に関する文献調査では、自動的手法は少数派であり、大多数(調査対象29本中15本)が依然として手動の方法論に留まっている。つまり「粒度を構造で解く」研究は存在するものの、業界標準としては未確立であり、markharnessが手動裁量に委ねていること自体は、この分野の主流と整合した選択でもある。

## 4. 論文への修正提案(具体的な挿入案)

### 4.1 Section 2(Related Work)への追加提案

現状Section 2.8はテスト管理ツールとの比較に限定されている。以下を新設項目として追加することを提案する。

> **2.x Granularity Determination in Modular Decomposition**
> The granularity at which test knowledge is partitioned into `FEATURE` units is left entirely to manual design in this model, mirroring the practice of Doorstop, StrictDoc, GTM, and tmt/fmf (Section 2.8). This is a known limitation shared broadly among Git-managed, YAML-based knowledge tools. A separate line of research addresses an analogous granularity problem in the context of monolith-to-microservice decomposition, proposing structural or algorithmic determination of module boundaries via graph clustering (e.g., Bunch [ref], based on cohesion/coupling fitness functions) or multi-objective evolutionary search (e.g., MSExtractor [ref], optimizing granularity, coupling, and cohesion jointly). A systematic literature review of this area found that automatic techniques remain a minority approach relative to manual methodologies [ref], suggesting that granularity determination is an open problem even in a more mature research context. This study does not attempt to adopt such techniques; the resulting exposure — Feature-level conservative candidate extraction over-including unaffected TestCases when granularity is coarse — is discussed as a limitation in Section 6.

### 4.2 Section 3.5(自動生成とディレクトリ構造)への追記提案

現状「Granularity of candidate extraction」の段落はConditionレベルへの絞り込み欠如のみを述べている。以下を追加。

> The quality of Feature granularity itself — how many Conditions a single Feature aggregates — is not validated or diagnosed by any command in this implementation (`markharness validate` checks schema and cross-reference integrity only, not granularity distribution). This design choice is consistent with comparable Git-native knowledge tools (Doorstop, StrictDoc, GTM, tmt/fmf; Section 2.8), none of which provide granularity diagnostics either. The consequence is that the false-positive rate of `impacted_testcases` scales with the number of Conditions aggregated under a single Feature, and this scaling is monotonic and unbounded by any tool-level safeguard.

### 4.3 Section 6(Threats to Validity)への追記提案

> - **Granularity dependency**: The precision of `impacted_testcases` is sensitive to how finely or coarsely a project partitions Features. Because this partitioning quality is not diagnosed by the tool, the evaluation plan in Chapter 5 should report the granularity characteristics of the target project's `knowledge/` tree (e.g., Conditions-per-Feature distribution) as a covariate, since a project with coarse Feature granularity may show systematically lower precision independent of the model's underlying validity.

### 4.4 Section 7(Future Work)への追記提案

現状Future Workの「Refinement of candidate extraction based on Condition/ExpectedResult diffs」の項目は、Condition単位への絞り込みのみを射程に入れている。粒度そのものへの対応を別項目として明示的に追加することを提案する。

> - **Granularity-aware diagnostics**: Beyond narrowing `impacted_testcases` to the changed Condition/ExpectedResult (already listed above), a complementary direction is diagnosing Feature granularity itself — e.g., flagging Features whose Condition count or `impacted_testcases` candidate count deviates statistically from the project's historical distribution, or surfacing co-change signals among Conditions within a Feature as a hint for split/merge decisions. This draws on structural/algorithmic module-boundary determination research from the microservice-decomposition literature (e.g., fitness-function-based clustering [Bunch], multi-objective evolutionary search [MSExtractor]), which this study does not adopt but which represents a plausible extension. Whether such diagnostics meaningfully reduce false positives without introducing new complexity is an open question, particularly given that automated approaches remain a minority practice even in the more mature microservice-granularity literature.

## 5. まとめ(論文改訂の狙い)

この一連の追記の狙いは、**「粒度問題を見落としていた」という印象を消し、「粒度問題を認識した上で、既存の類似ツールと同じ選択をした」という立場を明示すること**にある。現状の論文はSection 3.5で候補抽出の粗さを認めているが、その原因が「Condition単位への未対応」というローカルな実装の話に留まっており、より上位の「粒度設計そのものをツールが保証しない」という構造的な設計判断としては言語化されていない。この構造判断を明示し、かつ対応する研究領域(マイクロサービス粒度決定)への言及を追加することで、Section 2.9で著者らが行っている「個々の要素に新規性はない、統合が仮説」という自己批評的なスタンスとの一貫性が保たれる。

## 参考文献候補(引用形式は査読先の指定に合わせて要調整)

- Vera-Rivera et al. (2021). Defining and measuring microservice granularity — a literature overview. *PeerJ Computer Science*. https://pmc.ncbi.nlm.nih.gov/articles/PMC8444086/
- Saidani et al. MSExtractor: multi-objective evolutionary search for microservice extraction. https://www.sciencedirect.com/science/article/abs/pii/S0950584922001264
- Bunch (search-based software modularization via fitness functions on cohesion/coupling). 参照: CARGO関連文献 https://www.researchgate.net/publication/362252400_CARGO_AI-Guided_Dependency_Analysis_for_Migrating_Monolithic_Applications_to_Microservices_Architecture
- Doorstop GitHub Issue #430 (item granularity design discussion). https://github.com/doorstop-dev/doorstop/issues/430
