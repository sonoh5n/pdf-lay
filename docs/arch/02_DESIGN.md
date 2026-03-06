# pdf-lay: PDF Layout Analysis Library — 設計書

**Version:** 0.1.0
**Date:** 2026-02-20
**Status:** Draft

---

## 1. アーキテクチャ概要

### 1.1 全体構成

```
┌─────────────────────────────────────────────────────────┐
│                    利用側アプリケーション                    │
│    TypeScript Agent  /  Python Script  /  CLI            │
└──────────────┬────────────────┬────────────────┬─────────┘
               │                │                │
    ┌──────────▼──────┐ ┌──────▼───────┐ ┌──────▼───────┐
    │  pdflay (Python) │ │ pdf-lay (Rust)│ │ pdf-lay-cli  │
    │  PyO3 bindings   │ │ Library crate │ │ Binary crate │
    └──────────┬──────┘ └──────┬───────┘ └──────┬───────┘
               │                │                │
               └────────────────┼────────────────┘
                                │
               ┌────────────────▼────────────────┐
               │         pdf-lay-core             │
               │      (内部ライブラリクレート)        │
               ├──────────────────────────────────┤
               │  extract   │ テキスト・画像抽出     │
               │  layout    │ カラム・レイアウト解析  │
               │  structure │ ブロック・セクション構築 │
               │  figure    │ 画像-キャプション対応   │
               │  table     │ テーブル検出・抽出      │
               │  output    │ Markdown/JSON/Chunk生成 │
               └────────────┬─────────────────────┘
                            │
               ┌────────────▼─────────────────────┐
               │         外部依存                    │
               │  pdf_oxide  │  image (crate)       │
               │  serde      │  regex               │
               └──────────────────────────────────┘
```

### 1.2 クレート構成

```
pdf-lay/
├── Cargo.toml              # ワークスペース定義
├── crates/
│   ├── pdf-lay-core/       # コアライブラリ（内部クレート）
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── extract/    # PDF抽出層
│   │       ├── layout/     # レイアウト解析層
│   │       ├── structure/  # 構造構築層
│   │       ├── figure/     # 画像処理層
│   │       ├── table/      # テーブル処理層
│   │       ├── math/       # 数式検出・変換層
│   │       ├── selector/   # セクション選択層
│   │       ├── output/     # 出力生成層
│   │       ├── types/      # 共通型定義
│   │       └── config.rs   # 設定
│   │
│   ├── pdf-lay/            # パブリックRustライブラリクレート
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs      # pdf-lay-coreの公開API再エクスポート
│   │
│   ├── pdf-lay-cli/        # CLIバイナリクレート
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs
│   │
│   └── pdflay-python/      # PyO3 Pythonバインディング
│       ├── Cargo.toml
│       ├── pyproject.toml  # maturin設定
│       └── src/
│           └── lib.rs
│
├── tests/                  # 統合テスト
│   ├── fixtures/           # テスト用PDF
│   └── integration/
│
├── benches/                # ベンチマーク
└── docs/                   # ドキュメント
```

### 1.3 データフローパイプライン

```
PDF ファイル
  │
  ▼
[Extract] TextSpan[] + ImageInfo[] + PathObject[]
  │
  ▼
[Layout:Lines] TextLine[]           ── 行の再構成
  │
  ▼
[Layout:Columns] PageLayout[]       ── カラムレイアウト検出
  │
  ▼
[Structure:Blocks] TextBlock[]      ── ブロックグルーピング + 分類
  │
  ▼
[Structure:Sections] Section[]      ── セクション階層構築
  │
  ├─[Figure] FigureInfo[]           ── キャプション検出 + 画像マッチング
  ├─[Table] TableInfo[]             ── テーブル検出 + インラインテキスト変換
  ├─[Math] MathRegion[]             ── 数式検出 + LaTeX/Unicode変換
  │
  ▼
[Output] PaperDocument              ── 統合ドキュメント構造
  │
  ├──▶ doc.toc()                    ── セクション一覧（目次）
  ├──▶ doc.select_sections(...)     ── セクション選択
  │      └──▶ selected.to_markdown()
  │      └──▶ selected.to_llm_text()
  │      └──▶ selected.to_chunks()
  ├──▶ Markdown (全体)
  ├──▶ JSON
  └──▶ Chunks (LLM用)
```

---

## 2. モジュール詳細設計

### 2.1 types — 共通型定義

```rust
// crates/pdf-lay-core/src/types/mod.rs

pub mod geometry;
pub mod text;
pub mod document;

// --- geometry.rs ---

/// PDF座標系のバウンディングボックス
/// PDF default: 原点は左下、Y軸は上向き
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub left: f64,
    pub top: f64,      // Y座標の大きい方（ページ上方）
    pub right: f64,
    pub bottom: f64,   // Y座標の小さい方（ページ下方）
}

impl Rect {
    pub fn width(&self) -> f64 { self.right - self.left }
    pub fn height(&self) -> f64 { self.top - self.bottom }
    pub fn center_x(&self) -> f64 { (self.left + self.right) / 2.0 }
    pub fn center_y(&self) -> f64 { (self.top + self.bottom) / 2.0 }

    /// 2つのRectの垂直距離（gap）。重なりがある場合は負値
    pub fn vertical_gap(&self, other: &Rect) -> f64 {
        if self.bottom > other.top {
            self.bottom - other.top
        } else if other.bottom > self.top {
            other.bottom - self.top
        } else {
            0.0 // 重なっている
        }
    }

    /// 2つのRectを統合した最小外接矩形
    pub fn union(&self, other: &Rect) -> Rect {
        Rect {
            left: self.left.min(other.left),
            top: self.top.max(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageDimensions {
    pub page_number: u32,
    pub width: f64,     // ポイント単位
    pub height: f64,
}

// --- text.rs ---

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FontInfo {
    pub name: String,
    pub size: f64,
    pub is_bold: bool,
    pub is_italic: bool,
}

impl FontInfo {
    /// フォント名からBold判定（ヒューリスティック）
    pub fn detect_bold(font_name: &str) -> bool {
        let lower = font_name.to_lowercase();
        lower.contains("bold")
            || lower.contains("-bd")
            || lower.contains("heavy")
            || lower.contains("black")
    }

    /// フォント名からItalic判定
    pub fn detect_italic(font_name: &str) -> bool {
        let lower = font_name.to_lowercase();
        lower.contains("italic")
            || lower.contains("oblique")
            || lower.contains("-it")
    }
}
```

### 2.2 extract — PDF抽出層

```rust
// crates/pdf-lay-core/src/extract/mod.rs

mod pdf_reader;
mod image_extractor;
mod coordinate;

pub use pdf_reader::PdfReader;
pub use image_extractor::ImageExtractor;
pub use coordinate::CoordinateNormalizer;

// --- pdf_reader.rs ---

use pdf_oxide; // pdf_oxideへの依存

pub struct PdfReader {
    // pdf_oxideのドキュメントハンドル
}

impl PdfReader {
    pub fn open(path: &Path) -> Result<Self, PdfLayError> { ... }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PdfLayError> { ... }

    pub fn page_count(&self) -> u32 { ... }
    pub fn page_dimensions(&self, page: u32) -> PageDimensions { ... }

    /// ページからテキストスパンを抽出
    pub fn extract_text_spans(&self, page: u32) -> Result<Vec<TextSpan>, PdfLayError> {
        // pdf_oxideのAPI呼び出し
        // 各グリフ/テキストオブジェクトからTextSpanを生成
        // フォント情報の解析（is_bold, is_italic の推定）
        ...
    }

    /// 全ページからテキストスパンを抽出
    pub fn extract_all_text_spans(&self) -> Result<Vec<TextSpan>, PdfLayError> {
        let mut all_spans = Vec::new();
        for page in 0..self.page_count() {
            all_spans.extend(self.extract_text_spans(page)?);
        }
        Ok(all_spans)
    }

    /// ページからパスオブジェクト（罫線）を抽出（テーブル検出用）
    pub fn extract_paths(&self, page: u32) -> Result<Vec<PathObject>, PdfLayError> { ... }
}

// --- image_extractor.rs ---

pub struct ImageExtractor {
    output_dir: PathBuf,
}

impl ImageExtractor {
    pub fn new(output_dir: PathBuf) -> Self { ... }

    /// PDFから全画像を抽出して保存
    pub fn extract_all(
        &self,
        reader: &PdfReader,
    ) -> Result<Vec<ImageInfo>, PdfLayError> {
        let mut images = Vec::new();
        for page in 0..reader.page_count() {
            let page_images = self.extract_page_images(reader, page)?;
            images.extend(page_images);
        }
        Ok(images)
    }

    fn extract_page_images(
        &self,
        reader: &PdfReader,
        page: u32,
    ) -> Result<Vec<ImageInfo>, PdfLayError> {
        // pdf_oxideから画像オブジェクトを取得
        // デコード（JPEG, PNG, CCITT等）
        // ファイルに保存: p{page:03}_img{num:03}.png
        // ImageInfoを生成（raw_bbox付き）
        ...
    }
}

// --- coordinate.rs ---

/// 画像座標とテキスト座標のスケール差を解決する
pub struct CoordinateNormalizer {
    scale_factor: f64,
}

impl CoordinateNormalizer {
    /// 同一ページのテキストと画像のbboxからスケールファクタを自動推定
    pub fn estimate(
        images: &[ImageInfo],
        text_lines: &[TextLine],
        page_dims: &PageDimensions,
    ) -> Self {
        // 方法1: ページ幅に対する画像幅の比率から推定
        // 方法2: テキストのコンテンツ幅と画像の幅の比率から推定
        // 方法3: 既知のスケール（1/1000等）を試してテキストとの整合性を検証
        ...
    }

    pub fn normalize(&self, raw_bbox: &Rect) -> Rect {
        Rect {
            left: raw_bbox.left * self.scale_factor,
            top: raw_bbox.top * self.scale_factor,
            right: raw_bbox.right * self.scale_factor,
            bottom: raw_bbox.bottom * self.scale_factor,
        }
    }
}
```

### 2.3 layout — レイアウト解析層

