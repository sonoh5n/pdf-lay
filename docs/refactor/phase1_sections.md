# Phase 1 — セクション判定の再設計

**対象読者:** 本フェーズを実装するコーディングエージェント（Codex 等）およびレビュアー
**上位規範:** 本書は `docs/refactor/00_REVIEW_POLICY.md`（以下「方針書」）に従属する。食い違う場合は方針書を優先。
**位置づけ:** リファクタリング提案書 `REFACTORING_PROPOSAL.md` §3 Phase 1（訴え④「セクション判定が曖昧」）の実装設計。

---

## 前文

### 目的

ヘッダー（見出し）検出とセクション階層構築を、**分類器の結果を尊重し（分類は一度だけ）**、
**番号体系を第一級の構造キーとし**、**文書大域のフォント分布**でレベルを決め、
**英語専用ヒューリスティックへの依存を排して CJK・多言語に対応**し、
**見出しゼロでも文書全体が1セクションに潰れない**ように作り直す。

現状の欠陥（提案書 §1.2 の S1〜S9）は次の実コードに対応する:

- `HeaderDetector::detect` が `block_type` を無視し、caption / running head / reference まで見出し候補にする（S2）。
- `detect_repeated_headers_footers` が pipeline に未接続で走りヘッダが偽見出し化する（S3）。
- `text.len()`（**バイト長**）フィルタと英語専用シグナル（all-caps / 既知名 `contains`）で CJK 見出しが落ちる（S4）。
- レベルをブロック単体の孤立閾値 `1.1` / `1.15` / `1.8` で決め、番号体系混在や `1 Introduction`（ピリオド無し）に弱い（S5・S8）。
- 見出し未検出時に文書全体が1セクション化する（S1）。
- selector の無境界 `contains` マッチ + サブツリー丸呑み（S7）。
- `SectionHeader.block_index` に enumerate 位置を格納し `global_index` で引く時限爆弾（S9）。

### Phase 0 への依存（着手前提）

本フェーズは **Phase 0（`phase0_foundation.md`）が全タスク・マージ済み**であることを前提とする。特に:

- **P0-1（実ページ寸法）／P0-2（全域割当）:** 上端帯（タイトル・最初の見出し）や境界跨ぎ行が脱落しなくなっていること。
  これが未完だと本フェーズのゴールデン（見出し再現率）が上流バグに汚染され測定不能になる。
- **P0-5（drop 対象ブロックの厳格化）:** `is_caption` / `is_running_header` が過剰マッチしない前提。
  本フェーズは「分類結果を供給して除外する」ため、分類自体の健全性を Phase 0 が担保している必要がある。
- **P0-6（カバレッジ計測）:** `AnalysisResult.warnings` にカバレッジ／脱落計測が入っていること。
  本フェーズが追加する警告（番号異常等）は同じ `warnings` チャネルに載せる。

> 依存タスクが未マージなら着手しない（方針書 §2）。1タスク = 1ブランチ = 1PR を厳守。

### タスク一覧（実施順・依存）

| ID | 内容 | 重大度 | 依存（Phase 0 以外） |
|----|------|--------|----------------------|
| P1-1 | 分類結果をヘッダー検出へ供給（caption/running/footnote/reference を候補から除外） | HIGH | なし |
| P1-2 | 反復ヘッダー/フッター除去を pipeline に接続（ヘッダー検出前） | HIGH | なし（P1-1 と独立、順不同可） |
| P1-3 | 文書大域フォントクラスタリングで level 決定（孤立閾値 1.1/1.15 を置換） | MED-HIGH | P1-1 |
| P1-4 | 番号を構造キー化（タプル化・単調性検証・ピリオド任意・ローマ/alpha/深さ>3） | MED-HIGH | P1-1, P1-8 |
| P1-5 | Unicode 正規化（`chars()` カウント）・CJK 見出し・既知名リスト設定化/多言語化 | HIGH | P1-1 |
| P1-6 | no-confident-header フォールバック（フォント変化/段落でセグメント） | HIGH | P1-3 |
| P1-7 | selector マッチモード（exact/word-boundary/normalized、丸呑み既定オフ） | MED | なし |
| P1-8 | `block_index` を `global_index` 直参照化（S9 の時限爆弾除去） | LOW-MED | なし（P1-4 の前提） |

> 推奨着手順: **P1-8 → P1-1 → P1-2 → P1-3 → P1-4 → P1-5 → P1-6 → P1-7**。
> P1-8 は他タスクの基盤（安定キー）なので先に入れると後続が楽。ただし本書の番号順（P1-1…）でも成立する。
> 各タスクの「依存」に未マージがある場合は着手しない。

### 影響を受ける公開API面（方針書 §4「3面反映」）

`HeaderDetector` / `SectionSelector` / `SectionHeader` / `HeaderDetectionConfig` に触れる。
公開シグネチャを変える場合は **CLI（`crates/pdf-lay-cli`）・Python（`crates/pdflay-python`）・`pdf-lay` crate** の呼び出し側を必ず追随させること。
本フェーズは可能な限り**後方互換（既定は新挙動だが旧挙動へ戻せるフラグ／既存メソッド温存）**で設計する。

---

## 堅牢なセクション判定の設計思想

本フェーズが目指す目標アルゴリズムは、3本の柱で構成する。個別タスクはこの全体像の部品である。

1. **分類器主導の候補フィルタ（classifier-informed exclusion）**
   `BlockClassifier` が付与した `block_type` を**唯一の真実**とし、`HeaderDetector` は
   `Caption` / `PageNumber` / `RunningHeader` / `RunningFooter` / `Footnote` / `Reference` を**候補から除外**する。
   分類のやり直しをしない（方針書 §1-3「分類は一度だけ」）。反復走りヘッダは検出前に `detect_repeated_headers_footers` で確定させる。

2. **番号ラティス優先（numbering-lattice primary）**
   見出しテキスト先頭の番号（`2`, `2.1`, `II.`, `A.`, `付録A`…）を**構造タプル**（例 `[2,1,3]`）へパースし、
   タプルの深さを level、タプル列の親子関係でツリーを組む。番号が付いている限り、これが**最優先の構造信号**。
   単調性・スキップ・重複を検証し、逸脱は `warnings` に記録する（黙って直さない・黙って捨てない）。

3. **フォントクラスタ・フォールバック（font-cluster fallback）**
   番号が無い見出しには、**文書大域**で集めた見出し候補のフォント特徴（`font_size`・`bold`・`is_all_caps`・`left_x`）を
   決定的にクラスタリングし、クラスタのランク（大きい/太い/字下げ浅い順）を level へ写像する。
   孤立閾値（現行 `1.1`/`1.15`）は使わない。さらに**確信できる見出しが1つも無い**場合は、
   フォント変化・段落境界で文書をセグメントし、1セクション化を防ぐ（P1-6）。

出力は既存 IR（`Section` ツリー、serde 安定）に射影する。selector は正規化マッチと丸呑み抑制で「子だけ選ぶ」を可能にする（P1-7）。

---

### P1-1: 分類結果をヘッダー検出へ供給（候補フィルタ）

**目的:** `BlockClassifier` が非本文と判定したブロック（caption/running head/footnote/reference/page number）を `HeaderDetector` の見出し候補から除外し、分類のやり直しを止める。

**重大度 / 依存:** HIGH（提案書 S2） / Phase 0 全マージ。P1-2 とは独立（順不同）。

**対象ファイル:**
- `crates/pdf-lay-core/src/structure/header_detector.rs:84-90`（`detect`）, `:92-98`（`try_detect` 冒頭）
- `crates/pdf-lay-core/src/config.rs:176-193`（`HeaderDetectionConfig`）
- `crates/pdf-lay-core/src/pipeline.rs:147-156`（分類→検出の配線。既に分類済みブロックを渡している）

**現状（問題）:**
`header_detector.rs:84-90`:
```
pub fn detect(&self, blocks: &[TextBlock]) -> Vec<SectionHeader> {
    blocks
        .iter()
        .enumerate()
        .filter_map(|(i, block)| self.try_detect(block, i))
        .collect()
}
```
`try_detect` は `block.text` / `primary_font_size` / `is_bold` のみを見て `block.block_type` を**一切参照しない**。
その結果、`BlockClassifier::classify`（`block_classifier.rs:185-217`）が `Caption` / `RunningHeader` / `Footnote` などに分類したブロックでも、太字・短行・既知名を満たせば見出しスコアが立ち、偽見出しになる。分類器の仕事を捨てている（方針書 §1-3 違反状態）。

