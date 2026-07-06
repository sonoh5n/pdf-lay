# pdf-lay: PDF Layout Analysis Library — 仕様書

**Version:** 0.1.0
**Date:** 2026-02-20
**Status:** Draft

---

## 1. プロジェクト概要

### 1.1 目的

学術論文PDFから構造化されたデータ（テキスト・テーブル・図）を抽出し、LLMによる情報抽出に最適化された中間表現を生成するライブラリを開発する。

### 1.2 プロジェクト名

**pdf-lay** (PDF Layout Analyzer)

### 1.3 提供形態

| 形態 | 対象ユーザー | 説明 |
|------|-------------|------|
| Rust crate (`pdf-lay`) | Rustアプリケーション開発者 | コアライブラリ。直接依存として利用 |
| Python package (`pdflay`) | データサイエンティスト、MLエンジニア | PyO3経由のPythonバインディング |
| CLI (`pdf-lay-cli`) | 全ユーザー | コマンドラインツール |

### 1.4 スコープ

**対象:**
- 学術論文PDF（IEEE, Elsevier, Springer, Nature, ACS, RSC 等の主要ジャーナル形式）
- 1段組・2段組・混在レイアウト
- 英語論文（第1フェーズ）、日本語論文（第2フェーズ）

**対象外:**
- スキャンPDF（画像のみのPDF）→ OCRは別途連携
- フォーム入力付きPDF
- DRM保護されたPDF

---

### 1.5 実装状況（Implemented vs Planned）

本表は「仕様が約束する表面」と「現時点の実装」の対応。✅=実装済み / 🅿=計画中（未実装）。
CLI 列は `cargo run -p pdf-lay-cli -- --help` / 各サブコマンド `--help` と、Python 列は
`crates/pdflay-python/src/lib.rs` の `#[getter]`/`#[pymethods]`/`#[pyfunction]` と照合済み。

| 面 | 項目 | 状態 | 実体 / 備考 |
|----|------|------|-------------|
| CLI | `pdf-lay toc <PDF>` | ✅ | `main.rs` `Commands::Toc` |
| CLI | `pdf-lay markdown <PDF>` | ✅ | `main.rs` `Commands::Markdown` |
| CLI | `markdown --section NAME`（繰り返し可） | ✅ | 単数・repeatable・部分一致。**`--sections "A,B"` というカンマ区切りフラグは無い** |
| CLI | `markdown --heading-offset / --no-page-numbers / --image-base / -o,--output / --image-dir / --no-extract-images` | ✅ | `main.rs` `MarkdownArgs` |
| CLI | `markdown/json/chunks/llm-text --math-format latex\|unicode\|plain\|off` | ✅ | 全サブコマンドで有効（Phase 0 P0-3 実装済み。仕様書旧版は「未実装」としていたが現在は実装済み） |
| CLI | `markdown --section-index` | 🅿 | 未実装 |
| CLI | `pdf-lay json <PDF>`（フルダンプ / `--content-only`） | ✅ | `main.rs` `Commands::Json`（旧仕様書は「Phase 2 P2-6待ち」としていたが実装済み） |
| CLI | `pdf-lay chunks <PDF>`（JSONL。`--strategy` / `--max-tokens` / `--overlap` / `--tokenizer` / `--no-section-context`） | ✅ | `main.rs` `Commands::Chunks` |
| CLI | `pdf-lay llm-text <PDF>`（`--section` / `--figure-format` / `--image-base` / `--no-figures` / `--no-tables`） | ✅ | `main.rs` `Commands::LlmText` |
| CLI | `pdf-lay analyze` / `pdf-lay debug-layout` / バッチ `--parallel` | 🅿 | 未実装・未計画（`AGENTS.md` の `debug-layout` 記述も未実装コマンドの前提） |
| Python | `pdflay.analyze(path, image_dir, extract_images, detect_tables)` | ✅ | `lib.rs` `fn analyze` |
| Python | `doc.paper_id / doc.pages / doc.title / doc.authors / doc.doi / doc.sections / doc.figures` | ✅ | `lib.rs` `PyPaperDocument`。**`doc.metadata.*` という属性は無い** |
| Python | `doc.to_markdown / doc.to_json / doc.to_chunks / doc.toc() / doc.select_sections*` | ✅ | `lib.rs` `PyPaperDocument` |
| Python | `PySectionEntry.page_start / page_end` | ✅ | `lib.rs` `PySectionEntry`。**`entry.page_range[0]/[1]` という属性は無い** |
| Python | `selector.to_markdown / to_json / to_llm_text / to_chunks / total_estimated_tokens` | ✅ | `lib.rs` `PySectionSelector`。**`selector.to_chunks` に `strategy` 引数は無い**（`max_tokens`/`overlap` のみ）。**`to_llm_text` に `include_section_headers` 引数は無い**（常に true 固定） |
| Python | `PyChunk.tables` getter | ✅ | `lib.rs` `PyChunk::tables`（旧仕様書は「Phase 2 P2-6待ち」としていたが実装済み） |
| Python | `pdflay.Config(...)` / `analyze(..., config=...)` | 🅿 | 未実装。`analyze()` の引数は `path, image_dir, extract_images, detect_tables` のみ |
| Python | `pdflay.extract_spans / reconstruct_lines / detect_layout / analyze_batch` | 🅿 | 未実装（モジュールに存在しない） |
| Python | `doc.to_chunks(...)` / `selector.to_chunks(...)` の数式書式指定 | 🅿 | Python 側の chunk 生成は `math_config: None` 固定（数式は raw glyph のまま）。CLI の `pdf-lay chunks --math-format` のみ数式変換に対応 |
| Rust | `analyze_pdf(path, config)` / `analyze_pdf_bytes(bytes, config)` | ✅（戻り値注意） | 戻り値は `PaperDocument` ではなく `Result<AnalysisResult, PdfLayError>`（`AnalysisResult.document` 経由でアクセス） |
| Rust | 段階的API `extract_text_spans / reconstruct_lines / detect_layout / group_blocks / detect_sections / extract_images / match_figures` | 🅿 | `pdf-lay` crate から未再エクスポート（内部モジュール `extract`/`layout`/`structure`/`figure` のみに存在） |
| Rust | 自由関数 `to_markdown(doc, config)` / `to_json(doc)` / `to_chunks(doc, config)` | 🅿 | 存在しない。実際は `SectionSelector::to_markdown/to_json/to_chunks`（メソッド）または `MarkdownGenerator`/`JsonGenerator`/`Chunker`（構造体API、`pdf-lay` から再エクスポート済み） |
| 出力 | 数式変換（`--math-format` / `math_format=`） | ✅ | CLI 全経路（`markdown`/`json --content-only`/`chunks`/`llm-text`）で有効。Python は `to_markdown(math_format=...)` のみ対応、`to_chunks` は非対応（上記） |

