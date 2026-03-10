# Phase 2 実装プラン: テーブル・数式・品質向上

**Version:** 1.0
**Date:** 2026-03-06
**Status:** Draft
**Base branch:** `feature/phase2-implementation`

---

## 1. Overview

### 1.1 ゴール

Phase 1（コアパイプライン）で構築したテキスト抽出→レイアウト解析→セクション構築のパイプライン上に、以下の3つの主要機能を追加する：

1. **テーブル検出・変換** — 罫線ベース＋テキスト整列ベースの2段階検出、Markdown/CSV変換
2. **数式検出・変換** — フォント・シンボル検出、上付き/下付き、LaTeX/Unicode変換
3. **品質向上** — メタデータ抽出、テスト拡充、複数ジャーナル形式対応

### 1.2 成果物

- `table/` モジュール（4ファイル: mod.rs, detector.rs, grid_builder.rs, text_converter.rs）
- `math/` モジュール（4ファイル: mod.rs, detector.rs, converter.rs, symbol_map.rs）
- `structure/metadata.rs` — タイトル・著者・DOI メタデータ抽出
- パイプライン統合、テスト拡充、`detect_tables: true` デフォルト化

### 1.3 アーキテクチャ原則

- **既存パターン踏襲**: `figure/` モジュールの構成パターン（mod.rs = pure re-export）に従う
- **型の不変性**: 既定義の `TableInfo`, `TableRepresentation`, `PathObject` をそのまま使用
- **段階的有効化**: `detect_tables: false` のまま開発を進め、全機能完成後に `true` に切り替え
- **レンダー時処理**: 数式はパイプラインの型に影響を与えず、出力レンダー時に変換する

---

## 2. 新モジュール構成

### 2.1 table/ モジュール

```
crates/pdf-lay-core/src/table/
├── mod.rs               # pub use 再エクスポートのみ
├── detector.rs          # TableDetector — 領域検出（rule-based + text-alignment）
├── grid_builder.rs      # GridBuilder — セル境界推定 → TableGrid 構築
└── text_converter.rs    # TableTextConverter — TableGrid → TableRepresentation 変換
```

### 2.2 math/ モジュール

```
crates/pdf-lay-core/src/math/
├── mod.rs               # pub use 再エクスポートのみ
├── detector.rs          # MathDetector — フォント・シンボルベースの数式スパン検出
├── converter.rs         # MathConverter + MathFormatter — LaTeX/Unicode/Plain変換
└── symbol_map.rs        # to_latex_map(), to_unicode_map(), math_symbols()
```

### 2.3 その他の新規ファイル

| ファイル | 目的 |
|---------|------|
| `structure/metadata.rs` | `MetadataExtractor` — タイトル・著者・DOI をブロック列から抽出 |

### 2.4 変更対象ファイル

| ファイル | 変更内容 |
|---------|---------|
| `pipeline.rs` | Phase 5.5 (Table Detection) 追加、メタデータ抽出呼び出し |
| `lib.rs` (core) | `pub mod table;` `pub mod math;` 追加、型の再エクスポート |
| `lib.rs` (pdf-lay) | 新しい型の再エクスポート |
| `types/mod.rs` | 必要に応じ新型の再エクスポート |
| `extract/pdf_reader.rs` | `extract_paths()` のスタブ解除（実装） |
| `output/markdown.rs` | 数式のインラインレンダリング対応 |
| `selector/llm_text.rs` | 数式のインラインレンダリング対応 |
| `config.rs` | `detect_tables` デフォルトを `true` に変更（最終タスク） |
| `structure/mod.rs` | `pub mod metadata;` `pub use metadata::MetadataExtractor;` 追加 |

---

## 3. タスク分解

### 凡例

- **サイズ**: S (< 2h), M (2-4h), L (4-8h), XL (8h+)
- **依存**: 先行タスクのID列
- **受入条件**: タスク完了時に満たすべき条件

---

### Phase 2A: テーブルモジュール基盤 (P2-01 〜 P2-10)

#### P2-01: table/mod.rs スケルトン作成
- **サイズ**: S
- **依存**: なし
- **ファイル**: `table/mod.rs`, `lib.rs` (core)
- **内容**: `table/` ディレクトリ作成、mod.rs に空のサブモジュール宣言、`lib.rs` に `pub mod table;` 追加
- **受入条件**: `cargo check` が通る。`table` モジュールが `lib.rs` からアクセス可能