```rust
// crates/pdf-lay-core/src/layout/mod.rs

mod line_reconstructor;
mod column_detector;

pub use line_reconstructor::LineReconstructor;
pub use column_detector::ColumnDetector;

// --- line_reconstructor.rs ---

pub struct LineReconstructor {
    y_tolerance_factor: f64,  // デフォルト: 0.5 (font_size × factor)
}

impl LineReconstructor {
    pub fn new() -> Self {
        Self { y_tolerance_factor: 0.5 }
    }

    pub fn with_tolerance(mut self, factor: f64) -> Self {
        self.y_tolerance_factor = factor;
        self
    }

    /// テキストスパンを論理行に再構成
    pub fn reconstruct(&self, spans: &[TextSpan]) -> Vec<TextLine> {
        if spans.is_empty() { return vec![]; }

        // ページごとに処理
        let mut pages: BTreeMap<u32, Vec<&TextSpan>> = BTreeMap::new();
        for span in spans {
            pages.entry(span.page).or_default().push(span);
        }

        let mut all_lines = Vec::new();
        for (page, page_spans) in pages {
            all_lines.extend(self.reconstruct_page(&page_spans));
        }
        all_lines
    }

    fn reconstruct_page(&self, spans: &[&TextSpan]) -> Vec<TextLine> {
        // 1. Y座標(top)で降順ソート（ページ上部から）
        // 2. 近接Y座標のスパンをグループ化
        //    閾値: font_size × self.y_tolerance_factor
        // 3. グループ内をX座標で昇順ソート
        // 4. スパン間のギャップに応じてスペース挿入
        // 5. TextLineを生成
        ...
    }

    /// スパン間にスペースを挿入すべきかの判定
    fn needs_space(prev: &TextSpan, next: &TextSpan) -> bool {
        let gap = next.bbox.left - prev.bbox.right;
        let char_width = prev.font_size * 0.5; // 概算
        gap > char_width * 0.3
    }
}

// --- column_detector.rs ---

pub struct ColumnDetector {
    bin_width: f64,            // ヒストグラムのビン幅（デフォルト: 10.0pt）
    min_peak_ratio: f64,       // ピーク検出の最小比率（デフォルト: 0.2 = 20%）
    cluster_gap_ratio: f64,    // クラスタ間の最小ギャップ（ページ幅に対する比率, デフォルト: 0.15）
    zone_count: usize,         // Y方向の分割数（デフォルト: 4）
}

impl ColumnDetector {
    pub fn new() -> Self { ... }

    /// ページ内のカラムレイアウトを検出
    pub fn detect(
        &self,
        lines: &[TextLine],
        page_dims: &PageDimensions,
    ) -> PageLayout {
        // 1. ページをY方向にzone_count個のゾーンに分割
        // 2. 各ゾーンでカラム検出
        // 3. 隣接ゾーンが同一カラム数なら統合
        let zones = self.split_zones(lines, page_dims);
        let zone_layouts: Vec<_> = zones.iter()
            .map(|z| self.detect_zone_columns(z, page_dims))
            .collect();
        self.merge_zones(zone_layouts, page_dims)
    }

    fn detect_zone_columns(
        &self,
        zone_lines: &[&TextLine],
        page_dims: &PageDimensions,
    ) -> ZoneLayout {
        // X座標ヒストグラム → ピーク検出 → クラスタリング
        // ピークが1つ → 1段組
        // ピークが2つ → 2段組（ギャップ中心を境界とする）
        ...
    }

    /// 全幅要素（2カラムにまたがる図・テーブル）の検出
    fn detect_full_width_elements(
        &self,
        lines: &[TextLine],
        page_dims: &PageDimensions,
    ) -> Vec<FullWidthRegion> {
        // 行の幅がページコンテンツ幅の60%以上 → 全幅候補
        // 連続する全幅行をグルーピング
        ...
    }
}
```

### 2.4 structure — 構造構築層

```rust
// crates/pdf-lay-core/src/structure/mod.rs

mod block_grouper;
mod block_classifier;
mod header_detector;
mod section_builder;
mod reading_order;

pub use block_grouper::BlockGrouper;
pub use block_classifier::BlockClassifier;
pub use header_detector::HeaderDetector;
pub use section_builder::SectionBuilder;
pub use reading_order::ReadingOrderSorter;

// --- block_grouper.rs ---

pub struct BlockGrouper {
    gap_multiplier: f64,        // ブロック区切りの行間閾値（デフォルト: 1.8）
    font_size_threshold: f64,   // フォントサイズ変化の閾値（デフォルト: 1.0pt）
}

impl BlockGrouper {
    /// カラム内の行をブロックにグルーピング
    pub fn group(
        &self,
        lines: &[TextLine],
        layout: &PageLayout,
    ) -> Vec<TextBlock> {
        let mut all_blocks = Vec::new();
        let mut global_index = 0;

        for region in &layout.regions {
            for column in &region.columns {
                // カラムに属する行を抽出
                let col_lines = self.filter_column_lines(lines, column, region);
                // Y座標でソート（上から下）
                let sorted = self.sort_top_to_bottom(&col_lines);
                // ブロック区切り検出
                let blocks = self.group_sorted_lines(&sorted, &mut global_index, column.index);
                all_blocks.extend(blocks);
            }
        }

        all_blocks
    }

    fn detect_break(&self, prev: &TextLine, current: &TextLine) -> bool {
        // 1. 行間チェック
        let line_gap = prev.bbox.bottom - current.bbox.top;
        let normal_spacing = prev.primary_font_size * 1.2;
        if line_gap > normal_spacing * self.gap_multiplier {
            return true;
        }

        // 2. フォントサイズ変化
        if (prev.primary_font_size - current.primary_font_size).abs() > self.font_size_threshold {
            return true;
        }

        // 3. Bold ↔ Regular 変化
        if prev.is_bold != current.is_bold {
            return true;
        }

        false
    }
}

// --- block_classifier.rs ---

pub struct BlockClassifier {
    body_font_size: f64,     // 統計的に検出した本文フォントサイズ
    known_sections: Vec<String>,
}

impl BlockClassifier {
    /// 全ブロックのbody_font_sizeを統計的に検出してからインスタンス化
    pub fn from_blocks(blocks: &[TextBlock]) -> Self {
        let body_font_size = Self::detect_body_font_size(blocks);
        Self {
            body_font_size,
            known_sections: Self::default_known_sections(),
        }
    }

    /// ブロックのタイプを分類
    pub fn classify(&self, block: &mut TextBlock) {
        let text = block.text.trim();
        let font_size = block.primary_font_size();
        let is_bold = block.is_bold();
        let size_ratio = font_size / self.body_font_size;

        block.block_type = if self.is_caption(text) {
            BlockType::Caption
        } else if self.is_page_number(text) {
            BlockType::PageNumber
        } else if self.is_running_header(block) {
            BlockType::RunningHeader
        } else if self.is_footnote(block, size_ratio) {
            BlockType::Footnote
        } else if self.is_section_header(text, is_bold, size_ratio, block.lines.len()) {
            if self.detect_header_level(text, is_bold, size_ratio) == 1 {
                BlockType::SectionHeader
            } else {
                BlockType::SubsectionHeader
            }
        } else if size_ratio > 1.5 && block.lines.len() <= 3 {
            BlockType::Title
        } else {
            BlockType::BodyText
        };
    }

    fn is_caption(text: &str) -> bool {
        let lower = text.to_lowercase();
        lower.starts_with("fig.") || lower.starts_with("figure")
            || lower.starts_with("table") || lower.starts_with("tab.")
    }

    fn detect_body_font_size(blocks: &[TextBlock]) -> f64 {
        // 各ブロックの文字数でウェイト付けしたフォントサイズのヒストグラム
        // 最頻出 = 本文フォントサイズ
        ...
    }
}

// --- header_detector.rs ---

pub struct HeaderDetector {
    body_font_size: f64,
    numbering_patterns: Vec<NumberingPattern>,
    known_names: HashSet<String>,
}

/// セクション番号のパターン
enum NumberingPattern {
    Roman,          // I., II., III.
    Arabic,         // 1., 2., 3.
    ArabicDot,      // 1.1, 1.2, 3.1.1
    AlphaUpper,     // A., B., C.
}

impl HeaderDetector {
    pub fn detect(&self, blocks: &[TextBlock]) -> Vec<SectionHeader> {
        blocks.iter().enumerate()
            .filter_map(|(i, block)| self.try_detect(block, i))
            .collect()
    }

    fn try_detect(&self, block: &TextBlock, index: usize) -> Option<SectionHeader> {
        // 前提条件チェック
        if block.text.len() > 120 || block.lines.len() > 3 {
            return None;
        }

        let text = block.text.trim();
        let font_size = block.primary_font_size();
        let is_bold = block.is_bold();
        let size_ratio = font_size / self.body_font_size;

        // パターンマッチ（複数パターンをスコアリング）
        let mut score = 0;
        let mut level = 1u8;
        let mut numbering = None;

        // 番号付き
        if let Some((num, pat_level)) = self.match_numbering(text) {
            score += 3;
            level = pat_level;
            numbering = Some(num);
        }

        // 全大文字
        if self.is_all_caps(text) { score += 2; }

        // 太字
        if is_bold { score += 2; }

        // フォントサイズが大きい
        if size_ratio > 1.1 { score += 1; }

        // 既知セクション名
        if self.is_known_name(text) { score += 2; }

        // 短い（1行）
        if block.lines.len() == 1 { score += 1; }

        // スコア閾値
        if score >= 4 {
            Some(SectionHeader {
                text: text.to_string(),
                clean_text: self.clean_header_text(text),
                level,
                numbering,
                page: block.page,
                bbox: block.bbox.clone(),
                block_index: index,
            })
        } else {
            None
        }
    }

    /// セクション番号の検出とレベル推定
    fn match_numbering(&self, text: &str) -> Option<(String, u8)> {
        let trimmed = text.trim();

        // ローマ数字: "II. KNOWLEDGE GRAPHS" → ("II.", level 1)
        if let Some(caps) = regex!(r"^([IVX]+\.)\s+").captures(trimmed) {
            return Some((caps[1].to_string(), 1));
        }

        // アラビア数字 + サブセクション: "3.1.1" → level 3
        if let Some(caps) = regex!(r"^(\d+\.\d+\.\d+)[\s.]").captures(trimmed) {
            return Some((caps[1].to_string(), 3));
        }

        // アラビア数字 + サブセクション: "3.1" → level 2
        if let Some(caps) = regex!(r"^(\d+\.\d+)[\s.]").captures(trimmed) {
            return Some((caps[1].to_string(), 2));
        }

        // アラビア数字: "3." → level 1
        if let Some(caps) = regex!(r"^(\d+\.)\s+").captures(trimmed) {
            return Some((caps[1].to_string(), 1));
        }

        // 英字: "A." → level 2
        if let Some(caps) = regex!(r"^([A-Z]\.)\s+").captures(trimmed) {
            return Some((caps[1].to_string(), 2));
        }

        None
    }
}

// --- section_builder.rs ---

pub struct SectionBuilder;

impl SectionBuilder {
    /// ヘッダーとブロックからセクション階層を構築
    pub fn build(
        blocks: &[TextBlock],
        headers: &[SectionHeader],
        figures: &[FigureInfo],
        tables: &[TableInfo],
    ) -> Vec<Section> {
        // 1. ブロックをヘッダー間で区切り → フラットセクションリスト
        let flat_sections = Self::split_by_headers(blocks, headers);

        // 2. 各セクションに図・テーブルを割り当て
        let sections_with_assets = Self::assign_assets(flat_sections, figures, tables);

        // 3. フラットリストをレベルに基づいて階層化
        Self::build_hierarchy(sections_with_assets)
    }

    fn split_by_headers(
        blocks: &[TextBlock],
        headers: &[SectionHeader],
    ) -> Vec<FlatSection> {
        // ヘッダーのblock_indexでブロック列を分割
        // ヘッダーより前のブロック → preamble（タイトル・著者情報）
        ...
    }

    fn assign_assets(
        sections: Vec<FlatSection>,
        figures: &[FigureInfo],
        tables: &[TableInfo],
    ) -> Vec<FlatSection> {
        // 各図・テーブルのinsertion_pointのblock_indexを見て
        // 適切なセクションに割り当て
        ...
    }

    fn build_hierarchy(flat: Vec<FlatSection>) -> Vec<Section> {
        // スタックベースの階層化:
        // level 1 のセクションが親、level 2 が子、level 3 が孫
        // level が同じか小さい → 兄弟または親に戻る
        ...
    }
}

// --- reading_order.rs ---

pub struct ReadingOrderSorter;

impl ReadingOrderSorter {
    /// ブロックを論理的な読み順にソート
    pub fn sort(blocks: &mut [TextBlock], layouts: &[PageLayout]) {
        blocks.sort_by(|a, b| {
            // 1. ページ順
            let page_cmp = a.page.cmp(&b.page);
            if page_cmp != std::cmp::Ordering::Equal {
                return page_cmp;
            }

            // 2. 全幅要素はY座標順で他の要素と混在
            let a_full_width = Self::is_full_width(a, layouts);
            let b_full_width = Self::is_full_width(b, layouts);

            if a_full_width || b_full_width {
                // 全幅要素のY位置で比較
                return b.bbox.top.partial_cmp(&a.bbox.top)
                    .unwrap_or(std::cmp::Ordering::Equal);
            }

            // 3. 同一カラム内: Y座標順（上→下）
            if a.column_index == b.column_index {
                return b.bbox.top.partial_cmp(&a.bbox.top)
                    .unwrap_or(std::cmp::Ordering::Equal);
            }

            // 4. 異なるカラム: 左カラム優先
            a.column_index.cmp(&b.column_index)
        });
    }
}
```