> P2-6・Phase 0 P0-3 は本表作成時点で既にマージ済みであることを実 CLI/実 Python で確認した
> （`phase3_skills.md` 本文が書かれた時点の想定より実装が先行している）。

---

## 2. 機能要件

### 2.1 コア機能一覧

| ID | 機能名 | 優先度 | フェーズ |
|----|--------|--------|----------|
| F-001 | テキスト抽出（座標付き） | 必須 | 1 |
| F-002 | 行の再構成 | 必須 | 1 |
| F-003 | カラムレイアウト検出 | 必須 | 1 |
| F-004 | ブロック（段落）グルーピング | 必須 | 1 |
| F-005 | セクションヘッダー検出 | 必須 | 1 |
| F-006 | セクション階層構造の構築 | 必須 | 1 |
| F-007 | 画像抽出（座標付き） | 必須 | 1 |
| F-008 | キャプション検出・画像対応付け | 必須 | 1 |
| F-009 | Markdown生成（画像リンク埋め込み） | 必須 | 1 |
| F-010 | テーブル領域検出 | 必須 | 2 |
| F-011 | テーブルのMarkdown/CSV変換 | 高 | 2 |
| F-012 | テーブル画像切り出し | 高 | 2 |
| F-013 | ヘッダー/フッター除去 | 高 | 2 |
| F-014 | 数式領域検出 | 中 | 3 |
| F-015 | 参考文献セクション解析 | 中 | 3 |
| F-016 | メタデータ抽出（タイトル・著者・DOI） | 高 | 2 |
| F-017 | JSON中間表現出力 | 必須 | 1 |
| F-018 | LLM向けチャンク分割出力 | 高 | 2 |
| F-019 | セクション一覧取得・選択的出力 | 必須 | 1 |
| F-020 | テーブルのインラインテキスト表現 | 必須 | 1 |
| F-021 | 数式・LaTeX表現の検出と変換 | 高 | 2 |

### 2.2 F-001: テキスト抽出（座標付き）

**説明:** pdf_oxideを用いてPDFからテキストスパンを抽出する。各スパンにはテキスト内容、フォント情報、座標（bbox）を含む。

**入力:** PDFファイルパス or バイト列

**出力:**
```rust
struct TextSpan {
    text: String,
    font_name: String,
    font_size: f64,
    is_bold: bool,
    is_italic: bool,
    bbox: Rect,
    page: u32,
}
```

**制約:**
- PDF内のすべてのテキストグリフを抽出すること
- フォントのサブセットやエンコーディングの違いを適切に処理すること
- 座標はPDFデフォルト座標系（左下原点、ポイント単位）で返すこと

### 2.3 F-002: 行の再構成

**説明:** 個別のテキストスパンをY座標の近接性に基づいて論理行にグルーピングする。

**アルゴリズム:**
1. テキストスパンをY座標（top）で降順ソート
2. Y座標の差が `font_size × 0.5` 以内のスパンを同一行とする
3. 同一行内のスパンをX座標で昇順ソート
4. 行全体のバウンディングボックスを算出

**出力:**
```rust
struct TextLine {
    spans: Vec<TextSpan>,
    text: String,           // 結合済みテキスト
    bbox: Rect,
    page: u32,
    baseline_y: f64,
    primary_font_size: f64,
    primary_font_name: String,
    is_bold: bool,
}
```

**エッジケース:**
- 上付き文字・下付き文字（Y座標がベースラインからずれる）
- 行内の部分的な太字・イタリック
- 数式中のグリフ散在

### 2.4 F-003: カラムレイアウト検出

**説明:** ページ内のテキスト行の水平位置分布を分析し、1段組・2段組・混在レイアウトを検出する。

**アルゴリズム:**
1. ページをY方向に4〜6ゾーンに分割
2. 各ゾーン内の行のleft座標のヒストグラムを作成（ビン幅: 10pt）
3. ピーク検出（全行数の20%以上の出現頻度を閾値とする）
4. ピークのクラスタリング（ページ幅の15%以内の近接ピークを統合）
5. ゾーン間の結果を比較し、レイアウト遷移を検出

**出力:**
```rust
struct PageLayout {
    regions: Vec<LayoutRegion>,
    page_width: f64,
    page_height: f64,
}

struct LayoutRegion {
    y_top: f64,
    y_bottom: f64,
    columns: Vec<Column>,
}

struct Column {
    left: f64,
    right: f64,
    index: usize,
}
```

**対応パターン:**
- 1段組（全ページ）
- 2段組（全ページ）
- 混在（タイトル部分1段 + 本文2段）
- 全幅要素（図・テーブルが2カラムにまたがる場合）

### 2.5 F-004: ブロック（段落）グルーピング

**説明:** カラム内の行を段落単位のブロックにグルーピングする。

**ブロック区切り条件（OR）:**
1. 行間がフォントサイズの `1.2 × 1.8` 倍を超える
2. フォントサイズが1.0pt以上変化
3. Bold ↔ Regular のフォントウェイト変化
4. 左インデント位置が大きく変化（段落先頭のインデント）

**出力:**
```rust
struct TextBlock {
    global_index: usize,
    lines: Vec<TextLine>,
    text: String,
    bbox: Rect,
    page: u32,
    column_index: usize,
    block_type: BlockType,
}

enum BlockType {
    Title,
    Abstract,
    SectionHeader,
    SubsectionHeader,
    BodyText,
    Caption,
    ListItem,
    Equation,
    Footnote,
    PageNumber,
    RunningHeader,
    RunningFooter,
    Reference,
    Unknown,
}
```

### 2.6 F-005: セクションヘッダー検出

**説明:** テキストブロックの中からセクションヘッダーを検出し、レベルを付与する。

**検出パターン:**

| パターン | 例 | Level |
|---------|---|-------|
| ローマ数字 + ピリオド + 全大文字 | `II. KNOWLEDGE GRAPHS` | 1 |
| アラビア数字 + ピリオド + テキスト | `3. Methods` | 1 |
| 全大文字 + 太字 | `INTRODUCTION` | 1 |
| 既知セクション名 | `Abstract`, `CONCLUSION` | 1 |
| 英字 + ピリオド + テキスト | `A. Graph Construction` | 2 |
| 数字.数字 + テキスト | `3.1 Data Collection` | 2 |
| 数字.数字.数字 + テキスト | `3.1.1 Sampling` | 3 |
| 太字 + 1行 + 短文（<80文字） | `Experimental Setup` | 2 |