#### P2-02: extract_paths() スタブ解除
- **サイズ**: L
- **依存**: なし
- **ファイル**: `extract/pdf_reader.rs`
- **内容**: `pdf_oxide` のページコンテントストリームからパスオブジェクト（line/rect）を抽出。`PathObject` + `PathType` (Horizontal/Vertical/Rectangle/Other) に分類
- **受入条件**: テストPDFの罫線ありページで `Vec<PathObject>` が非空を返す。`PathType` 分類が正しい。罫線なしページでは空Vec

#### P2-03: TableDetector — 罫線ベース検出 (detect_by_rules)
- **サイズ**: L
- **依存**: P2-01, P2-02
- **ファイル**: `table/detector.rs`
- **内容**: `PathObject` の水平線・垂直線からグリッドパターンを検出。交点解析で `TableRegion` を生成
- **受入条件**: 3本以上の平行な水平線 + 2本以上の垂直線がある領域を検出。ユニットテスト3件以上

#### P2-04: TableDetector — テキスト整列ベース検出 (detect_by_text_alignment)
- **サイズ**: L
- **依存**: P2-01
- **ファイル**: `table/detector.rs`
- **内容**: Table キャプション直下の短テキストブロック群のX座標アラインメントを検出。`min_columns` 以上のアラインメント → テーブル候補
- **受入条件**: キャプション付き罫線なしテーブルを検出。`column_alignment_tolerance` で許容誤差を制御。ユニットテスト3件以上

#### P2-05: TableDetector — キャプション対応付け + 統合
- **サイズ**: M
- **依存**: P2-03, P2-04
- **ファイル**: `table/detector.rs`
- **内容**: `CaptionDetector` から取得した `CaptionType::Table` キャプションと検出テーブル領域を空間的にマッチ。罫線ベースとテキスト整列ベースの重複排除
- **受入条件**: `detect()` が `Vec<TableRegion>` を返し、各 `TableRegion` にキャプション情報が紐付く。重複なし。ユニットテスト2件以上

#### P2-06: GridBuilder — セル境界推定 + グリッド構築
- **サイズ**: L
- **依存**: P2-03
- **ファイル**: `table/grid_builder.rs`
- **内容**: `TableRegion` + `TextBlock[]` からセル境界を推定（罫線ありの場合は交点ベース、なしの場合はX/Yクラスタリング）。テキストをセルに割当て `TableGrid` を構築
- **受入条件**: `TableGrid` の `column_count` が正しい。ヘッダー行の検出（太字/罫線区切り）。ユニットテスト3件以上

#### P2-07: GridBuilder — マルチヘッダー・結合セル対応
- **サイズ**: M
- **依存**: P2-06
- **ファイル**: `table/grid_builder.rs`
- **内容**: 複数行ヘッダー（スパンセル）の検出。`has_multi_header` フラグ設定
- **受入条件**: 2行ヘッダーのテーブルで `header` が正しく2行格納される。ユニットテスト2件以上

#### P2-08: TableTextConverter — Markdown/CSV変換
- **サイズ**: M
- **依存**: P2-06
- **ファイル**: `table/text_converter.rs`
- **内容**: `TableGrid` → `TableRepresentation::Markdown` / `Csv` / `PlainText` 変換。パイプエスケープ、セル内改行処理、マルチヘッダーフラット化
- **受入条件**: `to_markdown()` が正しいMarkdownテーブル文字列を生成。`to_csv()` が正しいCSV文字列を生成。ユニットテスト4件以上

#### P2-09: パイプライン統合 — Phase 5.5 Table Detection
- **サイズ**: M
- **依存**: P2-05, P2-08
- **ファイル**: `pipeline.rs`
- **内容**: Phase 5 (Figure Matching) と Phase 6 (Section Assembly) の間に Table Detection フェーズを挿入。`detect_tables` フラグでガード。`CaptionType::Table` キャプション → `TableDetector` → `GridBuilder` → `TableTextConverter` → `Vec<TableInfo>` を生成し、`SectionBuilder::build()` の `vec![]` を置換
- **受入条件**: `detect_tables: true` の時に `TableInfo` がセクションに含まれる。`false` の時は従来通り空。既存テスト全パス