### 2.5 figure — 画像処理層

```rust
// crates/pdf-lay-core/src/figure/mod.rs

mod caption_detector;
mod image_matcher;

pub use caption_detector::CaptionDetector;
pub use image_matcher::ImageMatcher;

// --- caption_detector.rs ---

pub struct CaptionDetector {
    /// キャプション検出用の正規表現パターン
    patterns: Vec<CaptionPattern>,
}

struct CaptionPattern {
    regex: Regex,
    caption_type: CaptionType,  // Figure or Table
}

enum CaptionType {
    Figure,
    Table,
}

impl CaptionDetector {
    pub fn new() -> Self {
        Self {
            patterns: vec![
                CaptionPattern {
                    regex: Regex::new(r"(?i)^(Fig\.?|Figure)\s*(\d+)\s*[:.]?\s*(.*)").unwrap(),
                    caption_type: CaptionType::Figure,
                },
                CaptionPattern {
                    regex: Regex::new(r"(?i)^(Table|Tab\.)\s*(\d+)\s*[:.]?\s*(.*)").unwrap(),
                    caption_type: CaptionType::Table,
                },
            ],
        }
    }

    /// キャプションブロックを検出
    pub fn detect(&self, blocks: &[TextBlock]) -> Vec<CaptionInfo> {
        blocks.iter().enumerate()
            .filter_map(|(i, block)| {
                let text = block.text.trim();
                for pattern in &self.patterns {
                    if let Some(caps) = pattern.regex.captures(text) {
                        return Some(CaptionInfo {
                            block_index: i,
                            caption_type: pattern.caption_type.clone(),
                            prefix: caps.get(1).unwrap().as_str().to_string(),
                            number: caps.get(2).unwrap().as_str().parse().ok(),
                            description: caps.get(3)
                                .map(|m| m.as_str().to_string())
                                .unwrap_or_default(),
                            full_text: text.to_string(),
                            page: block.page,
                            bbox: block.bbox.clone(),
                        });
                    }
                }
                None
            })
            .collect()
    }
}

// --- image_matcher.rs ---

pub struct ImageMatcher {
    max_gap_pt: f64,             // キャプション-画像間の最大距離（デフォルト: 50pt）
    prefer_caption_below: bool,  // キャプションが下にある想定（デフォルト: true）
}

impl ImageMatcher {
    /// キャプションと画像を空間的にマッチング
    pub fn match_all(
        &self,
        captions: &[CaptionInfo],
        images: &[ImageInfo],
        blocks: &[TextBlock],
    ) -> Vec<FigureInfo> {
        let mut used_images: HashSet<usize> = HashSet::new();
        let mut results = Vec::new();

        // キャプションごとに最も近い画像を探す
        for caption in captions.iter().filter(|c| c.caption_type == CaptionType::Figure) {
            let best = self.find_nearest_image(caption, images, &used_images);
            if let Some((img_idx, image)) = best {
                used_images.insert(img_idx);

                let context = self.extract_context(caption, blocks, 500);
                let insertion = self.determine_insertion(caption, &image, blocks);

                results.push(FigureInfo {
                    figure_id: format!("{} {}", caption.prefix, caption.number.unwrap_or(0)),
                    figure_number: caption.number,
                    caption_text: caption.full_text.clone(),
                    image: image.clone(),
                    context_text: context,
                    insertion_point: insertion,
                });
            }
        }

        results
    }

    fn find_nearest_image(
        &self,
        caption: &CaptionInfo,
        images: &[ImageInfo],
        used: &HashSet<usize>,
    ) -> Option<(usize, ImageInfo)> {
        images.iter().enumerate()
            .filter(|(idx, img)| img.page == caption.page && !used.contains(idx))
            .map(|(idx, img)| {
                let score = self.distance_score(caption, img);
                (idx, img.clone(), score)
            })
            .filter(|(_, _, score)| *score < self.max_gap_pt * 10.0)
            .min_by_key(|(_, _, score)| (*score * 100.0) as i64)
            .map(|(idx, img, _)| (idx, img))
    }

    fn distance_score(&self, caption: &CaptionInfo, image: &ImageInfo) -> f64 {
        let bbox = &image.normalized_bbox;

        // 垂直距離（キャプションが下にある想定）
        let v_dist = if self.prefer_caption_below {
            (bbox.bottom - caption.bbox.top).abs()
        } else {
            // 上下両方を探索
            let below = (bbox.bottom - caption.bbox.top).abs();
            let above = (caption.bbox.bottom - bbox.top).abs();
            below.min(above)
        };

        // 水平距離（中心間）
        let h_dist = (bbox.center_x() - caption.bbox.center_x()).abs();

        v_dist * 10.0 + h_dist  // 垂直を重視
    }

    /// キャプション前後の本文テキストを取得（コンテキスト）
    fn extract_context(
        &self,
        caption: &CaptionInfo,
        blocks: &[TextBlock],
        max_chars: usize,
    ) -> String {
        // caption.block_indexの前後のBodyTextブロックからテキストを収集
        // 最大max_chars文字
        ...
    }
}
```

### 2.6 output — 出力生成層