**変更後の期待動作:**
`detect` は各ブロックについて **`is_header_eligible(block)` が false のものを候補から除外**する。除外対象は
`BlockType::{Caption, PageNumber, RunningHeader, RunningFooter, Footnote, Reference}`。
`BodyText` / `SectionHeader` / `SubsectionHeader` / `Title` / `Abstract` / `ListItem` / `Equation` / `Unknown` は従来通り候補に残す（`try_detect` のスコアリングで最終判定）。
除外は「見出し候補から外す」だけで、ブロック自体は破棄しない（本文としてセクションに残る＝No Silent Drop 準拠）。

**実装手順:**
1. `header_detector.rs` に private 関数を追加:
   ```
   fn is_header_eligible(block_type: &BlockType) -> bool {
       !matches!(block_type,
           BlockType::Caption | BlockType::PageNumber
           | BlockType::RunningHeader | BlockType::RunningFooter
           | BlockType::Footnote | BlockType::Reference)
   }
   ```
   （`use crate::types::BlockType;` を追加。）
2. `HeaderDetector` に `respect_classification: bool` フィールドを追加し、`new()` で `true` 初期化、`with_config` の `..Self::new(...)` で引き継ぐ。旧挙動（分類無視）へ戻せるようにする。
3. `detect` を次に変更（enumerate 位置は P1-8 で `global_index` へ置換予定。本タスクでは i を維持）:
   ```
   blocks.iter().enumerate()
     .filter(|(_, b)| !self.respect_classification || Self::is_header_eligible(&b.block_type))
     .filter_map(|(i, b)| self.try_detect(b, i))
     .collect()
   ```
4. `config.rs` の `HeaderDetectionConfig` に `#[serde(default = "default_respect_classification")] pub respect_classification: bool`（既定 `true`）を追加。`Default` 実装と `default_respect_classification()` を用意。
5. `header_detector.rs::with_config` のシグネチャに `respect_classification: bool` を追加し、`pipeline.rs:150-156` の呼び出しを `config.header_detection.respect_classification` で渡すよう更新。
6. `pipeline.rs` は既に `classifier.classify_all(&mut blocks)`（`:148`）の**後**に `detect`（`:150-156`）を呼んでおり、渡すのは分類済み `&blocks`。**配線変更は不要**（＝分類結果はすでに供給されている）。本タスクは「供給された `block_type` を実際に読む」だけ。

**シグネチャ変更:**
- `HeaderDetector::with_config(body, min_score, max_chars, max_lines)` → `with_config(body, min_score, max_chars, max_lines, respect_classification: bool)`。
- `HeaderDetectionConfig` に `respect_classification: bool` フィールド追加。
- `detect` の外形シグネチャは不変（`&[TextBlock] -> Vec<SectionHeader>`）。

**後方互換:**
- `respect_classification` の既定は `true`（新挙動）。`false` で旧挙動を完全再現。
- `HeaderDetectionConfig` の新フィールドは `#[serde(default = ...)]` 付きなので既存 JSON/TOML 設定はそのままデシリアライズ可能。
- `with_config` の引数追加は破壊的だが呼び出しは pipeline 1 箇所 + テスト。CLI/Python は `HeaderDetectionConfig` 経由なので影響なし（要確認の上、直接呼びがあれば追随）。

**受け入れ基準（Given/When/Then）:**
- Given `block_type = Caption` の「Table 1 shows …」ブロック、When `detect`（`respect_classification=true`）、Then 見出しとして返らない。
- Given `block_type = RunningHeader` の毎ページ反復ブロック、When `detect`、Then 見出しに含まれない。
- Given `block_type = BodyText` の正当な太字見出し「3. Methods」、When `detect`、Then 従来通り見出しとして返る。
- Given `respect_classification=false`、When 同じ Caption ブロック、Then 旧挙動どおりスコア次第で見出しになり得る（後方互換）。

**追加テスト:**
- `header_detector.rs` ユニット: `caption_block_excluded_from_headers`（Caption を渡し `headers.is_empty()`）、`footnote_block_excluded`、`running_header_block_excluded`、`reference_block_excluded`、`bodytext_header_still_detected`、`respect_classification_false_restores_legacy`。既存の `make_block` に `block_type` 引数を足すヘルパ拡張を行う。
- ゴールデン: IEEE 2 段組フィクスチャで、走りヘッダ文字列が `toc` の見出し一覧に出ないことを assert。arXiv フィクスチャで既知見出し（Introduction 等）が全て残ることを assert。日本語 A4 は P1-5 側で追加。

**非スコープ:** 分類ロジック自体（`is_caption` 等）の改良は Phase 0 P0-5 の担当。level 決定（P1-3）、番号（P1-4）には踏み込まない。

**検証:**
- `cargo fmt --all --check` / `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test -p pdf-lay-core header_detector`
- 手動: `cargo run -p pdf-lay-cli -- toc tests/fixtures/ieee_2col.pdf` で走りヘッダが見出しに混ざらないことを目視。

---

### P1-2: 反復ヘッダー/フッター除去の pipeline 接続

**目的:** 既存の `detect_repeated_headers_footers` を `analyze_pdf` の**ヘッダー検出前**に実行し、毎ページ反復する走りヘッダ/フッタを事前に `RunningHeader`/`RunningFooter` へ確定させて偽見出し化を防ぐ。

**重大度 / 依存:** HIGH（提案書 S3） / Phase 0 全マージ。P1-1 と独立だが、P1-1 の除外対象に効く（相乗）。

**対象ファイル:**
- `crates/pdf-lay-core/src/pipeline.rs:147-156`（分類とヘッダー検出の間に挿入）
- `crates/pdf-lay-core/src/structure/block_classifier.rs:78-182`（`detect_repeated_headers_footers`、現状 pipeline 未接続でテストからのみ呼ばれる）
- `crates/pdf-lay-core/src/config.rs`（新フラグ）
- `crates/pdf-lay-core/src/error.rs`（任意: 反復除去件数の警告）

**現状（問題）:**
`block_classifier.rs:78` の doc に「This method is intended to be called **after** `classify_all`」と明記されているが、`pipeline.rs` にはその呼び出しが**存在しない**（`:147-148` で `classify_all` した直後、`:150` でいきなり `HeaderDetector::detect`）。結果、走りヘッダは `RunningHeader` に昇格されず `BodyText` のまま検出器に流れ、毎ページ偽見出しになる。関数はテスト（`block_classifier.rs:456-610`）からしか呼ばれない死に配線。

**変更後の期待動作:**
`analyze_pdf` は `classify_all` の直後・`HeaderDetector::detect` の直前に `BlockClassifier::detect_repeated_headers_footers(&mut blocks)` を呼ぶ（`config.header_detection.detect_repeated_running` が `true` のときのみ）。これにより 3 ページ以上で反復する上/下端ゾーンの同一テキストが `RunningHeader`/`RunningFooter` に再分類され、P1-1 の除外対象になる。反復除去した件数を `warnings` に記録する（黙って消さない）。

**実装手順:**
1. `config.rs::HeaderDetectionConfig` に `#[serde(default = "default_detect_repeated_running")] pub detect_repeated_running: bool`（既定 `true`）を追加。`Default` と default 関数を用意。
2. `pipeline.rs`、`classifier.classify_all(&mut blocks);`（`:148`）の直後に挿入:
   ```
   if config.header_detection.detect_repeated_running {
       let before = count_running(&blocks);            // RunningHeader/Footer の数
       BlockClassifier::detect_repeated_headers_footers(&mut blocks);
       let added = count_running(&blocks) - before;
       if added > 0 {
           warnings.push(PdfLayWarning::RepeatedRunningReclassified { count: added });
       }
   }
   ```
   `count_running` は private ヘルパ（`blocks.iter().filter(|b| matches!(b.block_type, RunningHeader|RunningFooter)).count()`）。
3. `error.rs::PdfLayWarning` に variant を追加:
   ```
   /// 反復ヘッダー/フッターとして再分類されたブロック数。
   RepeatedRunningReclassified { count: usize },
   ```
   `Display` 実装（内容テキストを漏らさない方針に従い件数のみ）: `"reclassified {count} repeated running header/footer block(s)"`。
4. 副作用は `blocks` の `block_type` のみ（テキスト・順序・`global_index` は不変）。ヘッダー検出（`:150`）はこの変更済み `blocks` を受け取る。