#### P2-10: テーブル統合テスト
- **サイズ**: M
- **依存**: P2-09
- **ファイル**: `tests/integration/smoke_test.rs`, `test_helpers.rs`
- **内容**: テーブル検出のEnd-to-End統合テスト。`test_helpers.rs` に `make_table_info()` ヘルパー追加。合成データでのパイプラインテスト
- **受入条件**: Markdown出力にテーブルが含まれる。LLMテキスト出力にテーブルが含まれる。合成データテスト + fixtureテスト（#[ignore]）

---

### Phase 2B: 数式モジュール (P2-11 〜 P2-19)

#### P2-11: math/mod.rs スケルトン作成
- **サイズ**: S
- **依存**: なし
- **ファイル**: `math/mod.rs`, `lib.rs` (core)
- **内容**: `math/` ディレクトリ作成、mod.rs に空のサブモジュール宣言、`lib.rs` に `pub mod math;` 追加
- **受入条件**: `cargo check` が通る

#### P2-12: symbol_map.rs — シンボルマッピングテーブル
- **サイズ**: M
- **依存**: P2-11
- **ファイル**: `math/symbol_map.rs`
- **内容**: `to_latex_map()`, `to_unicode_map()`, `math_symbols()` の3関数。Greek文字、演算子、関係記号、矢印の対応テーブル。設計書 §2.9 の仕様通り
- **受入条件**: 30個以上のLaTeX変換マッピング。Unicode上付き/下付き数字(0-9)。`math_symbols()` が検出用 `HashSet<char>` を返す。ユニットテスト3件以上

#### P2-13: MathDetector — フォント・シンボル検出
- **サイズ**: M
- **依存**: P2-12
- **ファイル**: `math/detector.rs`
- **内容**: `is_math_span()` — フォント名パターン(CM*, Math, Symbol, MT*, STIX) + シンボルコードポイントで判定。`additional_math_fonts` 設定対応
- **受入条件**: CM フォントスパンを数式と判定。通常テキストフォントを非数式と判定。ユニットテスト4件以上

#### P2-14: MathDetector — インライン数式検出 (detect_in_line)
- **サイズ**: M
- **依存**: P2-13
- **ファイル**: `math/detector.rs`
- **内容**: `TextLine` 内の連続する数式スパンを `MathRegion` (Inline) としてグルーピング
- **受入条件**: 行内の部分数式を検出（例: "where $x = 5$"）。行全体が数式の場合は Display コンテキスト。ユニットテスト3件以上

#### P2-15: MathDetector — ディスプレイ数式検出 (detect_display_equations)
- **サイズ**: M
- **依存**: P2-14
- **ファイル**: `math/detector.rs`
- **内容**: 行全体が数式フォント + センタリング → `MathContext::Display`。行末の式番号 "(1)" パターン検出
- **受入条件**: センタリングされた数式行を Display として検出。式番号を `equation_number` に格納。ユニットテスト3件以上

#### P2-16: MathConverter — LaTeX変換
- **サイズ**: L
- **依存**: P2-12, P2-14
- **ファイル**: `math/converter.rs`
- **内容**: `to_latex()` — ベースライン検出、上付き `^{}` / 下付き `_{}` 生成、`symbol_to_latex` マッピング適用。`superscript_y_threshold` 設定対応
- **受入条件**: 上付き文字が `^{text}` に変換。下付き文字が `_{text}` に変換。ギリシャ文字が `\alpha` 等に変換。ユニットテスト4件以上

#### P2-17: MathConverter — Unicode/PlainText変換 + Auto判定
- **サイズ**: M
- **依存**: P2-16
- **ファイル**: `math/converter.rs`
- **内容**: `to_unicode()` — Unicode上付き/下付き文字変換。`to_plain()` — ASCII近似。`auto_convert()` — CMフォント→LaTeX、その他→Unicode
- **受入条件**: Unicode変換で `²` `₂` 等が正しく出力。Auto判定が CM → LaTeX、非CM → Unicode。ユニットテスト3件以上