```rust
// crates/pdf-lay-core/src/output/mod.rs

mod markdown;
mod json;
mod chunker;

pub use markdown::MarkdownGenerator;
pub use json::JsonGenerator;
pub use chunker::Chunker;

// --- markdown.rs ---

pub struct MarkdownGenerator {
    config: MarkdownConfig,
}

pub struct MarkdownConfig {
    pub image_base_path: String,       // 画像の相対パスベース
    pub include_page_numbers: bool,    // ページ番号をコメントとして挿入
    pub heading_offset: u8,            // 見出しレベルのオフセット（デフォルト: 1 → ## から開始）
    pub include_metadata_header: bool, // タイトル・著者をYAML front matterで出力
    pub table_as_image: bool,          // テーブルを画像として埋め込むか
    pub figure_caption_style: CaptionStyle,
}

pub enum CaptionStyle {
    Italic,     // *Fig. 1: ...*
    Bold,       // **Fig. 1:** ...
    PlainText,  // Fig. 1: ...
}

impl MarkdownGenerator {
    pub fn generate(&self, doc: &PaperDocument) -> String {
        let mut md = String::with_capacity(doc.estimated_text_size());

        // YAML front matter（オプション）
        if self.config.include_metadata_header {
            md.push_str(&self.generate_front_matter(&doc.metadata));
        }

        // セクションを再帰的に出力
        for section in &doc.sections {
            self.write_section(&mut md, section, 0);
        }

        md
    }

    fn write_section(&self, md: &mut String, section: &Section, depth: usize) {
        // ヘッダー
        if let Some(header) = &section.header {
            let level = header.level + self.config.heading_offset;
            let prefix = "#".repeat(level as usize);
            md.push_str(&format!("{} {}\n\n", prefix, header.clean_text));
        }

        // ページ番号コメント（オプション）
        if self.config.include_page_numbers {
            md.push_str(&format!("<!-- page {} -->\n\n", section.page_range.0));
        }

        // ブロックとアセット（図・テーブル）を挿入位置順に出力
        let mut block_iter = section.blocks.iter().peekable();
        let mut figure_queue: VecDeque<_> = section.figures.iter().collect();
        let mut table_queue: VecDeque<_> = section.tables.iter().collect();

        for block in &section.blocks {
            // テキストブロック出力
            match block.block_type {
                BlockType::Caption | BlockType::PageNumber |
                BlockType::RunningHeader | BlockType::RunningFooter => continue,
                _ => {
                    md.push_str(&block.text);
                    md.push_str("\n\n");
                }
            }

            // このブロックの後に挿入すべき図を出力
            while let Some(fig) = figure_queue.front() {
                if fig.insertion_point.after_block_index == Some(block.global_index) {
                    self.write_figure(md, fig);
                    figure_queue.pop_front();
                } else {
                    break;
                }
            }

            // テーブルも同様
            while let Some(table) = table_queue.front() {
                if table.insertion_point.after_block_index == Some(block.global_index) {
                    self.write_table(md, table);
                    table_queue.pop_front();
                } else {
                    break;
                }
            }
        }

        // サブセクション
        for child in &section.children {
            self.write_section(md, child, depth + 1);
        }
    }

    fn write_figure(&self, md: &mut String, fig: &FigureInfo) {
        let path = format!("{}/{}", self.config.image_base_path,
            fig.image.path.file_name().unwrap().to_string_lossy());

        md.push_str(&format!("![{}]({})\n\n", fig.figure_id, path));

        match self.config.figure_caption_style {
            CaptionStyle::Italic => md.push_str(&format!("*{}*\n\n", fig.caption_text)),
            CaptionStyle::Bold => {
                // "Fig. 1:" を太字にして残りは通常テキスト
                md.push_str(&format!("**{}** {}\n\n", fig.figure_id, fig.caption_description()));
            }
            CaptionStyle::PlainText => md.push_str(&format!("{}\n\n", fig.caption_text)),
        }
    }
}

// --- chunker.rs ---

pub struct Chunker {
    config: ChunkConfig,
}

pub struct ChunkConfig {
    pub max_tokens: usize,      // デフォルト: 4000
    pub overlap_tokens: usize,  // デフォルト: 200
    pub split_strategy: SplitStrategy,
    pub include_section_context: bool,  // チャンクにセクション名を含めるか
}

pub enum SplitStrategy {
    SectionBoundary,  // セクション境界で分割（推奨）
    TokenCount,       // トークン数で機械的に分割
    Paragraph,        // 段落境界で分割
}

impl Chunker {
    pub fn chunk(&self, doc: &PaperDocument) -> Vec<Chunk> {
        match self.config.split_strategy {
            SplitStrategy::SectionBoundary => self.chunk_by_section(doc),
            SplitStrategy::TokenCount => self.chunk_by_tokens(doc),
            SplitStrategy::Paragraph => self.chunk_by_paragraph(doc),
        }
    }

    fn chunk_by_section(&self, doc: &PaperDocument) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut chunk_id = 0;

        for section in &doc.sections {
            let section_text = section.full_text();
            let estimated_tokens = Self::estimate_tokens(&section_text);

            if estimated_tokens <= self.config.max_tokens {
                // セクション全体が1チャンクに収まる
                chunks.push(Chunk {
                    chunk_id,
                    paper_id: doc.paper_id.clone(),
                    section: section.header_text(),
                    page_range: section.page_range,
                    text: section_text,
                    figures: section.figures.clone(),
                    tables: section.tables.clone(),
                    estimated_tokens,
                    has_continuation: false,
                });
                chunk_id += 1;
            } else {
                // セクションを段落単位で分割
                let sub_chunks = self.split_section(section, &mut chunk_id);
                chunks.extend(sub_chunks);
            }
        }

        chunks
    }

    /// トークン数の概算（英語: ~4文字/トークン, 日本語: ~1.5文字/トークン）
    fn estimate_tokens(text: &str) -> usize {
        let ascii_chars = text.chars().filter(|c| c.is_ascii()).count();
        let non_ascii_chars = text.chars().filter(|c| !c.is_ascii()).count();
        ascii_chars / 4 + (non_ascii_chars as f64 / 1.5) as usize
    }
}
```

### 2.7 selector — セクション選択層

```rust
// crates/pdf-lay-core/src/selector/mod.rs

mod toc;
mod selector;
mod llm_text;

pub use toc::TocGenerator;
pub use selector::SectionSelector;
pub use llm_text::LlmTextGenerator;

// --- toc.rs ---

/// セクション目次エントリ（軽量なメタデータのみ）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SectionEntry {
    pub index: usize,
    pub path: String,             // "2.1" or "INTRODUCTION"
    pub header: String,
    pub header_raw: String,
    pub level: u8,
    pub page_range: (u32, u32),
    pub estimated_tokens: usize,
    pub has_figures: bool,
    pub figure_count: usize,
    pub has_tables: bool,
    pub table_count: usize,
    pub children: Vec<SectionEntry>,
}

pub struct TocGenerator;

impl TocGenerator {
    /// PaperDocumentからセクション目次を生成
    pub fn generate(doc: &PaperDocument) -> Vec<SectionEntry> {
        doc.sections.iter().enumerate()
            .map(|(i, section)| Self::section_to_entry(section, i))
            .collect()
    }

    fn section_to_entry(section: &Section, index: usize) -> SectionEntry {
        let text = section.full_text();
        SectionEntry {
            index,
            path: section.header.as_ref()
                .map(|h| h.numbering.clone().unwrap_or_else(|| h.clean_text.clone()))
                .unwrap_or_else(|| format!("section_{}", index)),
            header: section.header_text(),
            header_raw: section.header.as_ref()
                .map(|h| h.text.clone())
                .unwrap_or_default(),
            level: section.level,
            page_range: section.page_range,
            estimated_tokens: Chunker::estimate_tokens(&text),
            has_figures: !section.figures.is_empty(),
            figure_count: section.figures.len(),
            has_tables: !section.tables.is_empty(),
            table_count: section.tables.len(),
            children: section.children.iter().enumerate()
                .map(|(i, child)| Self::section_to_entry(child, i))
                .collect(),
        }
    }
}

// --- selector.rs ---

/// セクション選択の結果を保持し、各種出力メソッドを提供
pub struct SectionSelector<'a> {
    doc: &'a PaperDocument,
    selected: Vec<&'a Section>,
}

impl<'a> SectionSelector<'a> {
    /// ヘッダー名で選択（部分一致、大文字小文字無視）
    pub fn by_names(doc: &'a PaperDocument, names: &[&str]) -> Self {
        let selected = Self::collect_matching_sections(&doc.sections, names);
        Self { doc, selected }
    }

    /// インデックスで選択
    pub fn by_indices(doc: &'a PaperDocument, indices: &[usize]) -> Self {
        let flat = Self::flatten_sections(&doc.sections);
        let selected = indices.iter()
            .filter_map(|&i| flat.get(i).copied())
            .collect();
        Self { doc, selected }
    }

    /// レベルでフィルタ
    pub fn by_level(doc: &'a PaperDocument, level: u8) -> Self {
        let selected = Self::flatten_sections(&doc.sections).into_iter()
            .filter(|s| s.level == level)
            .collect();
        Self { doc, selected }
    }

    /// ページ範囲で選択
    pub fn by_pages(doc: &'a PaperDocument, start: u32, end: u32) -> Self {
        let selected = Self::flatten_sections(&doc.sections).into_iter()
            .filter(|s| s.page_range.0 >= start && s.page_range.1 <= end)
            .collect();
        Self { doc, selected }
    }

    /// 述語関数で選択
    pub fn by_predicate<F>(doc: &'a PaperDocument, pred: F) -> Self
    where F: Fn(&SectionEntry) -> bool {
        let toc = TocGenerator::generate(doc);
        let flat = Self::flatten_sections(&doc.sections);
        let flat_toc = Self::flatten_entries(&toc);

        let selected = flat.into_iter().zip(flat_toc.iter())
            .filter(|(_, entry)| pred(entry))
            .map(|(section, _)| section)
            .collect();
        Self { doc, selected }
    }

    // --- 出力メソッド ---

    pub fn to_markdown(&self, config: &MarkdownConfig) -> String {
        let gen = MarkdownGenerator::new(config.clone());
        gen.generate_for_sections(&self.selected)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.selected)
    }

    pub fn to_llm_text(&self, config: &LlmTextConfig) -> String {
        LlmTextGenerator::new(config.clone()).generate(&self.selected)
    }

    pub fn to_chunks(&self, config: &ChunkConfig) -> Vec<Chunk> {
        Chunker::new(config.clone()).chunk_sections(&self.selected)
    }

    pub fn total_estimated_tokens(&self) -> usize {
        self.selected.iter()
            .map(|s| Chunker::estimate_tokens(&s.full_text()))
            .sum()
    }

    pub fn sections(&self) -> &[&Section] {
        &self.selected
    }

    // --- 内部ヘルパー ---

    fn collect_matching_sections(
        sections: &'a [Section],
        names: &[&str],
    ) -> Vec<&'a Section> {
        let mut result = Vec::new();
        for section in sections {
            let header_text = section.header_text().to_uppercase();
            let clean_text = section.header.as_ref()
                .map(|h| h.clean_text.to_uppercase())
                .unwrap_or_default();

            let matches = names.iter().any(|name| {
                let upper_name = name.to_uppercase();
                // 完全一致
                header_text == upper_name || clean_text == upper_name
                // 部分一致
                || header_text.contains(&upper_name)
                || clean_text.contains(&upper_name)
            });

            if matches {
                result.push(section);
                // 子セクションも自動的に含まれる（Sectionの中にchildrenがあるため）
            } else {
                // 子セクションも再帰的にチェック
                result.extend(Self::collect_matching_sections(&section.children, names));
            }
        }
        result
    }

    fn flatten_sections(sections: &'a [Section]) -> Vec<&'a Section> {
        let mut result = Vec::new();
        for section in sections {
            result.push(section);
            result.extend(Self::flatten_sections(&section.children));
        }
        result
    }
}

// --- llm_text.rs ---

/// LLMに渡すのに最適化されたプレーンテキスト生成
pub struct LlmTextGenerator {
    config: LlmTextConfig,
}

pub struct LlmTextConfig {
    pub include_figures: bool,
    pub include_tables: bool,
    pub include_section_headers: bool,
    pub math_representation: MathRepresentationPreference,
    pub figure_format: FigureTextFormat,
}

pub enum FigureTextFormat {
    /// [IMAGE: Fig. 1 path/to/img.png]
    Placeholder,
    /// ![Fig. 1](path/to/img.png)
    MarkdownLink,
    /// キャプションテキストのみ
    CaptionOnly,
    /// 図を完全に省略
    Omit,
}

impl LlmTextGenerator {
    pub fn generate(&self, sections: &[&Section]) -> String {
        let mut output = String::new();

        for section in sections {
            // セクションヘッダー
            if self.config.include_section_headers {
                if let Some(header) = &section.header {
                    output.push_str(&format!("## {}\n\n", header.clean_text));
                }
            }

            // ブロックテキスト
            for block in &section.blocks {
                match block.block_type {
                    BlockType::BodyText | BlockType::Abstract => {
                        output.push_str(&block.text);
                        output.push_str("\n\n");
                    }
                    _ => {}
                }
            }

            // テーブル（インラインテキスト）
            if self.config.include_tables {
                for table in &section.tables {
                    if let Some(caption) = &table.caption {
                        output.push_str(&format!("**{}**\n\n", caption));
                    }
                    match &table.representation {
                        TableRepresentation::Markdown { markdown_text, .. } => {
                            output.push_str(markdown_text);
                        }
                        TableRepresentation::Csv { csv_text, .. } => {
                            output.push_str(csv_text);
                        }
                        TableRepresentation::PlainText { text, .. } => {
                            output.push_str(text);
                        }
                    }
                    output.push_str("\n\n");
                }
            }

            // 図
            if self.config.include_figures {
                for fig in &section.figures {
                    match self.config.figure_format {
                        FigureTextFormat::Placeholder => {
                            output.push_str(&format!(
                                "[IMAGE: {} {}]\n",
                                fig.figure_id,
                                fig.image.path.display()
                            ));
                        }
                        FigureTextFormat::MarkdownLink => {
                            output.push_str(&format!(
                                "![{}]({})\n",
                                fig.figure_id,
                                fig.image.path.display()
                            ));
                        }
                        FigureTextFormat::CaptionOnly => {
                            output.push_str(&format!("[{}]\n", fig.caption_text));
                        }
                        FigureTextFormat::Omit => {}
                    }
                    output.push('\n');
                }
            }

            // サブセクション（再帰）
            if !section.children.is_empty() {
                let child_refs: Vec<&Section> = section.children.iter().collect();
                output.push_str(&self.generate(&child_refs));
            }
        }

        output
    }
}
```