**既知セクション名リスト:**
```
ABSTRACT, INTRODUCTION, BACKGROUND, RELATED WORK,
METHOD, METHODS, METHODOLOGY, APPROACH,
EXPERIMENT, EXPERIMENTS, EXPERIMENTAL,
RESULTS, RESULT, RESULTS AND DISCUSSION,
DISCUSSION, ANALYSIS, CONCLUSION, CONCLUSIONS,
SUMMARY, REFERENCES, BIBLIOGRAPHY,
ACKNOWLEDGMENT, ACKNOWLEDGMENTS, APPENDIX,
SUPPLEMENTARY, SUPPORTING INFORMATION
```

**除外条件:**
- ブロック長 > 120文字
- ブロック行数 > 3行
- 数値のみ（ページ番号）
- フォントサイズがタイトル級（body_font_size × 1.8超）でかつ既知セクション名でない

### 2.7 F-006: セクション階層構造の構築

**説明:** 検出されたヘッダーとテキストブロックをセクション構造に組み立てる。

**出力:**
```rust
struct Section {
    header: Option<SectionHeader>,
    level: u8,
    blocks: Vec<TextBlock>,
    figures: Vec<FigureInfo>,
    tables: Vec<TableInfo>,
    children: Vec<Section>,
    page_range: (u32, u32),
}

struct SectionHeader {
    text: String,
    clean_text: String,   // 番号除去後
    level: u8,
    numbering: Option<String>,  // "II.", "3.1" etc.
    page: u32,
    bbox: Rect,
    block_index: usize,
}
```

### 2.8 F-007: 画像抽出（座標付き）

**説明:** PDFから埋め込み画像を抽出し、ファイルとして保存する。

**出力:**
```rust
struct ImageInfo {
    path: PathBuf,           // 保存先パス
    page: u32,
    raw_bbox: Rect,
    normalized_bbox: Rect,   // テキスト座標系に変換済み
    width_px: u32,
    height_px: u32,
    format: ImageFormat,     // PNG, JPEG, etc.
}
```

**座標正規化:**
- pdf_oxideの画像bboxとテキストbboxのスケール差を自動検出
- ページのメディアボックスを基準にスケールファクタを算出
- 正規化後の座標でテキストとの空間的関係を計算可能にする

### 2.9 F-008: キャプション検出・画像対応付け

**説明:** キャプションテキストブロックと画像を空間的にマッチングする。

**キャプション検出パターン:**
```regex
(?i)^(Fig\.?|Figure|TABLE|Tab\.)\s*(\d+)\s*[:.]?\s*(.*)
```

**マッチングアルゴリズム:**
1. キャプションブロックと同一ページの画像を検索
2. 画像のbottomとキャプションのtopの垂直距離を計算
3. 水平方向の中心位置の差異も考慮
4. 距離スコア: `vertical_dist × 10 + horizontal_dist`
5. スコア最小の画像をマッチ
6. 妥当性チェック: 垂直距離 < 50pt（約17mm）

**出力:**
```rust
struct FigureInfo {
    figure_id: String,
    figure_number: Option<u32>,
    caption_text: String,
    image: ImageInfo,
    context_text: String,    // 前後の本文テキスト（~500文字）
    insertion_point: InsertionPoint,
}

struct InsertionPoint {
    page: u32,
    after_block_index: Option<usize>,
    y_position: f64,
}
```

### 2.10 F-009: Markdown生成（画像リンク埋め込み）

**説明:** セクション構造・図・テーブルを統合したMarkdownを生成する。

**出力形式:**
```markdown
## Abstract

Large language models (LLMs) have shown strong potential...

## INTRODUCTION

The rapid evolution of large language models...

![Fig. 1](images/p002_img001.png)

*Fig. 1: Overview of the proposed KG-RAG framework...*

The framework illustrated in Figure 1 demonstrates...

## KNOWLEDGE GRAPHS FOR TELECOM

### Graph Construction

The construction of domain-specific knowledge graphs...
```

**変換ルール:**
- セクションレベル → Markdownヘッダーレベル（Level 1 → `##`, Level 2 → `###`）
- 図 → `![figure_id](相対パス)` + イタリックのキャプション
- テーブル → Markdownテーブル or `![table_id](相対パス)`
- PageNumber, RunningHeader, RunningFooter → 除外
- 段落間 → 空行

### 2.11 F-010: テーブル領域検出

**説明:** ページ内のテーブル領域を検出する。

**検出アプローチ（優先順）:**
1. 罫線ベース: PDFパスオブジェクト（水平線・垂直線）のグリッドパターン検出
2. テキスト座標ベース: X座標の列アラインメント検出（3列以上のアラインメント + "Table"キャプション近接）
3. フォールバック: "Table"キャプション検出 → 周辺領域を画像としてキャプチャ

### 2.12 F-017: JSON中間表現出力

**説明:** 解析結果をJSON形式で出力する。TypeScript側のエージェントとの連携に使用。

**スキーマ:**
```json
{
  "version": "0.1.0",
  "paper_id": "string",
  "source_file": "path/to/paper.pdf",
  "metadata": {
    "title": "string | null",
    "authors": ["string"],
    "pages": 12
  },
  "layout": {
    "pages": [
      {
        "page_number": 1,
        "width": 612.0,
        "height": 792.0,
        "regions": [
          { "y_top": 792.0, "y_bottom": 650.0, "columns": 1 },
          { "y_top": 650.0, "y_bottom": 0.0, "columns": 2 }
        ]
      }
    ]
  },
  "sections": [
    {
      "header": "INTRODUCTION",
      "header_raw": "I. INTRODUCTION",
      "level": 1,
      "page_range": [1, 2],
      "text": "The rapid evolution...",
      "figures": [
        {
          "id": "Fig. 1",
          "number": 1,
          "caption": "Overview of the proposed...",
          "image_path": "images/p002_img001.png",
          "context": "As shown in Figure 1...",
          "page": 2
        }
      ],
      "tables": [],
      "subsections": [...]
    }
  ],
  "figures": [...],
  "tables": [...]
}
```

### 2.13 F-018: LLM向けチャンク分割出力

**説明:** LLMのコンテキストウィンドウに収まるサイズでセクションを分割し、各チャンクにメタデータを付与する。

**パラメータ:**
- `max_tokens`: チャンクの最大トークン数（デフォルト: 4000）
- `overlap_tokens`: チャンク間のオーバーラップ（デフォルト: 200）
- `split_strategy`: セクション境界優先 / トークン数優先