#### P2-18: MathFormatter + レンダー統合
- **サイズ**: M
- **依存**: P2-17
- **ファイル**: `math/converter.rs`, `output/markdown.rs`, `selector/llm_text.rs`
- **内容**: `MathFormatter::format_for_markdown()` — インライン `$...$`、ディスプレイ `$$...$$`。Markdown/LLMテキストの出力時にブロックテキスト内の数式スパンを変換
- **受入条件**: Markdownにインライン数式 `$\alpha$` が埋め込まれる。ディスプレイ数式が `$$...\tag{1}$$` で出力。`math_config.representation` 設定が反映。ユニットテスト3件以上

#### P2-19: 数式統合テスト
- **サイズ**: M
- **依存**: P2-18
- **ファイル**: `tests/integration/smoke_test.rs`, `test_helpers.rs`
- **内容**: 数式検出・変換のEnd-to-End統合テスト。`test_helpers.rs` に `make_math_span()` ヘルパー追加
- **受入条件**: 合成データで数式がMarkdown/LLMテキストに正しく変換。LaTeX/Unicode/PlainText各モード動作確認

---

### Phase 2C: 品質向上 (P2-20 〜 P2-27)

#### P2-20: MetadataExtractor — タイトル・著者抽出
- **サイズ**: M
- **依存**: なし
- **ファイル**: `structure/metadata.rs`, `structure/mod.rs`
- **内容**: セクション分割前のブロック列から、タイトル（最大フォントサイズ・上部位置）と著者名（タイトル直下の中サイズテキスト）を抽出して `DocumentMetadata` を充填
- **受入条件**: IEEE形式でタイトル・著者を抽出。著者が複数の場合 `Vec<String>` に分割。ユニットテスト3件以上

#### P2-21: MetadataExtractor — DOI抽出
- **サイズ**: S
- **依存**: P2-20
- **ファイル**: `structure/metadata.rs`
- **内容**: ブロックテキストから DOI パターン (`10.xxxx/...`) を正規表現で検出
- **受入条件**: DOI付き論文で `metadata.doi` が `Some(...)` を返す。DOIなしの場合は `None`。ユニットテスト2件以上

#### P2-22: パイプライン統合 — メタデータ抽出
- **サイズ**: S
- **依存**: P2-20, P2-21
- **ファイル**: `pipeline.rs`
- **内容**: Phase 4 (Structure) 後に `MetadataExtractor` を呼び出し、`PaperDocument.metadata` を充填（現在は `pages` のみ設定）
- **受入条件**: `metadata.title`, `metadata.authors` が非空。既存テスト全パス

#### P2-23: ヘッダー/フッター除去の改善
- **サイズ**: M
- **依存**: なし
- **ファイル**: `structure/block_classifier.rs`
- **内容**: Running header/footer の検出精度向上。複数ページで繰り返されるテキストパターンの検出（前方一致 + ページ番号パターン除外）
- **受入条件**: 3ページ以上で同一テキストが出現するブロックを `RunningHeader`/`RunningFooter` と判定。ユニットテスト2件以上

#### P2-24: test_helpers 拡充
- **サイズ**: S
- **依存**: なし
- **ファイル**: `test_helpers.rs`
- **内容**: `make_table_info()`, `make_math_span()`, `make_path_object()`, `make_caption_info()` ヘルパー追加
- **受入条件**: テーブル・数式モジュールのテストで使用可能。各ヘルパーが最小限の有効データを生成

#### P2-25: 統合テスト拡充
- **サイズ**: L
- **依存**: P2-10, P2-19, P2-22
- **ファイル**: `tests/integration/smoke_test.rs`
- **内容**: フルパイプラインの統合テスト拡充。テーブル + 数式 + メタデータの複合テスト。合成 `PaperDocument` で全出力フォーマット検証
- **受入条件**: Markdown/JSON/LLMテキスト/Chunk の全出力にテーブル・数式が含まれる。メタデータがJSON出力に反映

#### P2-26: lib.rs 再エクスポート整理
- **サイズ**: S
- **依存**: P2-09, P2-18
- **ファイル**: `lib.rs` (core), `lib.rs` (pdf-lay)
- **内容**: 新しい公開型 (`TableDetector`, `GridBuilder`, `TableTextConverter`, `MathDetector`, `MathConverter`, `MetadataExtractor` 等) の再エクスポート追加
- **受入条件**: `pdf-lay` クレートから全新規公開型がアクセス可能。`cargo doc` が警告なしで完了