### 2.8 table — テーブル処理層

```rust
// crates/pdf-lay-core/src/table/mod.rs

mod detector;
mod grid_builder;
mod text_converter;

pub use detector::TableDetector;
pub use grid_builder::GridBuilder;
pub use text_converter::TableTextConverter;

// --- detector.rs ---

pub struct TableDetector {
    config: TableConfig,
}

pub struct TableConfig {
    /// テーブル検出の最小列数
    pub min_columns: usize,              // デフォルト: 2
    /// 列アラインメントの許容誤差（pt）
    pub column_alignment_tolerance: f64,  // デフォルト: 5.0
    /// 罫線検出を有効化
    pub use_rule_detection: bool,         // デフォルト: true
    /// テキスト座標ベースの検出を有効化
    pub use_text_alignment: bool,         // デフォルト: true
}

impl TableDetector {
    /// テーブル領域を検出
    pub fn detect(
        &self,
        blocks: &[TextBlock],
        paths: &[PathObject],
        captions: &[CaptionInfo],
    ) -> Vec<TableRegion> {
        let mut tables = Vec::new();

        // 方法1: 罫線ベース（最も精度が高い）
        if self.config.use_rule_detection {
            tables.extend(self.detect_by_rules(paths, blocks));
        }

        // 方法2: テキスト座標ベース
        if self.config.use_text_alignment {
            let text_tables = self.detect_by_text_alignment(blocks, captions);
            // 罫線ベースで検出済みの領域と重複しないものだけ追加
            for t in text_tables {
                if !tables.iter().any(|existing| existing.overlaps(&t)) {
                    tables.push(t);
                }
            }
        }

        // テーブルキャプションとの対応付け
        self.associate_captions(&mut tables, captions);

        tables
    }

    fn detect_by_rules(
        &self,
        paths: &[PathObject],
        blocks: &[TextBlock],
    ) -> Vec<TableRegion> {
        // 水平線と垂直線を抽出
        // グリッドパターンを検出
        // グリッドセル内のテキストブロックを割り当て
        ...
    }

    fn detect_by_text_alignment(
        &self,
        blocks: &[TextBlock],
        captions: &[CaptionInfo],
    ) -> Vec<TableRegion> {
        // "Table" キャプションの直下の領域を探索
        // 短いテキストブロックのX座標アラインメントを検出
        // 3列以上のアラインメント → テーブル候補
        ...
    }
}

// --- grid_builder.rs ---

/// テーブル領域からグリッド構造（行×列）を構築
pub struct GridBuilder;

impl GridBuilder {
    pub fn build(region: &TableRegion, blocks: &[TextBlock]) -> TableGrid {
        // 1. セル境界の推定
        //    - 罫線がある場合: 罫線交点からセル分割
        //    - ない場合: テキストのX/Y座標クラスタリング
        //
        // 2. テキストブロックをセルに割り当て
        //
        // 3. ヘッダー行の検出
        //    - 最初の行が太字 → ヘッダー
        //    - 水平罫線で区切られた最初の行 → ヘッダー
        //
        // 4. 結合セルの検出
        //    - 1セルが複数列にまたがる → colspan
        ...
    }
}

#[derive(Debug, Clone)]
pub struct TableGrid {
    pub header: Vec<Vec<String>>,      // ヘッダー行（複数行の可能性）
    pub rows: Vec<Vec<String>>,        // データ行
    pub column_count: usize,
    pub has_multi_header: bool,
}

// --- text_converter.rs ---

/// テーブルグリッドをテキスト表現に変換
pub struct TableTextConverter;

impl TableTextConverter {
    /// テーブルをMarkdownテーブルに変換
    pub fn to_markdown(grid: &TableGrid) -> TableRepresentation {
        let mut md = String::new();

        // ヘッダー
        let header = if grid.has_multi_header {
            // マルチヘッダーをフラット化
            Self::flatten_multi_header(&grid.header)
        } else {
            grid.header.first().cloned().unwrap_or_default()
        };

        // ヘッダー行
        md.push_str("| ");
        md.push_str(&header.join(" | "));
        md.push_str(" |\n");

        // セパレータ
        md.push_str("|");
        for _ in &header {
            md.push_str("---|");
        }
        md.push('\n');

        // データ行
        for row in &grid.rows {
            md.push_str("| ");
            let cells: Vec<String> = row.iter()
                .map(|cell| Self::escape_cell(cell))
                .collect();
            md.push_str(&cells.join(" | "));
            md.push_str(" |\n");
        }

        TableRepresentation::Markdown {
            header: header.clone(),
            rows: grid.rows.clone(),
            caption: None,
            markdown_text: md,
        }
    }

    /// セル内の特殊要素を処理
    fn escape_cell(cell: &str) -> String {
        cell.replace('|', "\\|")       // パイプのエスケープ
            .replace('\n', " / ")       // セル内改行 → スラッシュ区切り
            .trim()
            .to_string()
    }

    /// マルチヘッダーをフラット化
    /// 例: [["", "Metrics", "Metrics"], ["Method", "Acc", "F1"]]
    ///   → ["Method", "Metrics_Acc", "Metrics_F1"]
    fn flatten_multi_header(headers: &[Vec<String>]) -> Vec<String> {
        if headers.len() <= 1 {
            return headers.first().cloned().unwrap_or_default();
        }

        let col_count = headers.iter().map(|h| h.len()).max().unwrap_or(0);
        let mut result = Vec::with_capacity(col_count);

        for col in 0..col_count {
            let parts: Vec<&str> = headers.iter()
                .filter_map(|row| row.get(col).map(|s| s.as_str()))
                .filter(|s| !s.is_empty())
                .collect();

            if parts.len() > 1 {
                result.push(parts.join("_"));
            } else {
                result.push(parts.first().unwrap_or(&"").to_string());
            }
        }

        result
    }

    /// テーブルをCSVテキストに変換（フォールバック）
    pub fn to_csv(grid: &TableGrid) -> TableRepresentation {
        let mut csv = String::new();
        let header = grid.header.first().cloned().unwrap_or_default();
        csv.push_str(&header.join(","));
        csv.push('\n');
        for row in &grid.rows {
            let cells: Vec<String> = row.iter()
                .map(|c| if c.contains(',') { format!("\"{}\"", c) } else { c.clone() })
                .collect();
            csv.push_str(&cells.join(","));
            csv.push('\n');
        }
        TableRepresentation::Csv {
            header, rows: grid.rows.clone(), caption: None, csv_text: csv,
        }
    }
}
```

### 2.9 math — 数式検出・変換層