**シグネチャ変更:**
- `HeaderDetectionConfig` に `detect_repeated_running: bool` 追加。
- `PdfLayWarning` に `RepeatedRunningReclassified { count }` 追加。
- 関数シグネチャ変更なし（`detect_repeated_headers_footers` は既存の `&mut [TextBlock]` のまま）。

**後方互換:**
- 既定 `true`（新挙動）。`false` で完全に旧挙動（未接続相当）に戻せる。
- 新 config フィールドは `#[serde(default)]`。`PdfLayWarning` は非 serde・追加のみで既存パターンマッチは網羅性次第（`error.rs` 内 `Display` を更新すれば足りる。外部で `match` している箇所があれば `_ =>` の有無を確認）。

**受け入れ基準（Given/When/Then）:**
- Given 同一テキストが 3 ページ以上の上端 10% ゾーンに出るブロック群、When `analyze_pdf`、Then それらは `RunningHeader` になり `toc` の見出しに現れず、`warnings` に `RepeatedRunningReclassified{count>=3}` が入る。
- Given 2 ページにしか出ない文字列、When `analyze_pdf`、Then 再分類されない（`detect_repeated_headers_footers` の既存 3 ページ閾値、`block_classifier.rs:147`）。
- Given `detect_repeated_running=false`、When `analyze_pdf`、Then 反復除去は走らず旧挙動。

**追加テスト:**
- `pipeline.rs` ユニット（合成 `TextBlock` を直接組めないなら小さな結合）: 反復ヘッダ入り合成文書で `analyze_pdf` 相当の順序（classify→repeated→detect）を検証する薄いテスト、または `structure` 層に「classify_all→detect_repeated→detect」の順序を通す統合テスト `repeated_running_removed_before_header_detection`。
- 既存の `block_classifier.rs` 反復検出テスト群（`test_repeated_header_detected` 等）は不変で緑のまま。
- ゴールデン: IEEE 2 段組で、ページ上部の雑誌名/著者走りヘッダが見出しにならないことを回帰。

**非スコープ:** `detect_repeated_headers_footers` のゾーン判定アルゴリズム自体（10% 閾値・3 ページ閾値）の改良はしない。ページ高さ推定の精度は Phase 0/4 の管轄。

**検証:**
- `cargo test -p pdf-lay-core block_classifier` / `... pipeline`
- 手動: `cargo run -p pdf-lay-cli -- toc tests/fixtures/ieee_2col.pdf`、警告出力に反復除去件数が出ることを確認。

---

### P1-3: 文書大域フォントクラスタリングで level 決定

**目的:** 見出しレベルをブロック単体の孤立閾値（`1.1`/`1.15`）ではなく、文書全体で集めた見出し候補のフォント特徴を決定的にクラスタリングしたランクから決める（番号があれば P1-4 を優先し、本タスクは番号なし見出しのフォールバック）。

**重大度 / 依存:** MED-HIGH（提案書 S5） / P1-1（候補フィルタ）。P1-4 と協調（番号優先・クラスタ従属）。

**対象ファイル:**
- `crates/pdf-lay-core/src/structure/header_detector.rs:84-90`（`detect` を2パス化）, `:119-166`（スコア/level 決定、`1.1`は`:141`、`1.15`は`:161`）
- `crates/pdf-lay-core/src/config.rs`（クラスタ閾値の設定化）

**現状（問題）:**
`header_detector.rs:141`:
```
// Larger font (+1).
if size_ratio > 1.1 { score += 1; }
```
`:159-166`:
```
if numbering.is_none() {
    level = if size_ratio > 1.15 || self.is_all_caps(text) { 1 } else { 2 };
}
```
level が**そのブロック単体の `size_ratio` 孤立閾値**だけで決まる。文書内に複数の見出しサイズ階層（例: 14pt 章 / 12pt 節 / 11pt 小節）があっても、`1.15` を跨ぐか否かの 2 値でしか区別できず、3 段以上や、body 比が中途半端なサイズを取りこぼす。`left_x`（字下げ）・`bold` の階層情報も使っていない。マジックナンバー直書き（方針書 §1-6 違反）。

**変更後の期待動作:**
`detect` を **2 パス**にする。
- パス1: 全ブロックを走査し、`is_header_eligible`（P1-1）かつスコア >= `min_score` の**見出し候補**を集める。各候補の特徴 `(font_size, is_bold, is_all_caps, left_x=bbox.left)` を保持。
- 大域クラスタリング: 候補の `font_size` を丸めて（後述）ユニークなサイズ帯を作り、**降順ランク**を付ける。同サイズ帯内は `is_bold`（太字が上位）→ `left_x`（左端が浅い＝インデント小が上位）で副次ランク。ランク → level（1 起点、`max_level` で飽和）へ写像。
- パス2: 各候補の level を「**番号があれば P1-4 のタプル深さ**、無ければクラスタランク由来 level」で確定。`size_ratio>1.1` の加点自体は残す（スコアリングは別問題）だが、**level 決定からは孤立閾値を撤廃**する。

**実装手順（決定的アルゴリズム）:**
1. `header_detector.rs` に候補中間表現を定義:
   ```
   struct HeaderCandidate<'a> {
       index: usize,               // P1-8 後は global_index
       block: &'a TextBlock,
       score: u32,
       numbering: Option<NumberingKey>,   // P1-4
       font_size: f64, is_bold: bool, is_all_caps: bool, left_x: f64,
   }
   ```
2. パス1: `try_detect` を「スコア計算 + 候補生成」に分割（`try_score(block) -> Option<HeaderCandidate>`）。level 決定はここでは**行わない**。
3. 大域クラスタリング関数 `assign_levels_by_font(candidates: &mut [HeaderCandidate], cfg)`:
   - `bin = (font_size / cluster_bin_width).round()`（既定 `cluster_bin_width = 0.5`pt、`detect_body_font_size` の 0.5 ビンと整合、`block_classifier.rs:282`）。
   - ユニーク bin 集合を降順ソート。隣接 bin の差が `cluster_merge_gap`（既定 1 bin = 0.5pt）以内なら同一クラスタに併合（サイズ揺れ吸収）。
   - クラスタへ 0 起点の `size_rank` を降順付与（最大サイズ = rank 0）。
   - 各候補の `level_from_font = min(size_rank + 1, max_level)`（`max_level` 既定 6）。同一クラスタ内で `is_bold==false` かつ非最上位クラスタなら +0（サイズが第一。太字/left_x は**同点崩し**にのみ使い、level をまたいで増減させない）。
   - 決定性: 全ソートに `total_cmp`／`partial_cmp(...).unwrap_or(Equal)` を使い NaN 非パニック（方針書 §1-5）。同値タイブレークは `(font_size desc, is_bold desc, left_x asc, index asc)` の全順序で固定。
4. パス2 `detect`:
   ```
   let mut cands = collect_candidates(blocks);          // パス1
   assign_font_levels(&mut cands, &self.cluster_cfg);   // クラスタ
   cands.into_iter().map(|c| finalize_header(c)).collect()
   ```
   `finalize_header`: `level = c.numbering.map(|k| k.depth()).unwrap_or(c.level_from_font)`（番号優先）。`1.15`/`1.1` の孤立 level 判定は削除。
5. `config.rs` に `HeaderDetectionConfig` 追加フィールド（すべて `#[serde(default = ...)]`）:
   `cluster_bin_width: f64 (0.5)`, `cluster_merge_gap: f64 (0.5)`, `max_level: u8 (6)`。
6. `HeaderDetector` にこれらを保持し `new`/`with_config` で受ける（あるいは `HeaderDetectionConfig` を丸ごと保持する形へリファクタしても良いが、スコープを抑えフィールド追加に留める）。

**シグネチャ変更:**
- `detect` 外形は不変（`&[TextBlock] -> Vec<SectionHeader>`）だが内部が 2 パス化。
- `HeaderDetector::with_config` に `max_level`・`cluster_bin_width`・`cluster_merge_gap` が加わる（または `HeaderDetectionConfig` 参照を渡す形へ変更 → その場合 CLI/Python の Config 生成は不変なので影響小）。**推奨:** `with_config(body_font_size, cfg: &HeaderDetectionConfig)` へ集約し、pipeline 呼び出し（`:150-156`）を1行に簡素化。
- `HeaderDetectionConfig` に 3 フィールド追加。