#### P2-27: detect_tables デフォルト有効化
- **サイズ**: S
- **依存**: P2-25
- **ファイル**: `config.rs`
- **内容**: `Config::default()` の `detect_tables` を `true` に変更。既存テスト調整
- **受入条件**: `Config::default()` で `detect_tables == true`。全テストパス（fixture テスト含む）。CI green

---

## 4. Table Module 設計

### 4.1 検出戦略

テーブル検出は **2段階** で行い、精度の高い方法を優先する：

```
Step 1: 罫線ベース検出 (detect_by_rules)
  PathObject[] → 水平/垂直線フィルタ → グリッドパターン検出 → TableRegion[]
  ※ 最も精度が高い。IEEE等の罫線テーブルに有効

Step 2: テキスト整列ベース検出 (detect_by_text_alignment)
  TextBlock[] + CaptionInfo[] → Table キャプション直下探索 → X座標アラインメント → TableRegion[]
  ※ 罫線なしテーブルのフォールバック。Step 1 との重複排除あり
```

### 4.2 TableRegion（内部型）

```rust
/// テーブル検出の中間表現（table/detector.rs 内部）
struct TableRegion {
    bbox: Rect,
    page: u32,
    block_indices: Vec<usize>,       // テーブル領域に含まれるブロック
    caption: Option<CaptionInfo>,    // 対応するキャプション
    has_rules: bool,                 // 罫線で検出されたか
    horizontal_rules: Vec<PathObject>,
    vertical_rules: Vec<PathObject>,
}
```

### 4.3 TableGrid（内部型）

```rust
/// グリッド構造の中間表現（table/grid_builder.rs 内部）
pub struct TableGrid {
    pub header: Vec<Vec<String>>,  // ヘッダー行（複数行の可能性）
    pub rows: Vec<Vec<String>>,    // データ行
    pub column_count: usize,
    pub has_multi_header: bool,
}
```

### 4.4 パイプライン内の位置

```
Phase 5: Figure Matching  ← 既存
Phase 5.5: Table Detection ← 新規（detect_tables ガード付き）
Phase 6: Section Assembly  ← 既存（vec![] → detected tables）
```

### 4.5 キャプション再利用

`CaptionDetector` は既に `CaptionType::Table` キャプションを検出済み。`ImageMatcher::match_all()` は Figure キャプションのみ処理するため、Table キャプションは未消費の状態で残っている。

```rust
// pipeline.rs 内の統合イメージ
let table_captions: Vec<&CaptionInfo> = captions.iter()
    .filter(|c| c.caption_type == CaptionType::Table)
    .collect();
```

---

## 5. Math Module 設計

### 5.1 検出戦略

数式検出は **フォント名 + シンボルコードポイント** のヒューリスティック：

```
TextSpan.font_name matches:
  - /^CM[A-Z]{1,4}\d/i     (Computer Modern: CMMI10, CMR12, CMSY10...)
  - /math/i                 (MathematicalPi, TimesNewRomanMT-Math...)
  - /symbol/i               (Symbol, ZapfDingbats...)
  - /^MT/i                  (MathTime)
  - /STIX/i                 (STIX fonts)
  - additional_math_fonts   (ユーザー設定)

OR TextSpan.text contains chars in math_symbols() set
```

### 5.2 インライン vs ディスプレイ判定

| 条件 | 判定 |
|------|------|
| 行内の一部が数式スパン | **Inline** |
| 行全体が数式スパン + センタリング | **Display** |
| 行全体が数式だが左寄せ | **Inline**（一応） |

### 5.3 上付き/下付き検出

```
Y offset = span.bbox.bottom - baseline_y
size_ratio = span.font_size / base_font_size

上付き: y_offset > base_size * threshold AND size_ratio < 0.85
下付き: y_offset < -(base_size * threshold) AND size_ratio < 0.85
```

`threshold` は `MathConfig.superscript_y_threshold` (default: 0.3)。

### 5.4 レンダー時統合