```rust
// crates/pdf-lay-core/src/math/mod.rs

mod detector;
mod converter;
mod symbol_map;

pub use detector::MathDetector;
pub use converter::MathConverter;

// --- detector.rs ---

pub struct MathDetector {
    math_font_patterns: Vec<Regex>,
    symbol_codepoints: HashSet<char>,
}

impl MathDetector {
    pub fn new() -> Self {
        Self {
            math_font_patterns: vec![
                Regex::new(r"(?i)^CM[A-Z]{1,4}\d").unwrap(),   // Computer Modern
                Regex::new(r"(?i)math").unwrap(),
                Regex::new(r"(?i)symbol").unwrap(),
                Regex::new(r"(?i)^MT").unwrap(),                 // MathTime
                Regex::new(r"(?i)STIX").unwrap(),
            ],
            symbol_codepoints: symbol_map::math_symbols(),
        }
    }

    /// テキストスパンが数式フォントを使用しているか
    pub fn is_math_span(&self, span: &TextSpan) -> bool {
        self.math_font_patterns.iter().any(|p| p.is_match(&span.font_name))
            || span.text.chars().any(|c| self.symbol_codepoints.contains(&c))
    }

    /// 行内の数式領域を検出
    pub fn detect_in_line(&self, line: &TextLine) -> Vec<MathRegion> {
        let mut regions = Vec::new();
        let mut math_start: Option<usize> = None;

        for (i, span) in line.spans.iter().enumerate() {
            if self.is_math_span(span) {
                if math_start.is_none() {
                    math_start = Some(i);
                }
            } else if let Some(start) = math_start {
                regions.push(MathRegion {
                    spans: line.spans[start..i].to_vec(),
                    context: MathContext::Inline,
                    bbox: Self::compute_bbox(&line.spans[start..i]),
                    page: line.page,
                });
                math_start = None;
            }
        }

        // 行全体が数式の場合
        if let Some(start) = math_start {
            let all_math = start == 0 && line.spans.len() > 1;
            regions.push(MathRegion {
                spans: line.spans[start..].to_vec(),
                context: if all_math { MathContext::Display { equation_number: None } }
                         else { MathContext::Inline },
                bbox: Self::compute_bbox(&line.spans[start..]),
                page: line.page,
            });
        }

        regions
    }

    /// ディスプレイ数式（独立行）の検出
    /// 特徴: 行全体が数式フォント、センタリングされている、前後に空行
    pub fn detect_display_equations(
        &self,
        lines: &[TextLine],
        page_dims: &PageDimensions,
    ) -> Vec<MathRegion> {
        let mut equations = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            let all_math = line.spans.iter().all(|s| self.is_math_span(s));
            if !all_math { continue; }

            // センタリング検出
            let center_x = line.bbox.center_x();
            let page_center = page_dims.width / 2.0;
            let is_centered = (center_x - page_center).abs() < page_dims.width * 0.15;

            if is_centered {
                // 式番号の検出: 行末の "(1)" や "(2.3)" パターン
                let eq_number = self.detect_equation_number(line);

                equations.push(MathRegion {
                    spans: line.spans.clone(),
                    context: MathContext::Display { equation_number: eq_number },
                    bbox: line.bbox.clone(),
                    page: line.page,
                });
            }
        }

        equations
    }

    fn detect_equation_number(&self, line: &TextLine) -> Option<String> {
        let last_span = line.spans.last()?;
        let re = Regex::new(r"\((\d+(?:\.\d+)*)\)").ok()?;
        re.captures(&last_span.text)
            .map(|c| c.get(1).unwrap().as_str().to_string())
    }
}

#[derive(Debug, Clone)]
pub struct MathRegion {
    pub spans: Vec<TextSpan>,
    pub context: MathContext,
    pub bbox: Rect,
    pub page: u32,
}

#[derive(Debug, Clone)]
pub enum MathContext {
    Inline,
    Display { equation_number: Option<String> },
}

// --- converter.rs ---

/// 数式スパンをテキスト表現に変換
pub struct MathConverter {
    config: MathConfig,
    symbol_to_latex: HashMap<char, &'static str>,
    symbol_to_unicode: HashMap<char, char>,
}

impl MathConverter {
    pub fn new(config: MathConfig) -> Self {
        Self {
            config,
            symbol_to_latex: symbol_map::to_latex_map(),
            symbol_to_unicode: symbol_map::to_unicode_map(),
        }
    }

    /// 数式領域をテキストに変換
    pub fn convert(&self, region: &MathRegion) -> MathRepresentation {
        match self.config.representation {
            MathRepresentationPreference::LaTeX => self.to_latex(region),
            MathRepresentationPreference::UnicodeMath => self.to_unicode(region),
            MathRepresentationPreference::PlainText => self.to_plain(region),
            MathRepresentationPreference::Auto => self.auto_convert(region),
        }
    }

    fn auto_convert(&self, region: &MathRegion) -> MathRepresentation {
        // CM（Computer Modern）フォント → LaTeX由来の可能性が高い → LaTeX
        let has_cm = region.spans.iter()
            .any(|s| s.font_name.starts_with("CM") || s.font_name.starts_with("cm"));
        if has_cm {
            self.to_latex(region)
        } else {
            self.to_unicode(region)
        }
    }

    fn to_latex(&self, region: &MathRegion) -> MathRepresentation {
        let mut latex = String::new();
        let base_y = self.detect_baseline(&region.spans);
        let base_size = self.detect_base_font_size(&region.spans);

        for span in &region.spans {
            let text = &span.text;

            // 上付き文字検出
            let y_offset = span.bbox.bottom - base_y;
            let size_ratio = span.font_size / base_size;

            if y_offset > base_size * self.config.superscript_y_threshold
                && size_ratio < 0.85 {
                latex.push_str(&format!("^{{{}}}", self.map_symbols_latex(text)));
                continue;
            }

            // 下付き文字検出
            if y_offset < -(base_size * self.config.superscript_y_threshold)
                && size_ratio < 0.85 {
                latex.push_str(&format!("_{{{}}}", self.map_symbols_latex(text)));
                continue;
            }

            // 通常テキスト（記号変換あり）
            latex.push_str(&self.map_symbols_latex(text));
        }

        MathRepresentation::LaTeX(latex)
    }

    fn to_unicode(&self, region: &MathRegion) -> MathRepresentation {
        let mut text = String::new();
        let base_y = self.detect_baseline(&region.spans);
        let base_size = self.detect_base_font_size(&region.spans);

        for span in &region.spans {
            let y_offset = span.bbox.bottom - base_y;
            let size_ratio = span.font_size / base_size;

            if y_offset > base_size * self.config.superscript_y_threshold
                && size_ratio < 0.85 {
                // Unicode上付き文字に変換: 2 → ², 3 → ³
                text.push_str(&self.to_unicode_superscript(&span.text));
            } else if y_offset < -(base_size * self.config.superscript_y_threshold)
                && size_ratio < 0.85 {
                // Unicode下付き文字に変換: 1 → ₁, 2 → ₂
                text.push_str(&self.to_unicode_subscript(&span.text));
            } else {
                text.push_str(&span.text);
            }
        }

        MathRepresentation::UnicodeMath(text)
    }

    fn to_plain(&self, region: &MathRegion) -> MathRepresentation {
        let mut text = String::new();
        let base_y = self.detect_baseline(&region.spans);
        let base_size = self.detect_base_font_size(&region.spans);

        for span in &region.spans {
            let y_offset = span.bbox.bottom - base_y;
            let size_ratio = span.font_size / base_size;

            if y_offset > base_size * 0.3 && size_ratio < 0.85 {
                text.push_str(&format!("^{}", span.text));
            } else if y_offset < -0.3 && size_ratio < 0.85 {
                text.push_str(&format!("_{}", span.text));
            } else {
                text.push_str(&span.text);
            }
        }

        MathRepresentation::PlainText(text)
    }

    fn map_symbols_latex(&self, text: &str) -> String {
        text.chars()
            .map(|c| {
                self.symbol_to_latex.get(&c)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| c.to_string())
            })
            .collect()
    }

    fn detect_baseline(&self, spans: &[TextSpan]) -> f64 {
        // 最頻出のY位置 = ベースライン
        // フォントサイズが最大のスパンのbottomを基準にする
        spans.iter()
            .max_by(|a, b| a.font_size.partial_cmp(&b.font_size).unwrap())
            .map(|s| s.bbox.bottom)
            .unwrap_or(0.0)
    }

    fn detect_base_font_size(&self, spans: &[TextSpan]) -> f64 {
        spans.iter()
            .map(|s| s.font_size)
            .fold(f64::MIN, f64::max)
    }

    fn to_unicode_superscript(&self, text: &str) -> String {
        text.chars().map(|c| match c {
            '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³',
            '4' => '⁴', '5' => '⁵', '6' => '⁶', '7' => '⁷',
            '8' => '⁸', '9' => '⁹', '+' => '⁺', '-' => '⁻',
            'n' => 'ⁿ', 'i' => 'ⁱ',
            _ => c,
        }).collect()
    }

    fn to_unicode_subscript(&self, text: &str) -> String {
        text.chars().map(|c| match c {
            '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃',
            '4' => '₄', '5' => '₅', '6' => '₆', '7' => '₇',
            '8' => '₈', '9' => '₉', '+' => '₊', '-' => '₋',
            'i' => 'ᵢ', 'n' => 'ₙ',
            _ => c,
        }).collect()
    }
}

/// Markdown内での数式フォーマッティング
pub struct MathFormatter;

impl MathFormatter {
    /// 数式表現をMarkdownに埋め込む形式に変換
    pub fn format_for_markdown(
        repr: &MathRepresentation,
        context: &MathContext,
        config: &MathConfig,
    ) -> String {
        match (repr, context) {
            (MathRepresentation::LaTeX(latex), MathContext::Inline) => {
                format!("{}{}{}", config.inline_delimiter.0, latex, config.inline_delimiter.1)
            }
            (MathRepresentation::LaTeX(latex), MathContext::Display { equation_number }) => {
                let eq_tag = equation_number.as_ref()
                    .map(|n| format!(" \\tag{{{}}}", n))
                    .unwrap_or_default();
                format!("{}{}{}{}", config.display_delimiter.0, latex, eq_tag, config.display_delimiter.1)
            }
            (MathRepresentation::UnicodeMath(text), _) |
            (MathRepresentation::PlainText(text), _) => {
                text.clone()
            }
        }
    }
}

// --- symbol_map.rs ---

pub fn to_latex_map() -> HashMap<char, &'static str> {
    let mut m = HashMap::new();
    m.insert('∑', "\\sum");
    m.insert('∏', "\\prod");
    m.insert('∫', "\\int");
    m.insert('∮', "\\oint");
    m.insert('√', "\\sqrt");
    m.insert('∞', "\\infty");
    m.insert('∂', "\\partial");
    m.insert('∇', "\\nabla");
    m.insert('∈', "\\in");
    m.insert('∉', "\\notin");
    m.insert('∀', "\\forall");
    m.insert('∃', "\\exists");
    m.insert('±', "\\pm");
    m.insert('×', "\\times");
    m.insert('÷', "\\div");
    m.insert('≤', "\\leq");
    m.insert('≥', "\\geq");
    m.insert('≠', "\\neq");
    m.insert('≈', "\\approx");
    m.insert('→', "\\rightarrow");
    m.insert('←', "\\leftarrow");
    m.insert('↔', "\\leftrightarrow");
    m.insert('⇒', "\\Rightarrow");
    m.insert('⇐', "\\Leftarrow");
    m.insert('⇔', "\\Leftrightarrow");
    m.insert('α', "\\alpha");
    m.insert('β', "\\beta");
    m.insert('γ', "\\gamma");
    m.insert('δ', "\\delta");
    m.insert('ε', "\\epsilon");
    m.insert('θ', "\\theta");
    m.insert('λ', "\\lambda");
    m.insert('μ', "\\mu");
    m.insert('π', "\\pi");
    m.insert('σ', "\\sigma");
    m.insert('φ', "\\phi");
    m.insert('ω', "\\omega");
    m.insert('Σ', "\\Sigma");
    m.insert('Π', "\\Pi");
    m.insert('Ω', "\\Omega");
    m
}
```

```

### 2.10 PyO3 バインディング

```rust
// crates/pdflay-python/src/lib.rs

use pyo3::prelude::*;
use pdf_lay_core::*;

/// Python向けのメインドキュメントクラス
#[pyclass]
#[derive(Clone)]
struct PyPaperDocument {
    inner: PaperDocument,
}

#[pymethods]
impl PyPaperDocument {
    #[getter]
    fn metadata(&self) -> PyMetadata {
        PyMetadata { inner: self.inner.metadata.clone() }
    }