**後方互換:**
- クラスタ既定値は現行 `detect_body_font_size` のビン幅と整合。level 挙動は変わる（S5 の是正が目的なので**意図した仕様変更**）。旧 2 値挙動へ完全復帰するフラグは設けない代わり、`max_level=2` かつ大サイズ 1 クラスタに寄せる設定で近似可能である旨をドキュメントコメントに記す。
- 既存テスト `arabic_dot_dot_header_level2` 等は**番号優先**（P1-4 マージ後）で維持。P1-4 未マージの段階では、番号ありは従来 `match_numbering` の level を使う（本タスクは番号なしフォールバックのみ差し替え）。この移行順序を PR に明記。

**受け入れ基準（Given/When/Then）:**
- Given 14pt / 12pt / 11pt の 3 段の番号なし太字見出しが混在、When `detect`、Then それぞれ level 1 / 2 / 3 に割り当てられる（3 段以上を表現できる）。
- Given 同一 12.4pt と 12.6pt の見出し（測定揺れ）、When クラスタ併合 gap 0.5、Then 同一 level に落ちる。
- Given 番号付き見出し「2.1 …」、When `detect`（P1-4 マージ後）、Then level は番号タプル深さ 2 で、フォントクラスタより優先。
- Given 出力の決定性、When 同一入力で 2 回実行、Then 割り当て level は完全一致（非決定ソートなし）。

**追加テスト:**
- `header_detector.rs` ユニット: `three_font_tiers_map_to_three_levels`、`near_equal_font_sizes_merge_into_one_level`、`numbering_overrides_font_level`、`level_assignment_is_deterministic`（2 回実行で一致）。合成 `TextBlock` 群を降順シャッフルして順不同でも同結果を確認。
- ゴールデン: arXiv フィクスチャ（多様な見出しフォント）で、人手アノテーションした level 列と一致率を測定（回帰させない）。IEEE（ローマ数字 level1 + 大文字小節）。日本語 A4（サイズ階層のみで番号なし見出し）。

**非スコープ:** スコアリング（`min_score` 加点体系）の再設計はしない。番号パースは P1-4。見出しゼロ時のフォールバックは P1-6。

**検証:**
- `cargo test -p pdf-lay-core header_detector`
- 手動: `cargo run -p pdf-lay-cli -- toc tests/fixtures/arxiv.pdf` で階層 level が妥当か目視。

---

### P1-4: 番号を構造キー化（numbering-lattice）

**目的:** 見出し先頭の番号を構造タプルへパースし、それを第一級の階層キーとしてツリーを構築する。ピリオド任意・ローマ数字・appendix alpha・深さ>3 に対応し、単調性/スキップ/重複を検証して逸脱を警告する。

**重大度 / 依存:** MED-HIGH（提案書 S5） / P1-1, P1-8（安定キー）。P1-3 と協調。

**対象ファイル:**
- `crates/pdf-lay-core/src/structure/header_detector.rs:58-62`（正規表現）, `:123-128`（番号加点）, `:184-203`（`match_numbering`）, `:218-225`（`clean_header_text`）
- `crates/pdf-lay-core/src/types/text.rs:196-213`（`SectionHeader` に構造キー追加）
- `crates/pdf-lay-core/src/structure/section_builder.rs:111-159`（`build_hierarchy` を番号ラティス対応）
- `crates/pdf-lay-core/src/error.rs`（番号異常の警告）

**現状（問題）:**
`header_detector.rs:184-203` の `match_numbering`:
```
if let Some(caps) = self.re_roman.captures(t) { return Some((caps[1].to_string(), 1)); }
if let Some(caps) = self.re_arabic_dot_dot.captures(t) { return Some((caps[1].to_string(), 3)); }
if let Some(caps) = self.re_arabic_dot.captures(t) { return Some((caps[1].to_string(), 2)); }
if let Some(caps) = self.re_arabic.captures(t) { return Some((caps[1].to_string(), 1)); }
if let Some(caps) = self.re_alpha.captures(t) { return Some((caps[1].to_string(), 2)); }
None
```
問題:
- 正規表現 `re_arabic`(`:61`) は `^(\d+\.)\s+`（ピリオド**必須**）。`1 Introduction`（ピリオド無し）は番号未検出になる。
- 番号は `String`（`"3.1"`）で持ち、**構造タプルとしては使われない**。level は固定表（1/2/3）で、4 階層超（`3.1.1.2`）を表現できない。ローマ数字は常に level1、alpha は常に level2 に固定。
- 単調性（`2` の次が `4` に飛ぶ、`2.1` が二重に出る等）を**検証しない**。ツリー構築（`section_builder.rs:build_hierarchy`）は `header.level`（数値）だけを見てスタックで組むため、番号の親子関係と齟齬しても検出されない。

**変更後の期待動作:**
番号を `NumberingKey`（成分列）へパースする。成分は Arabic / Roman / Alpha を区別しつつ**正規化数値**を持つ。level はタプル深さ（`components.len()`）。ツリーは level ではなく**番号タプルの接頭辞関係**で親子を決める（番号ありセクション間）。番号なし見出しは P1-3 のフォントランクで level を得てラティスに差し込む。パース時、同一系列内の**単調増加違反・番号スキップ・重複**を検出し `warnings` に記録（黙って直さない）。番号の見かけと不整合でも**セクションは捨てない**（No Silent Drop）。

**実装手順:**
1. `types/text.rs` に構造キー型を追加（同ファイルか `types` 配下の新モジュール）:
   ```
   #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
   pub enum NumberComponent { Arabic(u32), Roman(u32), Alpha(u32) }   // Roman/Alpha は 1 起点の序数へ正規化

   #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
   pub struct NumberingKey { pub components: Vec<NumberComponent>, pub is_appendix: bool }
   impl NumberingKey { pub fn depth(&self) -> u8 { self.components.len() as u8 } }
   ```
2. `SectionHeader`（`text.rs:196-213`）に `#[serde(default)] pub numbering_key: Option<NumberingKey>` を追加（既存 `numbering: Option<String>` は表示用に温存）。
3. `header_detector.rs` のパーサ `parse_numbering(text) -> Option<(NumberingKey, String /*matched prefix*/)>`:
   - 正規表現（`Regex` を struct に保持、`new()` で 1 度だけコンパイル、`unwrap()` はコンパイル時定数なので許容—既存コードと同流儀 `:58-62`）:
     - 多階層 Arabic（ピリオド任意末尾）: `^(\d+(?:\.\d+)*)\.?(?:\s+|$)` → 各 `\d+` を `Arabic`。`2`, `2.1`, `3.1.1.2` を許容。
     - ローマ数字（任意 level、大文字、任意末尾ピリオド）: `^([IVXLCDM]+)\.?\s+` → ローマ→整数変換 `roman_to_u32`。**誤爆抑制**: `I` 単独等が単語頭字と衝突しうるので、続く語が見出しらしい（大文字始まり or 既知名）か、`.` 付随のときのみ採用。
     - Appendix alpha: `^(?:Appendix\s+)?([A-Z])\.?(?:\s+|$)` → `Alpha(letter-'A'+1)`、`is_appendix=true`。日本語「付録」（P1-5 と協調）も別 config リストで許容。
     - 多階層混在（`A.1`, `IV.2`）: 先頭成分の種別 + 後続 Arabic を許容する結合規則を定義（先頭 Roman/Alpha、以降 Arabic）。
   - 曖昧時の優先順位: Arabic-多階層 → Roman → Appendix-alpha。最長一致を優先。
4. `clean_header_text`（`:218-225`）は `matched prefix` を使って番号を除去（既存の `trim_start_matches` を prefix ベースへ）。
5. `header_detector.rs` の level 決定（P1-3 の `finalize_header`）: `numbering_key` があれば `level = key.depth()`。無ければフォントランク。`SectionHeader.numbering`（String）にも従来互換で prefix 文字列を格納。
6. `section_builder.rs::build_hierarchy`（`:111-159`）をラティス対応に拡張:
   - 各 flat セクションに `numbering_key`（あれば）を持たせる。
   - **番号あり系列**は接頭辞関係で親子決定: `key_child.components` が `key_parent.components` の 1 要素長い接頭辞なら子。level 数値と番号深さが食い違う場合は**番号深さを正**とする。
   - **番号なし**セクションは、直前の確定 level（フォントランク）でスタックへ差し込む（既存スタック法を踏襲）。
   - 単調性検証を `validate_numbering(sequence)` として実装し、`warnings` に:
     ```
     PdfLayWarning::SectionNumberingAnomaly { kind, at_page }
     // kind: NonMonotonic | SkippedNumber | Duplicate
     ```
     を push。異常でも**ツリー構築は続行**（該当見出しは番号なし相当でフォント level 差し込みにフォールバック）。