**出力:**
```json
{
  "chunks": [
    {
      "chunk_id": 0,
      "paper_id": "paper_001",
      "section": "INTRODUCTION",
      "page_range": [1, 2],
      "text": "...",
      "figures": [...],
      "tables": [...],
      "estimated_tokens": 3800,
      "has_continuation": false
    }
  ]
}
```

### 2.14 F-019: セクション一覧取得・選択的出力

**説明:** 解析済みドキュメントからセクション一覧（目次）を取得し、指定したセクションだけをMarkdown/テキスト/JSONとして出力する機能。LLMに渡すデータを最小化し、精度とコスト効率を両立する。

**ユースケース:**

1. まずセクション一覧を取得して構造を把握
2. データが含まれるセクション（Results, Experimental等）だけを選択
3. 選択したセクションのみをLLMに渡して情報抽出

**セクション一覧の出力形式:**

```rust
/// セクション目次エントリ（軽量）
pub struct SectionEntry {
    pub index: usize,            // セクション配列内のインデックス
    pub path: String,            // "2.1" or "INTRODUCTION" (一意識別子)
    pub header: String,          // クリーンなヘッダーテキスト
    pub header_raw: String,      // 生のヘッダーテキスト "II. KNOWLEDGE GRAPHS"
    pub level: u8,
    pub page_range: (u32, u32),
    pub estimated_tokens: usize, // テキストの概算トークン数
    pub has_figures: bool,
    pub figure_count: usize,
    pub has_tables: bool,
    pub table_count: usize,
    pub children: Vec<SectionEntry>,
}
```

**Python API:**

```python
doc = pdflay.analyze("paper.pdf")

# --- セクション一覧の取得 ---
toc = doc.toc()
for entry in toc:
    print(f"[L{entry.level}] {entry.header} (p.{entry.page_start}-{entry.page_end}, "
          f"~{entry.estimated_tokens} tokens, "
          f"fig:{entry.figure_count}, tab:{entry.table_count})")
    # 注: `entry.page_range[0]/[1]` という属性は無い。実 API は `page_start`/`page_end`
    # （`crates/pdflay-python/src/lib.rs` `PySectionEntry`）。
    for child in entry.children:
        print(f"  [L{child.level}] {child.header}")

# 出力例:
# [L1] Abstract (p.1-1, ~320 tokens, fig:0, tab:0)
# [L1] INTRODUCTION (p.1-2, ~1200 tokens, fig:0, tab:0)
# [L1] KNOWLEDGE GRAPHS FOR TELECOM (p.2-4, ~2400 tokens, fig:2, tab:1)
#   [L2] Graph Construction (p.2-3, ~800 tokens, fig:1, tab:0)
#   [L2] Integration with LLMs (p.3-4, ~1100 tokens, fig:1, tab:1)
# [L1] EXPERIMENTS (p.4-7, ~3200 tokens, fig:3, tab:2)
# [L1] RESULTS (p.7-9, ~2100 tokens, fig:2, tab:3)
# [L1] CONCLUSION (p.9-10, ~600 tokens, fig:0, tab:0)
# [L1] REFERENCES (p.10-12, ~1500 tokens, fig:0, tab:0)

# --- セクションの選択的出力 ---

# 方法1: ヘッダー名で選択（部分一致、大文字小文字無視）
selected = doc.select_sections(["RESULTS", "EXPERIMENTS"])
md = selected.to_markdown(image_base_path="./images")

# 方法2: インデックスで選択
selected = doc.select_sections_by_index([3, 4])
md = selected.to_markdown()

# 方法3: レベルでフィルタ（Level 1のみ = 大セクションだけ）
selected = doc.select_sections_by_level(level=1)

# 方法4: ページ範囲で選択
selected = doc.select_sections_by_pages(start=4, end=9)

# 方法5: 述語関数で選択
selected = doc.select_sections_where(
    lambda entry: entry.has_tables or entry.has_figures
)

# --- 選択結果からの出力 ---

# Markdown（選択セクションのみ）
md = selected.to_markdown(image_base_path="./images")

# JSON（選択セクションのみ）
json_str = selected.to_json()

# LLMに渡すためのプレーンテキスト（画像パスをプレースホルダに）
text = selected.to_llm_text(
    include_figures=True,     # 画像パスを [IMAGE: Fig. 1 path/to/img.png] として含める
    include_tables=True,      # テーブルをMarkdownテーブルとしてインライン表示
)
# 🅿 注意: 実 API に `include_section_headers` 引数は無い（セクション見出しは常に含まれる）。
# 渡すと TypeError になる。

# チャンク分割（選択セクションのみ）
chunks = selected.to_chunks(max_tokens=4000)

# 合計トークン数の確認
print(f"Total tokens: ~{selected.total_estimated_tokens()}")
```

**Rust API:**

```rust
// セクション一覧取得
pub fn toc(doc: &PaperDocument) -> Vec<SectionEntry>;

// セクション選択
pub struct SectionSelector<'a> {
    doc: &'a PaperDocument,
    selected: Vec<&'a Section>,
}

impl PaperDocument {
    pub fn toc(&self) -> Vec<SectionEntry>;

    pub fn select_sections(&self, names: &[&str]) -> SectionSelector;
    pub fn select_sections_by_index(&self, indices: &[usize]) -> SectionSelector;
    pub fn select_sections_by_level(&self, level: u8) -> SectionSelector;
    pub fn select_sections_by_pages(&self, start: u32, end: u32) -> SectionSelector;
    pub fn select_sections_where<F>(&self, predicate: F) -> SectionSelector
    where F: Fn(&SectionEntry) -> bool;
}

impl<'a> SectionSelector<'a> {
    pub fn to_markdown(&self, config: &MarkdownConfig) -> String;
    pub fn to_json(&self) -> Result<String, serde_json::Error>;
    pub fn to_llm_text(&self, config: &LlmTextConfig) -> String;
    pub fn to_chunks(&self, config: &ChunkConfig) -> Vec<Chunk>;
    pub fn total_estimated_tokens(&self) -> usize;
    pub fn sections(&self) -> &[&Section];
}
```

**CLI:**