    #[getter]
    fn sections(&self) -> Vec<PySection> {
        self.inner.sections.iter()
            .map(|s| PySection { inner: s.clone() })
            .collect()
    }

    #[getter]
    fn figures(&self) -> Vec<PyFigureInfo> {
        self.inner.all_figures().iter()
            .map(|f| PyFigureInfo { inner: f.clone() })
            .collect()
    }

    /// Markdown出力
    #[pyo3(signature = (image_base_path="./images", include_page_numbers=false, heading_offset=1))]
    fn to_markdown(
        &self,
        image_base_path: &str,
        include_page_numbers: bool,
        heading_offset: u8,
    ) -> String {
        let config = MarkdownConfig {
            image_base_path: image_base_path.to_string(),
            include_page_numbers,
            heading_offset,
            ..Default::default()
        };
        MarkdownGenerator::new(config).generate(&self.inner)
    }

    /// JSON出力
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string_pretty(&self.inner)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))
    }

    /// LLM向けチャンク分割
    #[pyo3(signature = (max_tokens=4000, overlap=200, strategy="section"))]
    fn to_chunks(
        &self,
        max_tokens: usize,
        overlap: usize,
        strategy: &str,
    ) -> Vec<PyChunk> {
        let split_strategy = match strategy {
            "section" => SplitStrategy::SectionBoundary,
            "token" => SplitStrategy::TokenCount,
            "paragraph" => SplitStrategy::Paragraph,
            _ => SplitStrategy::SectionBoundary,
        };
        let config = ChunkConfig { max_tokens, overlap_tokens: overlap, split_strategy, ..Default::default() };
        Chunker::new(config).chunk(&self.inner).into_iter()
            .map(|c| PyChunk { inner: c })
            .collect()
    }

    /// セクション一覧（目次）を取得
    fn toc(&self) -> Vec<PySectionEntry> {
        TocGenerator::generate(&self.inner).into_iter()
            .map(|e| PySectionEntry { inner: e })
            .collect()
    }

    /// ヘッダー名でセクションを選択（部分一致、大文字小文字無視）
    fn select_sections(&self, names: Vec<String>) -> PySectionSelector {
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let selector = SectionSelector::by_names(&self.inner, &name_refs);
        PySectionSelector {
            doc: self.inner.clone(),
            selected_indices: selector.selected_indices(),
        }
    }

    /// インデックスでセクションを選択
    fn select_sections_by_index(&self, indices: Vec<usize>) -> PySectionSelector {
        let selector = SectionSelector::by_indices(&self.inner, &indices);
        PySectionSelector {
            doc: self.inner.clone(),
            selected_indices: selector.selected_indices(),
        }
    }

    /// レベルでセクションを選択
    fn select_sections_by_level(&self, level: u8) -> PySectionSelector {
        let selector = SectionSelector::by_level(&self.inner, level);
        PySectionSelector {
            doc: self.inner.clone(),
            selected_indices: selector.selected_indices(),
        }
    }

    /// ページ範囲でセクションを選択
    fn select_sections_by_pages(&self, start: u32, end: u32) -> PySectionSelector {
        let selector = SectionSelector::by_pages(&self.inner, start, end);
        PySectionSelector {
            doc: self.inner.clone(),
            selected_indices: selector.selected_indices(),
        }
    }
}

/// セクション目次エントリ
#[pyclass]
#[derive(Clone)]
struct PySectionEntry {
    inner: SectionEntry,
}

#[pymethods]
impl PySectionEntry {
    #[getter] fn index(&self) -> usize { self.inner.index }
    #[getter] fn header(&self) -> String { self.inner.header.clone() }
    #[getter] fn header_raw(&self) -> String { self.inner.header_raw.clone() }
    #[getter] fn level(&self) -> u8 { self.inner.level }
    #[getter] fn page_range(&self) -> (u32, u32) { self.inner.page_range }
    #[getter] fn estimated_tokens(&self) -> usize { self.inner.estimated_tokens }
    #[getter] fn has_figures(&self) -> bool { self.inner.has_figures }
    #[getter] fn figure_count(&self) -> usize { self.inner.figure_count }
    #[getter] fn has_tables(&self) -> bool { self.inner.has_tables }
    #[getter] fn table_count(&self) -> usize { self.inner.table_count }
    #[getter]
    fn children(&self) -> Vec<PySectionEntry> {
        self.inner.children.iter()
            .map(|c| PySectionEntry { inner: c.clone() })
            .collect()
    }

    fn __repr__(&self) -> String {
        format!("[L{}] {} (p.{}-{}, ~{} tokens, fig:{}, tab:{})",
            self.inner.level, self.inner.header,
            self.inner.page_range.0, self.inner.page_range.1,
            self.inner.estimated_tokens,
            self.inner.figure_count, self.inner.table_count)
    }
}

/// セクション選択結果
#[pyclass]
#[derive(Clone)]
struct PySectionSelector {
    doc: PaperDocument,
    selected_indices: Vec<usize>,
}

#[pymethods]
impl PySectionSelector {
    /// 選択セクションのMarkdown出力
    #[pyo3(signature = (image_base_path="./images"))]
    fn to_markdown(&self, image_base_path: &str) -> String {
        let selector = self.rebuild_selector();
        let config = MarkdownConfig {
            image_base_path: image_base_path.to_string(),
            ..Default::default()
        };
        selector.to_markdown(&config)
    }

    /// 選択セクションのJSON出力
    fn to_json(&self) -> PyResult<String> {
        let selector = self.rebuild_selector();
        selector.to_json()
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))
    }

    /// LLM向けテキスト出力（テーブルインライン、図プレースホルダ）
    #[pyo3(signature = (include_figures=true, include_tables=true, figure_format="placeholder"))]
    fn to_llm_text(
        &self,
        include_figures: bool,
        include_tables: bool,
        figure_format: &str,
    ) -> String {
        let selector = self.rebuild_selector();
        let config = LlmTextConfig {
            include_figures,
            include_tables,
            include_section_headers: true,
            figure_format: match figure_format {
                "markdown" => FigureTextFormat::MarkdownLink,
                "caption" => FigureTextFormat::CaptionOnly,
                "omit" => FigureTextFormat::Omit,
                _ => FigureTextFormat::Placeholder,
            },
            ..Default::default()
        };
        selector.to_llm_text(&config)
    }

    /// チャンク分割
    #[pyo3(signature = (max_tokens=4000, overlap=200))]
    fn to_chunks(&self, max_tokens: usize, overlap: usize) -> Vec<PyChunk> {
        let selector = self.rebuild_selector();
        let config = ChunkConfig {
            max_tokens,
            overlap_tokens: overlap,
            ..Default::default()
        };
        selector.to_chunks(&config).into_iter()
            .map(|c| PyChunk { inner: c })
            .collect()
    }

    /// 合計推定トークン数
    fn total_estimated_tokens(&self) -> usize {
        let selector = self.rebuild_selector();
        selector.total_estimated_tokens()
    }

    fn rebuild_selector(&self) -> SectionSelector {
        SectionSelector::by_indices(&self.doc, &self.selected_indices)
    }
}

#[pyclass]
#[derive(Clone)]
struct PySection {
    inner: Section,
}

#[pymethods]
impl PySection {
    #[getter]
    fn header(&self) -> Option<String> {
        self.inner.header.as_ref().map(|h| h.clean_text.clone())
    }

    #[getter]
    fn header_raw(&self) -> Option<String> {
        self.inner.header.as_ref().map(|h| h.text.clone())
    }

    #[getter]
    fn level(&self) -> u8 { self.inner.level }

    #[getter]
    fn text(&self) -> String { self.inner.full_text() }

    #[getter]
    fn page_range(&self) -> (u32, u32) { self.inner.page_range }

    #[getter]
    fn figures(&self) -> Vec<PyFigureInfo> {
        self.inner.figures.iter()
            .map(|f| PyFigureInfo { inner: f.clone() })
            .collect()
    }

    #[getter]
    fn children(&self) -> Vec<PySection> {
        self.inner.children.iter()
            .map(|s| PySection { inner: s.clone() })
            .collect()
    }
}

#[pyclass]
#[derive(Clone)]
struct PyChunk {
    inner: Chunk,
}

#[pymethods]
impl PyChunk {
    #[getter]
    fn chunk_id(&self) -> usize { self.inner.chunk_id }
    #[getter]
    fn section(&self) -> String { self.inner.section.clone() }
    #[getter]
    fn text(&self) -> String { self.inner.text.clone() }
    #[getter]
    fn estimated_tokens(&self) -> usize { self.inner.estimated_tokens }
    #[getter]
    fn page_range(&self) -> (u32, u32) { self.inner.page_range }
    #[getter]
    fn has_continuation(&self) -> bool { self.inner.has_continuation }
}

// --- トップレベル関数 ---

/// PDFを解析してPaperDocumentを返す
#[pyfunction]
#[pyo3(signature = (path, image_dir="./images", extract_images=true, detect_tables=true))]
fn analyze(
    path: &str,
    image_dir: &str,
    extract_images: bool,
    detect_tables: bool,
) -> PyResult<PyPaperDocument> {
    let config = Config {
        image_output_dir: PathBuf::from(image_dir),
        extract_images,
        detect_tables,
        ..Default::default()
    };
    let doc = pdf_lay_core::analyze_pdf(Path::new(path), &config)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
    Ok(PyPaperDocument { inner: doc })
}

/// バッチ解析（並列処理）
#[pyfunction]
#[pyo3(signature = (paths, image_dir="./images", parallel=true))]
fn analyze_batch(
    paths: Vec<String>,
    image_dir: &str,
    parallel: bool,
) -> PyResult<Vec<PyPaperDocument>> {
    // rayon::par_iter で並列処理（parallel=true時）
    ...
}

/// 段階的API: テキストスパン抽出
#[pyfunction]
fn extract_spans(path: &str) -> PyResult<Vec<PyTextSpan>> { ... }

/// 段階的API: 行再構成
#[pyfunction]
fn reconstruct_lines(spans: Vec<PyTextSpan>) -> Vec<PyTextLine> { ... }

// --- モジュール登録 ---