7. `error.rs::PdfLayWarning` に `SectionNumberingAnomaly { kind: NumberingAnomalyKind, page: u32 }` と enum `NumberingAnomalyKind { NonMonotonic, SkippedNumber, Duplicate }` を追加。`Display` は内容非漏洩（種別 + ページのみ）。

**シグネチャ変更:**
- `SectionHeader` に `numbering_key: Option<NumberingKey>` 追加（`#[serde(default)]`）。
- `HeaderDetector::match_numbering` → `parse_numbering`（内部 API、返り値変更）。
- `SectionBuilder::build_hierarchy` は private のまま内部変更。
- `PdfLayWarning` に variant 追加。
- 新型 `NumberComponent` / `NumberingKey` / `NumberingAnomalyKind` を `types` から `pub` エクスポート。

**後方互換:**
- `SectionHeader.numbering`（String）は残し、既存の `toc.rs:86`（`numbering.clone()` を path に使用）や CLI 表示は不変。
- `numbering_key` は `#[serde(default)]` なので既存 JSON は `None` でデシリアライズ可。
- ローマ数字の level が「常に 1」から「深さ依存」に変わるのは**意図した仕様変更**（S5）。既存テスト `roman_numeral_header_detected`（level1 を期待, `:256-264`）は、トップレベルローマ数字は深さ 1 = level1 で**引き続き緑**。`arabic_dot_dot_header_level2/3` も深さ一致で緑。
- Python/CLI が `numbering`（String）に依存している箇所は不変。`numbering_key` は追加情報。

**受け入れ基準（Given/When/Then）:**
- Given 「1 Introduction」（ピリオド無し）、When `parse_numbering`、Then `NumberingKey{[Arabic(1)]}`、level1、clean_text=「Introduction」。
- Given 「3.1.1.2 Sampling」、When パース、Then 深さ 4 の Arabic タプル、level4。
- Given 「IV. Experiments」→「VI. Results」（V を飛ばす）、When ツリー構築、Then `SectionNumberingAnomaly{SkippedNumber}` が warnings に入り、両見出しはツリーに残る。
- Given 「Appendix A」「B.」、When パース、Then `Alpha` 成分・`is_appendix=true`。
- Given 「2.1」が 2 回出現、When 検証、Then `Duplicate` 警告、両方保持。
- Given 番号あり親子「2」「2.1」、When `build_hierarchy`、Then 2.1 が 2 の子になる（level 数値ではなく接頭辞で決定）。

**追加テスト:**
- `header_detector.rs` ユニット: `parse_dot_optional_arabic`, `parse_deep_arabic_depth4`, `parse_roman_any_level`, `parse_appendix_alpha`, `roman_to_u32_roundtrip`。
- `section_builder.rs` ユニット: `numbering_prefix_builds_tree`, `skipped_number_warns_but_keeps_section`, `duplicate_number_warns`, `unnumbered_falls_back_to_font_level`。
- ゴールデン: IEEE（ローマ数字章 + 英大文字小節 `A.`）, arXiv（Arabic 多階層 + Appendix）, 日本語 A4（番号なし主体でフォールバック）。各々 `toc` の階層と番号 path が期待に一致。

**非スコープ:** 目次（`docs/arch` 仕様）連携や cross-ref 解決はしない。フォント level 決定本体は P1-3。

**検証:**
- `cargo test -p pdf-lay-core header_detector section_builder`
- 手動: `cargo run -p pdf-lay-cli -- toc tests/fixtures/arxiv.pdf` で番号 path と階層が正しいか、`--` 出力の warnings に番号異常が出るか確認。

---

### P1-5: Unicode 正規化と CJK 対応

**目的:** 長さフィルタを**文字数**基準にし、CJK 見出しヒューリスティックを追加し、既知セクション名リストを設定可能・多言語化して、日本語ほか非英語論文でも見出しが落ちないようにする。

**重大度 / 依存:** HIGH（提案書 S4） / P1-1。P1-3/P1-4 と協調（追加シグナルは既存スコアに加算）。

**対象ファイル:**
- `crates/pdf-lay-core/src/structure/header_detector.rs:7-34`（`KNOWN_SECTION_NAMES`）, `:96`（バイト長フィルタ）, `:205-216`（`is_all_caps`/`is_known_name`、`contains` は `:215`）, `:130-148`（スコア加点）
- `crates/pdf-lay-core/src/config.rs:176-193`（既知名リストの設定化）

**現状（問題）:**
`header_detector.rs:96`:
```
if text.len() > self.max_chars || block.lines.len() > self.max_lines { return None; }
```
`text.len()` は**UTF-8 バイト長**。日本語見出し（例「関連研究」）は 4 文字でも 12 バイト。約 40 文字の日本語見出しは 120 バイト超で `max_chars=120` を超え**除外**される（S4）。
`:205-216` のシグナルは英語専用: `is_all_caps` は CJK に無意味（`char::is_uppercase` が false）、`is_known_name` は英語固定リストの `contains`（`:215`）で、日本語見出しに得点が付かない。結果、日本語見出しはスコア不足で検出されない。

**変更後の期待動作:**
- 長さフィルタを `text.chars().count()` に変更（バイト非依存）。既定 `max_chars=120` は文字数として解釈（日本語 40 字見出しが通る）。
- CJK 見出しヒューリスティックを加点シグナルとして追加（例: 行が短く CJK 主体で、行末が句点でない・番号や既知和名を含む等）。
- 既知セクション名リストを `HeaderDetectionConfig` で**設定可能・拡張可能**にし、既定に多言語エントリ（日本語等）を含める。`is_known_name` の無制限 `contains` を**完全一致優先＋境界付き部分一致**へ寄せて過剰発火（S6）を抑える。

**実装手順:**
1. `:96` を `if text.chars().count() > self.max_chars || block.lines.len() > self.max_lines` に変更。
2. CJK 判定ヘルパ:
   ```
   fn cjk_ratio(text: &str) -> f64  // CJK 統合漢字/かな/ハングル等の Unicode ブロック該当率
   fn is_cjk_heading_like(text: &str) -> bool
       // cjk_ratio > 0.5 && chars<=cfg.max_chars && 行末が '。'/'．'/'.' でない
   ```
   Unicode ブロック判定はコードポイント範囲（CJK Unified `\u{4E00}-\u{9FFF}`, ひらがな `\u{3040}-\u{309F}`, カタカナ `\u{30A0}-\u{30FF}`, ハングル `\u{AC00}-\u{D7A3}` 等）で行う（外部クレート追加は避け、`char` 範囲マッチ）。
3. スコアリング（`:130-148`）に加点を追加: `if is_cjk_heading_like(text) { score += 1; }`（`is_all_caps` が効かない言語の代替シグナル）。閾値は `HeaderDetectionConfig` の新 `#[serde(default)]` フィールド `cjk_heading_bonus: u32`（既定 1）とし直書きしない（方針書 §1-6）。
4. 既知名リストの設定化:
   - `HeaderDetectionConfig` に `#[serde(default = "default_known_section_names")] pub known_section_names: Vec<String>` を追加。
   - `default_known_section_names()` は現行 `KNOWN_SECTION_NAMES`（`:7-34`）の英語エントリ + 日本語（「概要」「序論」「はじめに」「関連研究」「手法」「提案手法」「実験」「評価」「結果」「考察」「議論」「結論」「まとめ」「参考文献」「謝辞」「付録」）を含む。大小/全角半角を正規化して比較。
   - `HeaderDetector` はこのリストを保持し `is_known_name` で参照（`const` 依存を撤廃）。
5. `is_known_name`（`:210-216`）の過剰 `contains` を是正: `normalize()`（後述）した上で **(a) 完全一致 or (b) 行が短く（chars<=既知名長+マージン）その既知名を含む** 場合のみ true。長文中の部分一致で発火しない。
6. `normalize(text)`: 前後空白除去 + 英字大文字化 + 全角英数の半角化（NFKC 相当の最小限。既存 `to_uppercase` を置換）。外部クレート `unicode-normalization` 導入可否は PR で確認（不要なら手書きの全角→半角テーブルで足りる範囲に留める）。
7. `block_classifier.rs:264-269` の英語専用 `is_known_section_name` も同じ多言語リストを参照させたいが、**本タスクのスコープは header_detector 側**。classifier 側の共有はリスト定数を `pub` 化して参照する軽微変更に留め、過剰改変しない（迷えば PR 質問）。