```bash
# セクション一覧表示
pdf-lay toc paper.pdf

# 出力:
# [1] Abstract                      p.1     ~320 tokens
# [1] INTRODUCTION                  p.1-2   ~1200 tokens
# [1] KNOWLEDGE GRAPHS FOR TELECOM  p.2-4   ~2400 tokens
#   [2] Graph Construction          p.2-3   ~800 tokens
#   [2] Integration with LLMs      p.3-4   ~1100 tokens
# [1] EXPERIMENTS                   p.4-7   ~3200 tokens  fig:3 tab:2
# [1] RESULTS                       p.7-9   ~2100 tokens  fig:2 tab:3
# [1] CONCLUSION                    p.9-10  ~600 tokens
# [1] REFERENCES                    p.10-12 ~1500 tokens

# 選択セクションのMarkdown出力（--section は単数・繰り返し可。カンマ区切りの
# `--sections "A,B"` というフラグは存在しない）
pdf-lay markdown paper.pdf -o results.md --section RESULTS --section EXPERIMENTS

# インデックス指定 🅿 未実装（--section-index フラグは無い。CLI では名前一致のみ）
# pdf-lay markdown paper.pdf -o results.md --section-index 3,4

# LLM向けテキスト出力（テーブル・図はデフォルトで含まれる。
# `--include-tables`/`--include-figures` というフラグは無く、除外したい場合は
# `--no-tables`/`--no-figures` を使う）
pdf-lay llm-text paper.pdf --section RESULTS
```

**名前マッチングルール:**

`select_sections` で名前を指定する際のマッチング:
1. 完全一致（大文字小文字無視）: `"RESULTS"` → `"Results"` ✓
2. 部分一致: `"RESULT"` → `"RESULTS AND DISCUSSION"` ✓
3. 番号除去後にマッチ: `"KNOWLEDGE GRAPHS"` → `"II. KNOWLEDGE GRAPHS FOR TELECOM"` ✓
4. 子セクションの自動包含: 親セクションを選択すると子も含まれる

### 2.15 F-020: テーブルのインラインテキスト表現

**説明:** テーブルを画像ではなく、Markdownテーブルまたは構造化テキストとしてインラインで出力する。LLMがテーブルの内容を直接テキストとして読める形式にする。

**テーブル検出・変換パイプライン:**

```
PDFページ
  │
  ├─ 罫線検出 → グリッド構造 → セル内テキスト割り当て → Markdownテーブル
  │                                                      (精度: 高)
  ├─ テキスト座標ベース → 列アラインメント検出 → セル推定 → Markdownテーブル
  │                                                      (精度: 中)
  └─ フォールバック → テーブル領域のテキストを座標順に整形テキスト化
                                                         (精度: 低、ただし常に動作)
```

**出力形式（3段階の精度レベル）:**

```rust
pub enum TableRepresentation {
    /// 完全なMarkdownテーブル（罫線ベースで成功した場合）
    Markdown {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
        caption: Option<String>,
        markdown_text: String,
    },
    /// CSV風のテキスト表現（列アラインメントベースの場合）
    Csv {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
        caption: Option<String>,
        csv_text: String,
    },
    /// 整形テキスト（フォールバック）
    PlainText {
        text: String,
        caption: Option<String>,
    },
}
```

**Markdownテーブルへの変換例:**

入力（PDFからの抽出）:
```
Table 2: Performance comparison of different methods

text="Method"    bbox=(L54, T400)
text="Accuracy"  bbox=(L180, T400)
text="F1 Score"  bbox=(L300, T400)
text="KG-RAG"    bbox=(L54, T385)
text="0.92"      bbox=(L180, T385)
text="0.89"      bbox=(L300, T385)
text="Baseline"  bbox=(L54, T370)
text="0.78"      bbox=(L180, T370)
text="0.72"      bbox=(L300, T370)
```

出力:
```markdown
**Table 2:** Performance comparison of different methods

| Method | Accuracy | F1 Score |
|--------|----------|----------|
| KG-RAG | 0.92 | 0.89 |
| Baseline | 0.78 | 0.72 |
```

**テーブル結合セル（マルチカラムヘッダー）の処理:**

```
入力:              処理後:
┌──────────────┐   | | Metrics | Metrics |
│   │ Metrics  │   | Method | Accuracy | F1 |
│   ├────┬─────┤   |--------|----------|-----|
│   │Acc │ F1  │   ← ヘッダーを展開・統合
├───┼────┼─────┤
│ A │0.9 │0.85 │
```

LLMに渡す場合、展開後のフラットなテーブルの方が解釈しやすい:
```
| Method | Metrics_Accuracy | Metrics_F1 |
|--------|------------------|------------|
| A | 0.9 | 0.85 |
```

**テーブル内の特殊要素:**

| 要素 | 処理方針 |
|------|---------|
| 下付き文字（H₂O） | Unicode下付き文字に変換 or `H_2O` のLaTeX風表記 |
| 上付き文字（10³） | Unicode上付き文字に変換 or `10^3` |
| 太字セル | `**text**` のMarkdown太字 |
| 空セル | `-` or 空文字 |
| セル内改行 | ` / ` で結合（Markdownテーブルは改行不可） |
| ± 記号 | そのまま保持 |

### 2.16 F-021: 数式・LaTeX表現の検出と変換

**説明:** PDF内の数式を検出し、LLMが理解可能なテキスト表現に変換する。

**背景と課題:**

PDFにおける数式は、元のLaTeXソースが失われた状態で格納されている。pdf_oxideから見える数式は以下のような形になる:

```
# 元のLaTeX: E = mc^2

# pdf_oxideの出力（個別のグリフ断片）:
text="E" font=CMMI10 size=11.0 bbox=(L200, T500)-(R210, B511)
text="=" font=CMR10  size=11.0 bbox=(L215, T500)-(R225, B511)
text="m" font=CMMI10 size=11.0 bbox=(L230, T500)-(R240, B511)
text="c" font=CMMI10 size=11.0 bbox=(L242, T500)-(R250, B511)
text="2" font=CMR7   size=7.0  bbox=(L251, T505)-(R257, B512)  ← 上付き（小さいフォント、Y位置が高い）
```

数式グリフの特徴:
- フォント名に `CM` (Computer Modern), `Math`, `Symbol`, `MT` 等が含まれる
- 通常のテキストとフォントが異なる
- 上付き・下付き文字はフォントサイズとY位置で判別可能
- 特殊記号（∑, ∫, √, ∈, ∀, ∃ 等）

**処理戦略（3段階）:**

```rust
pub enum MathRepresentation {
    /// LaTeX形式（最も表現力が高い）
    /// 例: $E = mc^2$, $$\sum_{i=1}^{n} x_i$$
    LaTeX(String),

    /// Unicode数学テキスト（LaTeX復元不能な場合のフォールバック）
    /// 例: E = mc², ∑ᵢ₌₁ⁿ xᵢ
    UnicodeMath(String),

    /// プレーンテキスト（最低限の表現）
    /// 例: E = mc^2, sum(i=1 to n) x_i
    PlainText(String),
}

pub enum MathContext {
    /// インライン数式（本文中）
    Inline,
    /// ディスプレイ数式（独立行）
    Display { equation_number: Option<String> },
}
```

**数式検出アルゴリズム:**