数式はパイプライン中で `TextSpan` → `MathRegion` への変換は行うが、`TextBlock.text` は変更しない。レンダー時（`MarkdownGenerator`, `LlmTextGenerator`）にブロック内スパンを走査して数式変換を適用する：

```rust
// output/markdown.rs の write_section 内
for block in &section.blocks {
    if math_enabled {
        let converted = math_converter.convert_block_text(block);
        md.push_str(&converted);
    } else {
        md.push_str(&block.text);
    }
}
```

### 5.5 MathRepresentation 出力形式

| 形式 | Inline出力 | Display出力 |
|------|-----------|-------------|
| LaTeX | `$\alpha + \beta$` | `$$\n\alpha + \beta \tag{1}\n$$` |
| UnicodeMath | `α + β` | `α + β (1)` |
| PlainText | `alpha + beta` | `alpha + beta (1)` |
| Auto | CM→LaTeX, その他→Unicode | 同左 |

---

## 6. Pipeline 統合

### 6.1 pipeline.rs 変更箇所

```rust
// ---- Phase 5: Figure Matching ---- (既存)
let caption_detector = CaptionDetector::new();
let captions = caption_detector.detect(&blocks);
let figures = image_matcher.match_all(&captions, &images, &blocks);

// ---- Phase 5.5: Table Detection ---- (新規)
let tables = if config.detect_tables {
    let paths = reader.extract_all_paths()?;  // 全ページのパス抽出
    let table_detector = TableDetector::new(config.table_config.clone());
    let table_captions: Vec<_> = captions.iter()
        .filter(|c| c.caption_type == CaptionType::Table)
        .collect();
    let regions = table_detector.detect(&blocks, &paths, &table_captions);
    let mut table_infos = Vec::new();
    for region in &regions {
        let grid = GridBuilder::build(region, &blocks);
        let repr = TableTextConverter::to_markdown(&grid);
        table_infos.push(TableInfo {
            table_id: format!("Table {}", region.caption.as_ref()
                .and_then(|c| c.number).unwrap_or(0)),
            table_number: region.caption.as_ref().and_then(|c| c.number),
            caption: region.caption.as_ref().map(|c| c.full_text.clone()),
            representation: repr,
            insertion_point: determine_insertion(&region, &blocks),
            page: region.page,
        });
    }
    table_infos
} else {
    vec![]
};

// ---- Phase 6: Section Assembly ---- (既存 — vec![] を tables に置換)
let sections = SectionBuilder::build(blocks, &headers, figures, tables, &layouts);

// ---- Metadata Extraction ---- (新規)
let metadata = MetadataExtractor::extract(&all_blocks_before_section, page_count);
```

### 6.2 extract_all_paths() 追加

`PdfReader` に全ページのパスを一括取得するヘルパーメソッドを追加：

```rust
pub fn extract_all_paths(&mut self) -> Result<Vec<PathObject>, PdfLayError> {
    let mut all = Vec::new();
    for page in 0..self.page_count() {
        all.extend(self.extract_paths(page)?);
    }
    Ok(all)
}
```

---

## 7. テスト戦略

### 7.1 ユニットテスト（各モジュール内 `#[cfg(test)]`）

| モジュール | テスト内容 | 件数目安 |
|-----------|-----------|---------|
| `table/detector.rs` | 罫線パターン検出、テキスト整列検出、キャプション対応 | 8+ |
| `table/grid_builder.rs` | セル割当、ヘッダー検出、マルチヘッダー | 5+ |
| `table/text_converter.rs` | Markdown変換、CSV変換、エスケープ処理 | 5+ |
| `math/symbol_map.rs` | マッピング正確性、双方向変換 | 3+ |
| `math/detector.rs` | フォント判定、インライン/ディスプレイ検出 | 6+ |
| `math/converter.rs` | LaTeX/Unicode/Plain変換、上付き/下付き | 7+ |
| `structure/metadata.rs` | タイトル抽出、著者分割、DOI検出 | 5+ |

### 7.2 統合テスト