**シグネチャ変更:**
- `HeaderDetectionConfig` に `known_section_names: Vec<String>` と `cjk_heading_bonus: u32` 追加（両方 `#[serde(default)]`）。
- `HeaderDetector` が既知名リストをフィールド保持（`new`/`with_config` で受領）。`is_known_name`/`is_all_caps` は内部のまま。

**後方互換:**
- 長さフィルタの意味変更（バイト→文字）は**意図した仕様変更**。英語 ASCII 見出しは 1 char = 1 byte なので既存テスト `long_text_excluded`（`"A".repeat(121)`, `:303-309`）は緑のまま（121 文字 > 120）。
- 既定リストに日本語を足しても英語判定は不変。`known_section_names` を空にすれば既知名加点だけ無効化でき、旧英語限定挙動は既定リストを英語のみに差し替えて再現可能。
- 新 config フィールドは `#[serde(default)]`。

**受け入れ基準（Given/When/Then）:**
- Given 40 文字の日本語見出し（120 バイト超）、When `detect`、Then 長さフィルタで落ちない。
- Given 「関連研究」太字ブロック、When `detect`、Then CJK 見出しシグナル + 既知和名で見出し検出。
- Given 本文中に「method」を含む長い英文段落、When `is_known_name`、Then 境界条件で false（S6 の過剰発火抑制）。
- Given `known_section_names` に「提案手法」を追加した config、When 該当見出し、Then 検出される（拡張可能性）。
- Given ASCII 121 文字、When `detect`、Then 従来通り除外（回帰なし）。

**追加テスト:**
- `header_detector.rs` ユニット: `char_count_filter_allows_long_cjk_heading`, `cjk_heading_detected`, `known_name_no_overfire_in_long_body`, `configurable_known_names_extend`, `ascii_length_regression_unchanged`。
- ゴールデン: **日本語 A4 論文**フィクスチャで、主要見出し（序論/手法/実験/結論等）が `toc` に現れる再現率を測定（新規回帰の中心）。IEEE/arXiv は英語見出しが不変であることを回帰確認。

**非スコープ:** `detect_body_font_size`（`block_classifier.rs:291`）のバイト重み付け是正は本タスク外（提案書 P0/別途）。縦書き・CID 抽出は Phase 4。数式は対象外。

**検証:**
- `cargo test -p pdf-lay-core header_detector`
- 手動: `cargo run -p pdf-lay-cli -- toc tests/fixtures/ja_a4.pdf` で日本語見出しが並ぶことを確認。

---

### P1-6: no-confident-header フォールバック

**目的:** 確信できる見出しが 1 つも検出されないとき、文書全体を 1 セクションに潰さず、フォント変化・段落境界でセグメント化して複数セクションに分割する。

**重大度 / 依存:** HIGH（提案書 S1） / P1-3（フォントランク基盤を流用）。

**対象ファイル:**
- `crates/pdf-lay-core/src/structure/section_builder.rs:36-71`（`split_by_headers`）, `:62-68`（no-header 時の単一セクション化）, `:111-159`（`build_hierarchy`）
- `crates/pdf-lay-core/src/config.rs`（フォールバック閾値）

**現状（問題）:**
`section_builder.rs:36-71` の `split_by_headers` は、`headers` が空だと for ループ内で 1 度も分割されず、`:62-68` の「Final section」で**全ブロックが 1 つの `FlatSection`** になる:
```
sections.push(FlatSection {
    header: current_header.cloned(),   // None
    blocks: current_blocks,            // 全ブロック
    ...
});
```
ユニットテスト `flat_document_no_headers`（`:204-211`）が「headers 無し → 1 セクション」を仕様として固定しており、以降 `select_sections`（名前選択）が何も返せない（S1）。CJK・書籍・見出しフォント差が乏しい PDF で頻発する。

**変更後の期待動作:**
`SectionBuilder::build` は、**確信見出し数が 0**（または `< min_confident_headers`）のとき、`split_by_headers` の代わりに**フォールバック・セグメンタ**を使う。セグメンタは読み順ソート済みブロック列を、
(a) フォントサイズの上方シフト（次ブロックが直前本文より有意に大きい/太い＝疑似見出し）または
(b) 大きな段落間ギャップ（`page` 跨ぎ・空行相当）
で切り、各セグメント先頭を「見出しなし（`header=None`）だが独立した」`FlatSection` にする。テキストは 1 つも捨てない（全ブロックがいずれかのセグメントに入る＝No Silent Drop）。

**実装手順:**
1. `build`（`:16-34`）で分岐:
   ```
   let flat = if headers.len() >= cfg.min_confident_headers {
       Self::split_by_headers(&blocks, headers)
   } else {
       Self::segment_without_headers(&blocks, body_font_size, &cfg)
   };
   ```
   `body_font_size` は呼び出し側（pipeline）から渡す必要があるため、`build` シグネチャに `body_font_size: f64` と `cfg: &HeaderDetectionConfig`（または専用 `SectionFallbackConfig`）を追加。pipeline は `classifier.body_font_size` を渡す。
2. `segment_without_headers(blocks, body_font_size, cfg) -> Vec<FlatSection>`:
   - 読み順ソート済み前提（`build` 冒頭 `:24` で既に `ReadingOrderSorter::sort`）。
   - 走査し境界判定:
     - フォントシフト境界: `block.primary_font_size() >= body_font_size * cfg.fallback_font_shift_ratio`（既定 1.15）**かつ**直前ブロックが本文サイズ相当、または `block.is_bold() && !prev.is_bold()`。
     - ページ/段落境界: `block.page != prev.page` は境界候補（ただし単独では弱シグナル。フォントシフトと併用）。
   - 境界ごとに新 `FlatSection`（`header=None`）。境界ブロックが疑似見出しなら、その 1 行を `SectionHeader` を作らず `header=None` のまま**セクション先頭ブロック**として保持（テキストは残す）。
   - どのブロックも必ずいずれかのセグメントに属する不変条件を保証（脱落 0）。
   - セグメントが 1 個も切れなかった場合は従来通り単一セクション（劣化しない）。
3. 分割が起きたことを `warnings` に記録: `PdfLayWarning::HeaderlessSegmentation { segments: usize }`（`error.rs` に追加、Display は件数のみ）。
4. `build_hierarchy`（`:111-159`）はフォールバック結果もそのまま処理可能（全 level=1 のフラット列 → 全て root）。必要なら疑似見出しセグメントに擬似 level を与えるが、**スコープを抑え全 level=1 のフラット構造**に留める（過剰設計回避）。
5. `config.rs` に `#[serde(default)]` フィールド追加: `min_confident_headers: usize`（既定 1）, `fallback_font_shift_ratio: f64`（既定 1.15）。`HeaderDetectionConfig` に同居か、新 `SectionFallbackConfig` を `Config` に追加（既存構造に合わせ後者が明快なら採用。迷えば `HeaderDetectionConfig` 拡張に統一）。

**シグネチャ変更:**
- `SectionBuilder::build(blocks, headers, figures, tables, layouts)` → `build(blocks, headers, figures, tables, layouts, body_font_size: f64, fallback_cfg: &...)`。pipeline（`:230`）とテストを追随。
- `PdfLayWarning` に `HeaderlessSegmentation { segments }` 追加。
- config に 2 フィールド追加。

**後方互換:**
- 既存ユニットテスト `flat_document_no_headers`（`:204-211`）は**仕様変更**により結果が変わる（1 → 複数になり得る）。合成ブロックはフォント差が無いため**境界が切れず 1 セクションのまま**なら緑を維持できる（`make_block` は全て同一フォント, `:175-190`）。実際そうなるので既存 assert は温存見込み。切れてしまう場合はテストを「フォント差が無ければ 1 セクション」を明示する形へ更新し、PR で仕様変更として説明（方針書 §3 チェックリスト）。
- `min_confident_headers=0` に設定すれば従来の「常に `split_by_headers`」挙動へ戻せる。