```rust
pub struct MathDetector {
    math_font_patterns: Vec<Regex>,
    symbol_codepoints: HashSet<char>,
}

impl MathDetector {
    pub fn new() -> Self {
        Self {
            math_font_patterns: vec![
                Regex::new(r"(?i)^CM[A-Z]{1,4}\d").unwrap(),   // Computer Modern: CMMI10, CMR7, CMSY10
                Regex::new(r"(?i)math").unwrap(),                // *Math*
                Regex::new(r"(?i)symbol").unwrap(),              // *Symbol*
                Regex::new(r"(?i)^MT").unwrap(),                 // MathTime
                Regex::new(r"(?i)STIX").unwrap(),                // STIX fonts
            ],
            symbol_codepoints: [
                '∑', '∏', '∫', '∮', '√', '∞', '∂', '∇',
                '∈', '∉', '∀', '∃', '±', '≤', '≥', '≠', '≈',
                '→', '←', '↔', '⇒', '⇐', '⇔',
                'α', 'β', 'γ', 'δ', 'ε', 'θ', 'λ', 'μ', 'π', 'σ', 'φ', 'ω',
                // ... 拡張
            ].into_iter().collect(),
        }
    }

    /// スパンが数式フォントを使用しているか判定
    pub fn is_math_font(&self, font_name: &str) -> bool {
        self.math_font_patterns.iter().any(|p| p.is_match(font_name))
    }

    /// スパンが数式記号を含むか
    pub fn contains_math_symbol(&self, text: &str) -> bool {
        text.chars().any(|c| self.symbol_codepoints.contains(&c))
    }
}
```

**数式のテキスト変換（段階的アプローチ）:**

```
Level 1: フォントベース上付き・下付き検出
  - Y位置がベースラインより上 + フォントサイズが小さい → 上付き: ^{text}
  - Y位置がベースラインより下 + フォントサイズが小さい → 下付き: _{text}
  - 例: "mc2" (2が上付き) → "mc^{2}"

Level 2: 数式記号のLaTeXマッピング
  - ∑ → \sum, ∫ → \int, √ → \sqrt
  - α → \alpha, β → \beta
  - → → \rightarrow, ≤ → \leq
  - 例: "∑i=1n xi" → "\sum_{i=1}^{n} x_i"

Level 3: 構造推定（分数、ルート等）
  - 水平線 + 上下のテキスト → \frac{numerator}{denominator}
  - √記号 + 右のテキスト → \sqrt{text}
  ※ Level 3は精度が低いため、フォールバックとしてプレーンテキストも保持
```

**出力形式（Markdown内での表現）:**

```markdown
本文テキスト中のインライン数式は $E = mc^{2}$ のようにLaTeX形式で表現される。

ディスプレイ数式（独立行）は以下のように出力する:

$$
\sum_{i=1}^{n} x_i = x_1 + x_2 + \cdots + x_n \tag{1}
$$

LaTeX復元が困難な場合はUnicode数学テキストにフォールバック:

E = mc², ∑ᵢ₌₁ⁿ xᵢ = x₁ + x₂ + ⋯ + xₙ
```

**数式のLLMへの渡し方の推奨:**

LLMモデル（GPT-5, o3）はLaTeX形式の数式を理解可能。以下の優先順位で変換する:

| 状況 | 推奨形式 | 理由 |
|------|---------|------|
| LaTeX由来のPDF | `$LaTeX$` 形式 | LLMが最もよく理解する形式 |
| Word由来のPDF | Unicode数学テキスト | LaTeX復元が困難なため |
| 数式が少ない論文 | プレーンテキスト | シンプルな表現で十分 |
| テーブル内の数式 | プレーンテキスト or Unicode | テーブルセル内にLaTeXは冗長 |

**設定:**

```rust
pub struct MathConfig {
    /// 数式の出力形式
    pub representation: MathRepresentationPreference,
    /// インライン数式のデリミタ
    pub inline_delimiter: (String, String),   // デフォルト: ("$", "$")
    /// ディスプレイ数式のデリミタ
    pub display_delimiter: (String, String),  // デフォルト: ("$$\n", "\n$$")
    /// 上付き・下付きの検出感度
    pub superscript_y_threshold: f64,         // デフォルト: 0.3 (font_size比)
    /// 数式フォントの追加パターン（ユーザー定義）
    pub additional_math_fonts: Vec<String>,
}

pub enum MathRepresentationPreference {
    LaTeX,          // 可能な限りLaTeX
    UnicodeMath,    // 可能な限りUnicode数学文字
    PlainText,      // 常にプレーンテキスト
    Auto,           // フォント情報に基づいて自動選択（推奨）
}
```

### 3.1 パフォーマンス

| 項目 | 目標値 |
|------|--------|
| 12ページ論文の全解析時間 | < 3秒 |
| 画像抽出込み | < 5秒 |
| メモリ使用量（12ページ論文） | < 200MB |
| 10論文のバッチ処理 | < 30秒（並列処理時） |

### 3.2 正確性

| 項目 | 目標値 |
|------|--------|
| セクションヘッダー検出精度 | > 95%（主要ジャーナル形式） |
| カラムレイアウト検出精度 | > 98% |
| キャプション-画像マッチング精度 | > 90% |
| テキスト読み順の正確性 | > 95% |

### 3.3 互換性

| 項目 | 要件 |
|------|------|
| Rust | 1.75+ (2024 edition) |
| Python | 3.9+ |
| OS | Linux (x86_64, aarch64), macOS (aarch64), Windows (x86_64) |
| PDFバージョン | PDF 1.0 〜 2.0 |

### 3.4 エラーハンドリング

- 破損PDFに対してパニックしないこと（Result型でエラーを返す）
- 部分的に解析不能なページがあっても、残りのページの解析を継続すること
- 座標正規化に失敗した場合、警告を出力しフォールバック値を使用すること

---

## 4. API仕様

### 4.1 Rust API