| テスト | 対象 |
|-------|------|
| `test_table_in_markdown_output` | テーブル付き合成Doc → Markdown出力 |
| `test_table_in_llm_text_output` | テーブル付き合成Doc → LLMテキスト出力 |
| `test_math_in_markdown_output` | 数式付き合成Doc → Markdown出力（LaTeX/Unicode） |
| `test_metadata_in_json_output` | メタデータ付き合成Doc → JSON出力 |
| `test_full_pipeline_with_tables` | (#[ignore]) 実PDF → テーブル検出パイプライン |

### 7.3 test_helpers 追加ヘルパー

```rust
pub fn make_table_info(id: &str, number: u32, markdown: &str) -> TableInfo
pub fn make_math_span(text: &str, font_name: &str, left: f64, top: f64, font_size: f64) -> TextSpan
pub fn make_path_object(page: u32, left: f64, top: f64, right: f64, bottom: f64, path_type: PathType) -> PathObject
pub fn make_caption_info(block_index: usize, caption_type: CaptionType, number: u32, text: &str, page: u32) -> CaptionInfo
```

---

## 8. 品質目標

### 8.1 パフォーマンス

| メトリクス | 目標 |
|-----------|------|
| 12ページ論文の解析時間（テーブル有効） | < 5秒 |
| テーブル検出のオーバーヘッド | < 500ms |
| メモリ使用量増加 | < 20% |

### 8.2 精度

| メトリクス | 目標 |
|-----------|------|
| テーブル検出（罫線あり） | > 95% |
| テーブル検出（罫線なし） | > 80% |
| テーブル検出（全体） | > 90% |
| 数式スパン検出 | > 85% |
| 上付き/下付き検出 | > 80% |
| メタデータ抽出（タイトル） | > 90% |

### 8.3 CI要件

- `cargo test` 全パス（ユニット + 統合の always-run テスト）
- `cargo clippy -- -D warnings` 警告なし
- `cargo fmt --check` フォーマット準拠
- `cargo doc` 警告なし

---

## 9. 依存関係グラフ

```
P2-01 (table/mod.rs) ──┬── P2-03 (rule検出) ──┬── P2-05 (キャプション統合) ── P2-09 (pipeline)
                       │                      │
P2-02 (extract_paths) ─┘   P2-06 (grid) ──────┤
                                               │
P2-04 (text-align検出) ───────────────────────┘   P2-08 (text変換) ──────────── P2-09
                                                  │
                            P2-07 (multi-header) ──┘

P2-09 (pipeline table) ── P2-10 (table統合テスト) ── P2-25 (統合テスト拡充) ── P2-27 (default true)

P2-11 (math/mod.rs) ── P2-12 (symbol_map) ──┬── P2-13 (detector基盤)
                                             │
                                             ├── P2-16 (LaTeX変換)
                                             │
P2-13 ── P2-14 (inline検出) ── P2-15 (display検出) ── P2-18 (formatter統合)
                                                         │
P2-16 ── P2-17 (Unicode/Plain/Auto) ────────────────────┘
                                                         │
P2-18 ── P2-19 (math統合テスト) ── P2-25 (統合テスト拡充)

P2-20 (metadata title/author) ── P2-21 (DOI) ── P2-22 (pipeline metadata)
                                                   │
P2-22 ── P2-25 (統合テスト拡充)

P2-23 (header/footer改善) ── (独立、いつでも実行可能)
P2-24 (test_helpers拡充) ── (独立、他タスクと並行可能)
P2-26 (lib.rs整理) ── P2-09, P2-18 完了後
```

### クリティカルパス

```
P2-01 → P2-03 → P2-05 → P2-09 → P2-10 → P2-25 → P2-27
```

### 並行可能なストリーム

| ストリーム | タスク | 前提 |
|-----------|-------|------|
| **Table** | P2-01 → P2-03/P2-04 → P2-05 → P2-06/P2-08 → P2-09 → P2-10 | なし |
| **Math** | P2-11 → P2-12 → P2-13/P2-14 → P2-15/P2-16 → P2-17 → P2-18 → P2-19 | なし |
| **Quality** | P2-20 → P2-21 → P2-22 / P2-23 / P2-24 | なし |
| **Integration** | P2-25 → P2-26 → P2-27 | Table + Math + Quality 完了後 |

Table ストリームと Math ストリームは完全に並行して進められる。Quality ストリームも独立して開始可能。