**受け入れ基準（Given/When/Then）:**
- Given 見出し 0 かつ 3 箇所でフォントが本文の 1.2 倍に上がる文書、When `build`、Then 3+1 個程度の複数セクションに分割され、`select_sections` で位置指定が効く。
- Given 見出し 0 かつ全ブロック同一フォント、When `build`、Then 単一セクション（劣化しない）だが `HeaderlessSegmentation` 警告は出さない（segments<=1）。
- Given フォールバック発火時、When 出力文字数を集計、Then 入力ブロックの全テキストが保持（脱落 0）。
- Given `min_confident_headers=1` 未満の見出し数（=0）、When `build`、Then フォールバック経路に入る。

**追加テスト:**
- `section_builder.rs` ユニット: `no_headers_font_shift_segments_document`（フォント差ありブロック列で複数セクション）, `no_headers_uniform_font_single_section`（差なしで 1 セクション・警告なし）, `fallback_preserves_all_blocks`（総ブロック数保存）。
- ゴールデン: 見出しフォント差の乏しい日本語 A4 で、`toc` が 1 行に潰れないこと。IEEE/arXiv（見出しあり）はフォールバックに入らず既存挙動不変を回帰。

**非スコープ:** 疑似見出しの level 階層化・番号推定はしない（全 level1）。見出し検出そのものの改善は P1-3/P1-4。

**検証:**
- `cargo test -p pdf-lay-core section_builder`
- 手動: `cargo run -p pdf-lay-cli -- toc tests/fixtures/ja_a4.pdf`（見出し弱い文書）で複数セクションになることを確認。

---

### P1-7: selector マッチモードとサブツリー丸呑み抑制

**目的:** セクション名選択に exact / word-boundary / normalized のマッチモードを導入し、親一致時のサブツリー自動丸呑みを既定オフにして、子セクションを個別選択できるようにする（現行のデフォルト挙動も温存）。

**重大度 / 依存:** MED（提案書 S7） / なし（他タスクと独立）。

**対象ファイル:**
- `crates/pdf-lay-core/src/selector/selector.rs:23-26`（`by_names`）, `:121-148`（`collect_by_names`、部分一致 `:131-137`、丸呑み `:139-144`）
- 呼び出し側: `crates/pdf-lay-core/src/selector/selector.rs:176-178`（`PaperDocument::select_sections`）, CLI・Python バインディング

**現状（問題）:**
`selector.rs:131-137`（無境界部分一致）:
```
let matches = names.iter().any(|name| {
    let upper_name = name.to_uppercase();
    header_upper == upper_name || clean_upper == upper_name
        || header_upper.contains(&upper_name) || clean_upper.contains(&upper_name)
});
```
`:139-144`（丸呑み）:
```
if matches {
    result.push(section);          // 子は Section::children 経由で丸ごと含まれる
} else {
    result.extend(Self::collect_by_names(&section.children, names));
}
```
問題:
- `contains` が無境界なので `"in"` が `INTRODUCTION` に、`"method"` が `METHODOLOGY` に誤爆する（S7）。
- 親が一致すると `push(section)` して `else` に入らず、**子を個別選択できない**（親を選ぶと必ずサブツリー全体）。逆に親を除いて子だけ、も不可能。

**変更後の期待動作:**
`MatchMode` を導入:
- `Exact`: 正規化後の完全一致。
- `WordBoundary`: 正規化後、単語境界での部分一致（`"method"` は `METHODOLOGY` に一致しない、`"related work"` は `II. RELATED WORK` に一致）。
- `Normalized`: 正規化（大小・全角半角・前後空白）後の完全一致（CJK 向け、P1-5 の `normalize` と共有）。
- 既定の後方互換モードとして従来の無境界 `Substring` も**残す**。
選択時、`include_subtree: bool` を指定可能にし、**既定オフ**（親一致でも子は自動追加しない、子は独立に走査され個別一致できる）。既存 `select_sections` は**従来挙動（Substring + 丸呑み）を保つ**新旧両立。

**実装手順:**
1. `selector`（または `config`）に:
   ```
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
   pub enum MatchMode { #[default] Substring, Exact, WordBoundary, Normalized }
   ```
2. `SectionSelector::by_names_with`（新規）:
   ```
   pub fn by_names_with(doc, names: &[&str], mode: MatchMode, include_subtree: bool) -> Self
   ```
   既存 `by_names`（`:23-26`）は `by_names_with(doc, names, MatchMode::Substring, true)` に委譲（**完全後方互換**）。
3. `collect_by_names` を `collect_by_names(sections, names, mode, include_subtree)` に拡張:
   - マッチ判定 `name_matches(header_upper, clean_upper, name, mode)`:
     - `Substring`: 現行 `:131-137` と同一。
     - `Exact`: `== upper_name`（正規化済み）。
     - `Normalized`: `normalize(header)==normalize(name)`（P1-5 の `normalize` を `crate` 内で共有）。
     - `WordBoundary`: 正規化文字列に対し、対象名を `\b` 相当（ASCII は `regex` 単語境界、CJK は前後が非同一スクリプト境界）で検索。実装は `Regex::new(&format!(r"(?i)\b{}\b", escape(name)))` を使い、`escape` は `regex::escape`。CJK には別途「完全一致 or 区切り文字境界」判定を用意（`\b` は CJK で機能しないため）。
   - 走査ロジック:
     ```
     for section in sections {
         let matched = name_matches(...);
         if matched { result.push(section); }
         if !matched || !include_subtree {
             result.extend(collect_by_names(&section.children, names, mode, include_subtree));
         }
     }
     ```
     → `include_subtree=false` のとき、親一致でも子を走査（子も一致すれば個別に入る）。`include_subtree=true`（既定 by_names）では従来どおり親一致で子走査を止め丸呑み。
   - **重複防止**: `include_subtree=false` で親も子も一致する場合、両方入る（意図通り）。ポインタ同一による重複は起きない設計。
4. 公開 API 追加（後方互換）:
   - `PaperDocument::select_sections_with(&self, names, mode: MatchMode, include_subtree: bool)`。既存 `select_sections`（`:176-178`）は不変。
5. CLI/Python: 既存呼び出しは `select_sections` のままで不変。新モードを露出する場合のみ CLI フラグ（例 `--match exact|word|normalized`, `--no-subtree`）と Python 引数を追加。**本タスクでは core API 追加までを必須**とし、CLI/Python 露出は「公開 API を壊さない範囲で追加」に留める（露出しないなら 3 面ビルドは維持される）。露出する場合は 3 面反映（方針書 §4）。

**シグネチャ変更:**
- 追加: `MatchMode` enum、`SectionSelector::by_names_with`、`PaperDocument::select_sections_with`。
- 既存 `by_names` / `select_sections` はシグネチャ不変（内部委譲のみ）。

**後方互換:**
- 既定モード `Substring` + `include_subtree=true` が現行と完全一致。既存テスト `select_by_name_partial_match`（`:253-259`）は緑。
- 新 API は追加のみ。CLI/Python を触らなければ 3 面ビルドは無傷。

**受け入れ基準（Given/When/Then）:**
- Given `["method"]` と `MatchMode::WordBoundary`、When 文書に `METHODOLOGY` のみ、Then 一致しない（誤爆抑制）。`["methodology"]` なら一致。
- Given 親 `METHODS` と子 `Data Collection`、`include_subtree=false`、`["Data Collection"]`、When 選択、Then 子のみ返る（親は非一致）。
- Given `["METHODS"]`, `include_subtree=false`、When 選択、Then 親のみ返り子は含まれない（丸呑みしない）。
- Given 既存 `select_sections(["RESULT"])`、When 選択、Then 従来どおり `RESULTS` が丸呑みで返る（後方互換）。
- Given `MatchMode::Normalized` と全角「ＲＥＳＵＬＴＳ」、When `["RESULTS"]`、Then 一致（正規化）。

**追加テスト:**
- `selector.rs` ユニット: `word_boundary_no_substring_overfire`, `select_child_only_without_subtree`, `select_parent_without_swallowing_children`, `exact_mode_requires_full_match`, `normalized_mode_fullwidth`, `legacy_by_names_unchanged`。既存 `make_doc`（`:233-251`）を再利用。
- ゴールデン: arXiv で `--match word` 相当の選択が意図した節のみ返すことを確認（CLI 露出時）。

**非スコープ:** レベル/ページ/述語セレクタ（`by_indices`/`by_level`/`by_pages`/`by_predicate`）は変更しない。ファジー検索は導入しない。

**検証:**
- `cargo test -p pdf-lay-core selector`
- CLI 露出時のみ: `cargo run -p pdf-lay-cli -- ...`（新フラグのスモーク）＋ Python `select_sections_with` の doctest。