```rust
// メインエントリーポイント（戻り値は `PaperDocument` 単体ではなく `AnalysisResult`
// — `AnalysisResult.document: PaperDocument` と `.warnings` 等を持つラッパー。
// `crates/pdf-lay-core/src/pipeline.rs`、`pdf-lay` crate から再エクスポート済み）
pub fn analyze_pdf(path: &Path, config: &Config) -> Result<AnalysisResult, PdfLayError>;
pub fn analyze_pdf_bytes(bytes: &[u8], config: &Config) -> Result<AnalysisResult, PdfLayError>;

// 段階的API 🅿 未実装（`pdf-lay` crate から再エクスポートされていない。同等の内部実装は
// 存在するが非公開: `extract::PdfReader`/`layout::LineReconstructor`/`layout::ColumnDetector`/
// `structure::BlockGrouper`/`structure::SectionBuilder`/`extract::ImageExtractor`/
// `figure::ImageMatcher` 等、いずれも `pdf-lay-core` 内部モジュールのみ）
// pub fn extract_text_spans(path: &Path) -> Result<Vec<TextSpan>, PdfLayError>;
// pub fn reconstruct_lines(spans: &[TextSpan]) -> Vec<TextLine>;
// pub fn detect_layout(lines: &[TextLine], page_dims: &PageDimensions) -> PageLayout;
// pub fn group_blocks(lines: &[TextLine], layout: &PageLayout) -> Vec<TextBlock>;
// pub fn detect_sections(blocks: &[TextBlock]) -> Vec<Section>;
// pub fn extract_images(path: &Path, output_dir: &Path) -> Result<Vec<ImageInfo>, PdfLayError>;
// pub fn match_figures(blocks: &[TextBlock], images: &[ImageInfo]) -> Vec<FigureInfo>;

// セクション操作
impl PaperDocument {
    pub fn toc(&self) -> Vec<SectionEntry>;
    pub fn select_sections(&self, names: &[&str]) -> SectionSelector;
    pub fn select_sections_by_index(&self, indices: &[usize]) -> SectionSelector;
    pub fn select_sections_by_level(&self, level: u8) -> SectionSelector;
    pub fn select_sections_by_pages(&self, start: u32, end: u32) -> SectionSelector;
    pub fn select_sections_where<F>(&self, predicate: F) -> SectionSelector
    where F: Fn(&SectionEntry) -> bool;
}

impl<'a> SectionSelector<'a> {
    pub fn to_markdown(&self, config: &MarkdownConfig) -> String;
    pub fn to_json(&self) -> Result<String, serde_json::Error>;
    pub fn to_llm_text(&self, config: &LlmTextConfig) -> String;
    pub fn to_chunks(&self, config: &ChunkConfig) -> Vec<Chunk>;
    pub fn total_estimated_tokens(&self) -> usize;
}

// 出力生成 🅿 これらの自由関数は存在しない。実際は `SectionSelector` のメソッド
// （上記 `to_markdown`/`to_json`/`to_chunks`）、または構造体API
// `MarkdownGenerator::new(config).generate(&doc)` / `JsonGenerator::generate(&doc)` /
// `Chunker::new(config).chunk(&doc)`（`pdf_lay::output::{MarkdownGenerator, JsonGenerator, Chunker}`
// として再エクスポート済み）を使う。
// pub fn to_markdown(doc: &PaperDocument, config: &MarkdownConfig) -> String;
// pub fn to_json(doc: &PaperDocument) -> Result<String, serde_json::Error>;
// pub fn to_chunks(doc: &PaperDocument, config: &ChunkConfig) -> Vec<Chunk>;

// 設定
pub struct Config {
    pub image_output_dir: PathBuf,
    pub image_format: ImageFormat,
    pub extract_images: bool,
    pub detect_tables: bool,
    pub table_config: TableConfig,
    pub math_config: MathConfig,
    pub caption_max_gap_pt: f64,       // デフォルト: 50.0
    pub column_detection_bins: f64,     // デフォルト: 10.0
    pub block_gap_multiplier: f64,      // デフォルト: 1.8
    pub header_detection: HeaderDetectionConfig,
}

pub struct LlmTextConfig {
    pub include_figures: bool,          // デフォルト: true
    pub include_tables: bool,           // デフォルト: true
    pub include_section_headers: bool,  // デフォルト: true
    pub math_representation: MathRepresentationPreference,
    pub figure_format: FigureTextFormat,
}

pub enum FigureTextFormat {
    /// [IMAGE: Fig. 1 path/to/img.png]
    Placeholder,
    /// ![Fig. 1](path/to/img.png)
    MarkdownLink,
    /// キャプションテキストのみ（画像パスなし）
    CaptionOnly,
    /// 図を完全に省略
    Omit,
}
```

### 4.2 Python API (PyO3)

```python
import pdflay

# ワンショット解析
doc = pdflay.analyze("paper.pdf", image_dir="./images")

# 結果アクセス（`doc.metadata.*` という属性は無い。実 API は doc.title / doc.authors 等の
# トップレベル属性 — `crates/pdflay-python/src/lib.rs` `PyPaperDocument`）
print(doc.title)
print(doc.authors)
print(len(doc.sections))

for section in doc.sections:
    print(f"[{section.level}] {section.header}")
    print(section.text[:200])
    for fig in section.figures:
        print(f"  {fig.figure_id}: {fig.image_path}")
    for child in section.children:
        print(f"  [{child.level}] {child.header}")

# --- セクション一覧（目次）取得 ---
toc = doc.toc()
for entry in toc:
    # 注: `entry.page_range[0]/[1]` は存在しない。実 API は page_start / page_end。
    print(f"[L{entry.level}] {entry.header} "
          f"(p.{entry.page_start}-{entry.page_end}, "
          f"~{entry.estimated_tokens} tokens, "
          f"fig:{entry.figure_count}, tab:{entry.table_count})")

# --- セクション選択的出力 ---
# ヘッダー名で選択（部分一致、大文字小文字無視）
selected = doc.select_sections(["RESULTS", "EXPERIMENTS"])

# Markdown出力（選択セクションのみ）
md = selected.to_markdown(image_base_path="./images")

# LLM向けテキスト出力（テーブルをインライン、図をプレースホルダ）
llm_text = selected.to_llm_text(
    include_figures=True,
    include_tables=True,
)
print(f"Total tokens: ~{selected.total_estimated_tokens()}")

# インデックスで選択
selected = doc.select_sections_by_index([3, 4])

# テーブル・図があるセクションだけ選択
selected = doc.select_sections_where(
    lambda entry: entry.has_tables or entry.has_figures
)

# --- 全体のMarkdown出力 ---
md = doc.to_markdown(image_base_path="./images")
with open("output.md", "w") as f:
    f.write(md)

# JSON出力
json_str = doc.to_json()

# LLM向けチャンク
chunks = doc.to_chunks(max_tokens=4000, overlap=200)
for chunk in chunks:
    print(f"Section: {chunk.section}, Tokens: ~{chunk.estimated_tokens}")

# --- 数式表現の設定 🅿 未実装 ---
# `pdflay.Config` クラスは公開されておらず、`analyze()` は `config=` 引数を取らない。
# `analyze()` の実引数は `path, image_dir, extract_images, detect_tables` のみ
# （`crates/pdflay-python/src/lib.rs` `fn analyze`）。以下は将来 API の例:
# config = pdflay.Config(
#     extract_images=True,
#     detect_tables=True,
#     math_representation="latex",   # "latex", "unicode", "plain", "auto"
# )
# doc = pdflay.analyze("paper.pdf", config=config)

# 段階的API 🅿 未実装（モジュールに存在しない関数）
# spans = pdflay.extract_spans("paper.pdf")
# lines = pdflay.reconstruct_lines(spans)
# layout = pdflay.detect_layout(lines, page_width=612.0, page_height=792.0)

# バッチ処理 🅿 未実装（モジュールに存在しない関数）
# results = pdflay.analyze_batch(
#     ["paper1.pdf", "paper2.pdf", "paper3.pdf"],
#     image_dir="./images",
#     parallel=True,
# )
```