#[pymodule]
fn pdflay(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(analyze, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_batch, m)?)?;
    m.add_function(wrap_pyfunction!(extract_spans, m)?)?;
    m.add_function(wrap_pyfunction!(reconstruct_lines, m)?)?;
    m.add_class::<PyPaperDocument>()?;
    m.add_class::<PySection>()?;
    m.add_class::<PySectionEntry>()?;
    m.add_class::<PySectionSelector>()?;
    m.add_class::<PyFigureInfo>()?;
    m.add_class::<PyChunk>()?;
    Ok(())
}
```

---

## 3. 依存クレート

### 3.1 コアクレート依存

| クレート | バージョン | 用途 |
|---------|-----------|------|
| `pdf_oxide` | latest | PDF解析エンジン |
| `serde` + `serde_json` | 1.x | シリアライゼーション |
| `regex` | 1.x | パターンマッチング |
| `image` | 0.25+ | 画像処理・保存 |
| `rayon` | 1.x | 並列処理 |
| `thiserror` | 2.x | エラー型定義 |
| `log` + `env_logger` | 0.4 | ロギング |

### 3.2 Pythonバインディング依存

| クレート | バージョン | 用途 |
|---------|-----------|------|
| `pyo3` | 0.23+ | Python FFI |
| `maturin` | 1.x | ビルドツール (dev) |

### 3.3 CLI依存

| クレート | バージョン | 用途 |
|---------|-----------|------|
| `clap` | 4.x | CLI引数パーサー |
| `indicatif` | 0.17+ | プログレスバー |

---

## 4. エラー設計

```rust
#[derive(Debug, thiserror::Error)]
pub enum PdfLayError {
    #[error("PDF file not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Failed to parse PDF: {0}")]
    PdfParseError(String),

    #[error("Page {0} out of range (total: {1})")]
    PageOutOfRange(u32, u32),

    #[error("Image extraction failed on page {page}: {reason}")]
    ImageExtractionError { page: u32, reason: String },

    #[error("Coordinate normalization failed: scale factor could not be determined")]
    CoordinateNormalizationError,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Image processing error: {0}")]
    ImageError(#[from] image::ImageError),
}

/// 警告（処理は継続するが問題がある場合）
#[derive(Debug, Clone)]
pub enum PdfLayWarning {
    /// キャプションに対応する画像が見つからなかった
    UnmatchedCaption { caption: String, page: u32 },
    /// 画像に対応するキャプションが見つからなかった
    UnmatchedImage { image_path: String, page: u32 },
    /// 座標正規化にフォールバック値を使用
    CoordinateFallback { page: u32, scale_used: f64 },
    /// ページの解析をスキップ
    PageSkipped { page: u32, reason: String },
}

/// 解析結果 + 警告
pub struct AnalysisResult {
    pub document: PaperDocument,
    pub warnings: Vec<PdfLayWarning>,
}
```

---

## 5. ビルド・配布

### 5.1 Rustクレート

```toml
# Cargo.toml (workspace)
[workspace]
members = [
    "crates/pdf-lay-core",
    "crates/pdf-lay",
    "crates/pdf-lay-cli",
    "crates/pdflay-python",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
repository = "https://github.com/xxx/pdf-lay"
```

### 5.2 Python パッケージ

```toml
# crates/pdflay-python/pyproject.toml
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[project]
name = "pdflay"
requires-python = ">=3.9"
description = "PDF Layout Analysis for Academic Papers"
license = { text = "MIT OR Apache-2.0" }
classifiers = [
    "Programming Language :: Rust",
    "Programming Language :: Python :: Implementation :: CPython",
    "Topic :: Text Processing :: General",
    "Topic :: Scientific/Engineering",
]

[tool.maturin]
features = ["pyo3/extension-module"]
module-name = "pdflay"
```

**ビルドコマンド:**
```bash
# 開発ビルド
cd crates/pdflay-python
maturin develop

# リリースビルド（wheel生成）
maturin build --release

# PyPI公開
maturin publish
```

### 5.3 クロスコンパイル

```bash
# Linux (manylinux)
maturin build --release --target x86_64-unknown-linux-gnu

# macOS (Apple Silicon)
maturin build --release --target aarch64-apple-darwin

# Windows
maturin build --release --target x86_64-pc-windows-msvc
```

### 5.4 CI/CD (GitHub Actions)

```yaml
# .github/workflows/release.yml の概要
jobs:
  build-wheels:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
        python: ["3.9", "3.10", "3.11", "3.12", "3.13"]
    steps:
      - uses: actions/checkout@v4
      - uses: PyO3/maturin-action@v1
        with:
          command: build
          args: --release -o dist
      - uses: actions/upload-artifact@v4

  publish:
    needs: build-wheels
    steps:
      - uses: pypa/gh-action-pypi-publish@release/v1
```

---

## 6. 開発フェーズ計画

### Phase 1: コア機能（5〜7週間）

| Week | タスク |
|------|--------|
| 1 | プロジェクト構成、型定義、pdf_oxide連携のテキスト抽出 |
| 2 | 行再構成、カラムレイアウト検出 |
| 3 | ブロックグルーピング、ブロック分類、読み順ソート |
| 4 | セクションヘッダー検出、セクション階層構築 |
| 5 | 画像抽出、座標正規化、キャプション-画像マッチング |
| 6 | **セクション一覧(toc)、セクション選択(select_sections)、LLMテキスト出力** |
| 7 | Markdown生成、JSON出力、PyO3バインディング基本 |

**Phase 1 成果物:**
- `pdflay.analyze()` で論文PDFからMarkdown + JSON出力が可能
- `doc.toc()` でセクション一覧取得、`doc.select_sections()` で選択出力が可能
- IEEE形式の2段組論文で動作確認済み

### Phase 2: テーブル・数式・品質向上（4〜5週間）

| Week | タスク |
|------|--------|
| 8 | テーブル領域検出（罫線ベース + テキスト座標ベース） |
| 9 | **テーブルのインラインテキスト変換（Markdown/CSV）、マルチヘッダー対応** |
| 10 | **数式検出（フォントベース）、上付き・下付き検出、LaTeX/Unicode変換** |
| 11 | メタデータ抽出、ヘッダー/フッター除去、LLMチャンク分割 |
| 12 | テストスイート整備、複数ジャーナル形式対応、バグ修正 |

### Phase 3: 拡張・最適化（2〜3週間）

| Week | タスク |
|------|--------|
| 13 | 数式構造推定（分数、ルート等 Level 3）、参考文献解析 |
| 14 | パフォーマンス最適化、並列処理、ベンチマーク |
| 15 | CLI実装、ドキュメント整備、PyPI公開準備 |

---

## 7. テスト戦略

### 7.1 テストレベル

```
crates/pdf-lay-core/src/
├── layout/
│   ├── line_reconstructor.rs
│   └── line_reconstructor_test.rs    ← ユニットテスト（同一モジュール内）
│
tests/
├── fixtures/
│   ├── ieee_two_column.pdf          ← テスト用PDF
│   ├── elsevier_single_column.pdf
│   └── expected/
│       ├── ieee_two_column.json     ← 期待出力
│       └── ieee_two_column.md
├── integration/
│   ├── test_ieee_format.rs          ← 統合テスト
│   └── test_elsevier_format.rs
└── python/
    └── test_pdflay.py               ← Pythonバインディングテスト
```

### 7.2 ユニットテストの方針

各モジュールの関数に対して、入力と期待出力を明示的に定義したテストを記述する。特にレイアウト解析のエッジケースを重点的にテストする。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_two_column_layout() {
        let lines = vec![
            make_line("left text", 54.0, 720.0, 280.0),   // 左カラム
            make_line("right text", 306.0, 720.0, 530.0),  // 右カラム
            make_line("left text 2", 54.0, 708.0, 280.0),
            make_line("right text 2", 306.0, 708.0, 530.0),
            // ... 十分な行数
        ];

        let detector = ColumnDetector::new();
        let layout = detector.detect(&lines, &PageDimensions {
            page_number: 0, width: 612.0, height: 792.0,
        });

        assert_eq!(layout.regions[0].columns.len(), 2);
        assert!(layout.regions[0].columns[0].right < 300.0);
        assert!(layout.regions[0].columns[1].left > 290.0);
    }

    #[test]
    fn test_detect_mixed_layout() {
        // タイトル（全幅）+ 本文（2段組）のケース
        let lines = vec![
            make_line("Paper Title", 100.0, 760.0, 500.0),  // 全幅
            make_line("left col", 54.0, 650.0, 280.0),
            make_line("right col", 306.0, 650.0, 530.0),
            // ...
        ];

        let detector = ColumnDetector::new();
        let layout = detector.detect(&lines, &page_dims_letter());

        assert!(layout.regions.len() >= 2);
        assert_eq!(layout.regions[0].columns.len(), 1);  // 上部: 1段組
        assert_eq!(layout.regions[1].columns.len(), 2);  // 下部: 2段組
    }
}
```

### 7.3 統合テストの方針

実際の論文PDFを入力として、エンドツーエンドの出力を検証する。

```rust
// tests/integration/test_ieee_format.rs

#[test]
fn test_ieee_paper_full_pipeline() {
    let doc = pdflay::analyze_pdf(
        Path::new("tests/fixtures/ieee_two_column.pdf"),
        &Config::default(),
    ).unwrap();

    // セクション構造の検証
    assert!(doc.sections.len() >= 5);
    assert_eq!(doc.sections[0].header_text(), "Abstract");
    assert_eq!(doc.sections[1].header_text(), "INTRODUCTION");

    // 画像の検出・対応付け
    assert!(!doc.all_figures().is_empty());
    for fig in doc.all_figures() {
        assert!(!fig.caption_text.is_empty());
        assert!(fig.image.path.exists());
    }

    // Markdown生成
    let md = doc.to_markdown(&MarkdownConfig::default());
    assert!(md.contains("## Abstract"));
    assert!(md.contains("![Fig."));
}
```

---

## 8. パフォーマンス設計

### 8.1 最適化ポイント

| ポイント | アプローチ |
|---------|-----------|
| PDF解析 | pdf_oxideのストリーミング解析を活用。全ページを一度にメモリに展開しない |
| 画像抽出 | 画像デコードは必要な場合のみ（座標だけ必要な場合はスキップ） |
| 並列処理 | ページ単位の処理をrayonで並列化。バッチ処理は論文単位で並列化 |
| メモリ | 大きなテキストブロックはCow<str>で不要なコピーを避ける |
| 文字列結合 | String::with_capacityで事前アロケーション |

### 8.2 ベンチマーク

```rust
// benches/analyze_benchmark.rs
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_12page_ieee(c: &mut Criterion) {
    c.bench_function("analyze_12page_ieee", |b| {
        b.iter(|| {
            pdf_lay::analyze_pdf(
                Path::new("benches/fixtures/ieee_12page.pdf"),
                &Config { extract_images: false, ..Default::default() },
            )
        })
    });
}
```