---

### P1-8: `block_index` の `global_index` 直参照化

**目的:** `SectionHeader.block_index` に enumerate 位置ではなく `TextBlock.global_index` を直接格納・参照し、将来の並べ替え/フィルタで全ヘッダーが誤アンカー化する時限爆弾（S9）を除去する。

**重大度 / 依存:** LOW-MED（提案書 S9） / なし（P1-4 の安定キー前提。先行マージ推奨）。

**対象ファイル:**
- `crates/pdf-lay-core/src/structure/header_detector.rs:84-90`（`detect` の enumerate）, `:92`（`try_detect` の `block_index` 引数）, `:170-178`（`SectionHeader` 生成、`block_index: block_index`）
- `crates/pdf-lay-core/src/structure/section_builder.rs:36-46`（`header_at` を `block_index` でキー化、`:46` で `block.global_index` 参照）
- `crates/pdf-lay-core/src/types/text.rs:211-212`（`SectionHeader.block_index` の doc）

**現状（問題）:**
`header_detector.rs:84-90` は `enumerate()` の位置 `i` を `try_detect(block, i)` に渡し、`:177` で `block_index: block_index`（＝スライス内位置）を格納する。一方 `section_builder.rs:38-46`:
```
let header_at: HashMap<usize, &SectionHeader> =
    headers.iter().map(|h| (h.block_index, h)).collect();
...
if let Some(header) = header_at.get(&block.global_index) {  // global_index で引く
```
**格納は enumerate 位置、参照は `global_index`**。両者は「`detect` に渡すスライスの位置 == `global_index`」のときだけ偶然一致する。P1-1（候補フィルタ）や将来のブロック並べ替え/フィルタでスライスがずれると、全ヘッダーが誤アンカーになりセクション分割が崩壊する。`types/text.rs:211` の doc も「Index into the flat `TextBlock` array」と曖昧。

**変更後の期待動作:**
`SectionHeader.block_index` は常に **`TextBlock.global_index`** を格納する。`detect` は enumerate 位置ではなく `block.global_index` を渡す。`section_builder` は現行どおり `block.global_index` で引くため**参照側は不変**で、格納側の意味を一致させるだけで恒久的に正しくなる。

**実装手順:**
1. `header_detector.rs::detect`（`:84-90`）:
   ```
   blocks.iter()
     .filter(|b| ...P1-1 の eligibility...)
     .filter_map(|b| self.try_detect(b, b.global_index))   // enumerate 廃止
     .collect()
   ```
   （P1-1 とマージ順が前後する場合は、どちらでも `try_detect(block, block.global_index)` を渡す形に統一。）
2. `try_detect`（`:92`）の第 2 引数名を `block_index` → `global_index` に改名（意味を明示）。`:177` の `block_index: global_index` を格納。
3. `types/text.rs:211-212` の doc を「該当見出しブロックの `global_index`（安定 ID）。`TextBlock.global_index` と一致する。」に更新（方針書 §3「コメントと実装の一致」）。
4. `section_builder.rs:36-46` は不変（既に `global_index` 参照）。ただし doc コメント `:37`「Collect header block_indices」を「header の global_index」に更新。
5. 既存テスト `block_index_preserved`（`header_detector.rs:328-337`）は、`make_block`（`:233-253`）が `global_index: 0` 固定なので**修正が必要**: テストヘルパで `global_index` を可変にし、2 番目のブロックに `global_index=1` を設定して `headers[0].block_index == 1` を検証する形へ更新（enumerate ではなく global_index を検証する意味に変える）。

**シグネチャ変更:**
- `SectionHeader.block_index` の**意味変更**（型は `usize` のまま）。フィールド名は互換のため維持（`block_global_index` へ改名すると serde/CLI/Python 波及が大きい）。doc のみ更新。
- `try_detect` の引数名変更（private、外部影響なし）。

**後方互換:**
- フィールド名・型・serde 表現は不変（JSON キー `block_index` 維持）。値の意味が「安定 ID」に変わるが、現行は偶然一致していたため**既存の正しい入力では出力不変**。
- 破壊的挙動変更なし（むしろバグ予防）。

**受け入れ基準（Given/When/Then）:**
- Given `global_index` がスライス位置と異なるブロック列（例: フィルタで歯抜け）、When `detect`→`build`、Then 各見出しが正しいブロック位置でセクション分割される（誤アンカーなし）。
- Given 先頭が本文、2 番目（`global_index=1`）が見出し、When `detect`、Then `headers[0].block_index == 1`（＝ global_index）。
- Given 既存の正常入力（位置 == global_index）、When 全パイプライン、Then セクション構造は従来と一致（回帰なし）。

**追加テスト:**
- `header_detector.rs` ユニット: `block_index_is_global_index`（`global_index` を位置と食い違わせて検証）。既存 `block_index_preserved` を上記方針で更新。
- `section_builder.rs` ユニット: `header_anchored_by_global_index_not_position`（歯抜け `global_index` のブロック列で正しいセクション境界）。

**非スコープ:** `global_index` の採番自体（`BlockGrouper`）は変更しない。フィールド改名は行わない。

**検証:**
- `cargo test -p pdf-lay-core header_detector section_builder`
- `cargo clippy --all-targets --all-features -- -D warnings`

---

## フェーズ完了の定義（Phase 1 DoD）

本フェーズは、方針書 §4 の共通 DoD に加えて次を**すべて**満たしたとき完了とする。

1. **P1-1 〜 P1-8 が各々 1 タスク = 1 PR で個別マージ済み**であり、各タスクの受け入れ基準（Given/When/Then）が全て満たされている。
2. 各タスクの**追加テストが存在し緑**、かつ既存テストが緑（本書で「意図した仕様変更」と明記した箇所は PR で正当性を説明済み）。
3. `cargo fmt --all --check` と `cargo clippy --all-targets --all-features -- -D warnings` が緑。
4. **分類器の結果がヘッダー検出に供給されている**（P1-1）ことがコードとテストで確認できる。`HeaderDetector::detect` が `Caption`/`PageNumber`/`RunningHeader`/`RunningFooter`/`Footnote`/`Reference` を候補から外す。
5. **反復ヘッダー/フッター除去が pipeline で走る**（P1-2）。走りヘッダが `toc` の見出しに現れない。
6. **レベル決定が文書大域フォントクラスタ + 番号ラティス**で行われ、孤立閾値 `1.1`/`1.15` が撤廃されている（P1-3/P1-4）。番号は構造タプルで保持され、単調性/スキップ/重複が `warnings` に記録される。
7. **日本語 A4 フィクスチャ**で見出しが `chars()` 基準・CJK シグナル・多言語既知名により検出される（P1-5）。バイト長由来の脱落が起きない。
8. **見出しゼロでも文書が 1 セクションに潰れない**（P1-6）。フォールバック時に全テキストが保持される（No Silent Drop）。
9. **selector に exact/word-boundary/normalized モードと丸呑み抑制**があり、既存 `select_sections` の挙動は不変（P1-7）。
10. **`SectionHeader.block_index` が `global_index` を直接格納**し、位置結合が除去されている（P1-8）。
11. **3 種のゴールデンフィクスチャ（IEEE 2 段組・arXiv preprint・日本語 A4）**で、見出しの適合率/再現率が Phase 0 時点から**悪化していない**（方針書 §8.3 の回帰基準）。フィクスチャ整備は横断タスク X-1 に依存するが、未整備なら該当ゴールデンを `#[ignore]` とし、その旨を各 PR に明記して合成データのユニットで代替する。
12. 触れた公開 API（`HeaderDetector` / `SectionSelector` / `SectionHeader` / `HeaderDetectionConfig` / 新 config フィールド）の変更が **CLI / Python / `pdf-lay` crate の 3 面**に反映され、ビルドが全面的に緑。
13. すべての新規閾値（クラスタビン幅・`max_level`・`cjk_heading_bonus`・`min_confident_headers`・`fallback_font_shift_ratio` 等）が `Config`/`*Config` に `#[serde(default)]` 付きで追加され、コード直書きが無い（方針書 §1-6）。
14. 追加した `PdfLayWarning` variant（`RepeatedRunningReclassified`・`SectionNumberingAnomaly`・`HeaderlessSegmentation`）が内容テキストを漏らさず件数/種別のみを表示する（`error.rs` の既存方針に一致）。