### 4.3 CLI

```bash
# 基本使用 🅿 未実装（`analyze` サブコマンドは存在しない。`toc`/`markdown`/`json`/
# `chunks`/`llm-text` の5つのみが実サブコマンド）
# pdf-lay analyze paper.pdf -o output/

# --- セクション一覧表示 ---
pdf-lay toc paper.pdf
# 出力:
# [1] Abstract                      p.1     ~320 tokens
# [1] INTRODUCTION                  p.1-2   ~1200 tokens
# [1] KNOWLEDGE GRAPHS FOR TELECOM  p.2-4   ~2400 tokens  fig:2 tab:1
#   [2] Graph Construction          p.2-3   ~800 tokens   fig:1
#   [2] Integration with LLMs      p.3-4   ~1100 tokens  fig:1 tab:1
# [1] EXPERIMENTS                   p.4-7   ~3200 tokens  fig:3 tab:2
# [1] RESULTS                       p.7-9   ~2100 tokens  fig:2 tab:3
# [1] CONCLUSION                    p.9-10  ~600 tokens
# [1] REFERENCES                    p.10-12 ~1500 tokens

# --- セクション選択出力 ---
# ヘッダー名指定（`--section` は単数・繰り返し可。カンマ区切りの
# `--sections "A,B"` というフラグは存在しない）
pdf-lay markdown paper.pdf -o results.md --section RESULTS --section EXPERIMENTS

# インデックス指定 🅿 未実装（--section-index フラグは無い）
# pdf-lay markdown paper.pdf -o results.md --section-index 3,4

# LLM向けテキスト出力（テーブル・図はデフォルトで含まれる。`--include-tables`/
# `--include-figures` というフラグは無く、除外時は `--no-tables`/`--no-figures` を使う）
pdf-lay llm-text paper.pdf --section RESULTS

# --- 全体出力 ---
# Markdown出力
pdf-lay markdown paper.pdf -o paper.md --image-dir ./images

# JSON出力（フルダンプ、または `--content-only` で軽量な LLM 向け投影）
pdf-lay json paper.pdf -o paper.json
pdf-lay json paper.pdf -o paper.json --content-only

# チャンク分割（JSONL、RAG向け）
pdf-lay chunks paper.pdf --max-tokens 4000 --overlap 200 --strategy section -o paper.chunks.jsonl

# バッチ処理 🅿 未実装（複数ファイル一括処理・`--parallel` フラグは無い）
# pdf-lay analyze papers/*.pdf -o output/ --parallel 4

# --- 数式設定（`markdown`/`json --content-only`/`chunks`/`llm-text` 全サブコマンドで有効）---
pdf-lay markdown paper.pdf -o paper.md --math-format latex
pdf-lay markdown paper.pdf -o paper.md --math-format unicode
pdf-lay markdown paper.pdf -o paper.md --math-format plain
pdf-lay markdown paper.pdf -o paper.md --math-format off

# デバッグ（レイアウト可視化） 🅿 未実装（`debug-layout` サブコマンドは無い。
# `AGENTS.md` の同記述も同じく未実装コマンドを前提にしている）
# pdf-lay debug-layout paper.pdf -o debug/ --page 2
```

---

## 5. 入出力仕様

### 5.1 入力

| 形式 | 説明 |
|------|------|
| PDF file path | ファイルシステム上のPDFパス |
| PDF bytes | メモリ上のPDFバイト列（Python: `bytes`, Rust: `&[u8]`） |

### 5.2 出力

| 形式 | ファイル | 説明 |
|------|---------|------|
| Markdown | `{paper_id}.md` | セクション構造化 + 画像リンク埋め込み |
| JSON | `{paper_id}.json` | 完全な中間表現 |
| Images | `images/p{page}_img{num}.png` | 抽出画像 |
| Chunks JSON | `{paper_id}_chunks.json` | LLM向け分割データ |
| Debug HTML | `debug/p{page}_layout.html` | レイアウト可視化（開発用） |

---

## 6. テスト要件

### 6.1 テスト対象論文

以下のジャーナル形式から最低各2本のテスト論文を用意する:

| ジャーナル形式 | 特徴 |
|---------------|------|
| IEEE (2段組) | ローマ数字セクション番号、全大文字見出し |
| Elsevier (1段組) | アラビア数字セクション番号 |
| Springer/Nature (1段組) | 太字セクション名、キャプション上配置 |
| ACS (2段組) | 段幅の狭い2段組 |
| RSC (2段組) | RSC独自のレイアウト |
| arXiv preprint | LaTeX由来、多様なスタイル |

### 6.2 テストケース

| カテゴリ | テスト項目 |
|---------|-----------|
| レイアウト検出 | 1段組検出、2段組検出、混在検出、全幅図検出 |
| セクション検出 | IEEE形式ヘッダー、数字付きヘッダー、太字ヘッダー、Abstract検出 |
| 画像対応付け | キャプション下配置、キャプション上配置、2カラムまたぎ |
| Markdown生成 | 読み順の正確性、画像リンクの位置、セクション階層 |
| エラー処理 | 破損PDF、画像なしPDF、テキストなしページ |
| パフォーマンス | 12ページ論文の処理時間、100ページ論文の処理時間 |

---

## 7. 用語定義

| 用語 | 定義 |
|------|------|
| TextSpan | PDFから抽出された最小テキスト単位（座標・フォント情報付き） |
| TextLine | Y座標が近接するスパンを結合した論理的な1行 |
| TextBlock | 隣接する行をまとめた段落レベルの単位 |
| Column | ページ内のテキストの縦方向の配置領域 |
| LayoutRegion | ページ内で同一カラム構成を持つY方向の帯 |
| Section | セクションヘッダーと配下のブロック・図・テーブルの集合 |
| InsertionPoint | Markdown生成時の画像/テーブルの挿入位置 |
| Chunk | LLMのコンテキストウィンドウに収まるテキスト分割単位 |
