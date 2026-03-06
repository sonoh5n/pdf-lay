# AGENTS.md — pdf-lay

このファイルは、AI コーディングエージェント（Claude Code, Cursor, Copilot, Cline 等）がこのリポジトリで作業する際のガイドラインです。

---

## プロジェクト概要

**pdf-lay** は学術論文 PDF からテキスト・テーブル・図・数式を構造化抽出し、LLM による情報抽出に最適化された中間表現を生成する Rust ライブラリです。PyO3 経由で Python パッケージ (`pdflay`) としても提供します。

## リポジトリ構成

```
pdf-lay/
├── AGENTS.md                ← このファイル
├── Cargo.toml               # ワークスペース定義
├── crates/
│   ├── pdf-lay-core/        # コアライブラリ（内部クレート、公開しない）
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── extract/     # PDF テキスト・画像抽出（pdf_oxide 依存）
│   │       ├── layout/      # 行再構成、カラム検出
│   │       ├── structure/   # ブロック分類、セクション階層構築
│   │       ├── figure/      # キャプション検出、画像マッチング
│   │       ├── table/       # テーブル検出、Markdown/CSV テキスト変換
│   │       ├── math/        # 数式検出、LaTeX/Unicode 変換
│   │       ├── selector/    # セクション一覧(toc)、選択的出力
│   │       ├── output/      # Markdown/JSON/LLMテキスト/チャンク生成
│   │       ├── types/       # 共通型（Rect, TextSpan, TextLine, TextBlock 等）
│   │       └── config.rs
│   │
│   ├── pdf-lay/             # パブリック Rust ライブラリ（pdf-lay-core の再エクスポート）
│   ├── pdf-lay-cli/         # CLI バイナリ
│   └── pdflay-python/       # PyO3 Python バインディング（maturin ビルド）
│
├── tests/
│   ├── fixtures/            # テスト用 PDF ファイル
│   └── integration/         # 統合テスト
├── benches/                 # criterion ベンチマーク
└── docs/
    ├── 01_SPECIFICATION.md  # 仕様書
    └── 02_DESIGN.md         # 設計書
```

## 技術スタック

| 領域 | 技術 |
|------|------|
| 言語 | Rust 2024 edition (1.75+) |
| PDF 解析 | `pdf_oxide` |
| シリアライゼーション | `serde` + `serde_json` |
| 正規表現 | `regex` |
| 画像処理 | `image` crate |
| 並列処理 | `rayon` |
| エラー型 | `thiserror` |
| Python バインディング | `pyo3` (0.23+) |
| Python ビルド | `maturin` (1.x) |
| CLI | `clap` (4.x) |
| テスト | `cargo test`, `pytest` |
| ベンチマーク | `criterion` |

## ビルドとテスト

```bash
# Rust ビルド
cargo build
cargo build --release

# テスト
cargo test                              # 全テスト
cargo test -p pdf-lay-core              # コアのみ
cargo test -p pdf-lay-core -- layout    # layout モジュールのみ

# ベンチマーク
cargo bench

# Python バインディング開発ビルド
cd crates/pdflay-python
maturin develop
python -c "import pdflay; print(pdflay.__version__)"

# Python テスト
pytest tests/python/

# リント・フォーマット
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

## アーキテクチャ原則

### データフローパイプライン

処理は常にこの順序で流れる。各段階は前段の出力のみに依存し、後段を参照しない。

```
PDF → [Extract] → TextSpan[] + ImageInfo[]
    → [Layout]  → TextLine[] → PageLayout[]
    → [Structure] → TextBlock[] → Section[]
    → [Figure/Table/Math] → FigureInfo[] + TableInfo[] + MathRegion[]
    → [Output]  → PaperDocument → Markdown / JSON / LLMText / Chunks
```

### モジュール間の依存方向

```
types ← extract ← layout ← structure ← figure
                                      ← table
                                      ← math
                         ← selector ← output
```

- `types` は全モジュールから参照される共通型
- `extract` は `pdf_oxide` への唯一の依存点。他モジュールは `pdf_oxide` を直接使わない
- `output` は最終段階。他の全モジュールの型を参照してよい

### 重要な設計判断

1. **座標系**: PDF デフォルト座標系（左下原点、Y 軸上向き、ポイント単位）で統一する。`Rect` 型では `top > bottom` が常に成立する
2. **画像座標の正規化**: `pdf_oxide` の画像 bbox はテキスト座標と異なるスケールの場合がある。`extract::CoordinateNormalizer` で統一する
3. **フォント名は不信頼**: PDF ごとにフォント名の命名規則が異なる（`Ty1`, `CMMI10`, `F2` 等）。フォント名の文字列そのものではなく、同一ドキュメント内での相対的なサイズ・太字/イタリック属性の比較で判断する
4. **本文フォントサイズは統計的に決定**: `BlockClassifier::detect_body_font_size()` で文字数ウェイト付きヒストグラムの最頻値を使う

## コーディング規約

### Rust

- **フォーマット/リント**: 変更を提出する前に `cargo fmt --all` と `cargo clippy --all-targets -- -D warnings` を必ず実行し、警告はゼロ（`-D warnings`）にする
- **エラー処理**: 回復不能なエラーは `Result<T, PdfLayError>` で返す。パニックしない。部分的に解析不能なページがあっても残りを処理し続ける
- **警告**: 処理は継続するが問題がある場合は `PdfLayWarning` を `AnalysisResult::warnings` に蓄積する
- **命名**: 型名は CamelCase、関数名は snake_case。モジュール名は snake_case
- **可視性**: `pdf-lay-core` の内部型は `pub(crate)` を使い、外部に公開する型のみ `pub` にする。`pdf-lay` クレートで再エクスポートする
- **テスト**: 各モジュールファイル内に `#[cfg(test)] mod tests` を配置。テストヘルパー（`make_line()`, `make_span()` 等）は `tests/helpers/` にまとめる
- **ドキュメント**: 公開 API には必ず `///` doc comment を書く。パニック条件がある場合は `# Panics` セクションに記載
- **クローン回避**: 大きな構造体（`PaperDocument`, `Section`）は参照で渡す。`SectionSelector` はライフタイム `'a` で元データを借用する
- **正規表現**: `regex` クレートの `Regex` はコンパイルコストがあるため、構造体のフィールドに保持して再利用する。関数内でのコンパイルは避ける

### Python バインディング (PyO3)

- `#[pyclass]` を付ける型は `Clone` を実装する（PyO3 の要件）
- Python 側のゲッターは `#[getter]` 属性を使う
- Python の関数シグネチャは `#[pyo3(signature = (...))]` で明示する
- Rust の `Option<T>` は Python 側で `None` として表現される
- Python の `lambda` を受け取る `select_sections_where` は、PyO3 の制約上 `PySectionSelector` をインデックスベースで再構築するパターンを使う

### Python（一般）

Rust バインディング以外の Python コード（テスト、スクリプト、補助ツール等）は、原則として以下に従うこと。

#### Coding Rules

##### Python Coding and Docstring Guidelines

Python code should generally follow PEP 8. The main coding rules are summarized below.

##### Whitespace

- Use 4 spaces for indentation (tabs are not allowed).
- Each line should be 88 characters or less.
- For long expressions, add 4 spaces to the normal indentation.
- Separate functions and classes with 2 blank lines, and separate methods within a class with 1 blank line.
- In dictionaries, no space between the key and the colon, but one space before the value on the same line.
- Use exactly one space before and after assignment operators.
- In type hints, place one space after the colon.

##### Naming Conventions

- Functions, variables, and attributes: `lowercase_underscore`.
- Protected attributes: `_leading_underscore`.
- Private attributes: `__double_underscore`.
- Classes and exceptions: `CapitalizedWord`.
- Constants: `ALL_CAPS`.
- The first argument of instance methods: `self`.
- The first argument of class methods: `cls`.

##### Expressions and Statements

- Use "negating the inner part" (`if a is not b`) for negation.
- Use `if not somelist` or `if somelist` to check for empty or non-empty values.
- Write `if`, `for`, `while`, and `except` statements on multiple lines.
- If an expression doesn't fit in one line, enclose it in parentheses and indent it for readability.
- Avoid using `\` for line continuation; use parentheses instead.

##### Imports

- Place all import statements at the top of the file.
- Use absolute imports (e.g., `from bar import foo`).
- If relative imports are necessary, use explicit syntax (e.g., `from . import foo`).
- Order imports as follows: standard library → third-party libraries → custom modules.
- Within each group, sort imports alphabetically.

##### Type Hinting: Mapping vs dict

- Prefer `collections.abc.Mapping[K, V]` for parameters that are read-only (lookup/iterate only).
- Prefer `collections.abc.MutableMapping[K, V]` for parameters that are mutated (assignment/deletion/update).
- Use concrete `dict[K, V]` only when:
  - the function relies on dict-specific methods/behavior (e.g., `pop`, `setdefault`, `update`, merge `|`), or
  - the API intentionally requires a concrete dict.
- If an API accepts `Mapping` but internal code requires `dict`, normalize at the boundary (copy) and keep internals dict-based:

```python
from collections.abc import Mapping
from typing import Any

def func(cfg: Mapping[str, Any]) -> None:
    cfg_dict = dict(cfg)  # normalize/copy at the boundary
    ...
```

- Import these types from `collections.abc` (not `typing`) on Python 3.9+:

```python
from collections.abc import Mapping, MutableMapping
```

#### Docstring Guide

This document provides guidelines for writing docstrings in Python using the Google Style and following the Ruff linter recommendations. All docstrings should be written in English.

##### General Rules

- Use triple double-quotes (`"""`) for all docstrings.
- Write docstrings for modules, classes, methods, and functions.
- The first line should be a concise summary of the purpose.
- Separate sections with blank lines.
- Use imperative mood for function descriptions (e.g., "Fetch data from the API.").
- Do not include redundant information that is already present in type hints.
- Ensure consistency with Ruff linter rules (D series, e.g., D100-D107).

##### Module Docstrings

Each module should have a top-level docstring describing its purpose.

```python
"""This module provides utilities for handling API requests.

It includes functions for sending HTTP requests and processing responses.
"""
```

##### Class Docstrings

Class docstrings should describe the class's purpose and usage.

```python
class User:
    """Represents a user in the system.

    Attributes:
        name (str): The name of the user.
        email (str): The email address of the user.
    """

    def __init__(self, name: str, email: str) -> None:
        """Initialize a User instance."""
        self.name = name
        self.email = email
```

##### Function and Method Docstrings

Function and method docstrings should describe parameters, return values, and exceptions.

```python
def fetch_data(url: str, timeout: int = 10) -> dict:
    """Fetch data from the given URL.

    Args:
        url: The API endpoint to fetch data from.
        timeout: The request timeout in seconds. Defaults to 10.

    Returns:
        The response data parsed as a dictionary.

    Raises:
        ValueError: If the URL is invalid.
        TimeoutError: If the request exceeds the timeout.
    """
    raise NotImplementedError
```

##### Special Cases

###### One-liner Docstrings

Use one-line docstrings for very simple functions.

```python
def is_valid_email(email: str) -> bool:
    """Return whether the email format is valid."""
    raise NotImplementedError
```

###### Docstrings for Properties

For properties, describe what the property represents.

```python
class User:
    @property
    def full_name(self) -> str:
        """Return the user's full name."""
        return f"{self.first_name} {self.last_name}"
```

##### Best Practices

- Be concise but informative.
- Use proper punctuation and grammar.
- Keep formatting consistent across the codebase.
- Avoid redundant information already implied by type hints.
- Use examples when necessary for clarity.
- Follow Ruff's linter rules to ensure consistent style and avoid common pitfalls.

### コミットメッセージ

```
<type>(<scope>): <summary>

type: feat, fix, refactor, test, docs, perf, ci
scope: extract, layout, structure, figure, table, math, selector, output, python, cli
```

例:
```
feat(selector): add select_sections_by_pages method
fix(layout): handle mixed 1-col/2-col layout on same page
test(table): add multi-header table parsing tests
perf(extract): avoid unnecessary String allocation in span merging
```

## 各モジュールの作業ガイド

### extract/ — PDF 抽出

- `pdf_oxide` の API は不安定な場合があるため、ラッパー型で隔離する
- `PdfReader` が唯一の `pdf_oxide` 依存。他モジュールでは `TextSpan`, `ImageInfo` のみ使う
- テキスト抽出は全グリフを対象とする。フォントエンコーディングの差異に注意
- 画像の raw_bbox と テキストの bbox のスケール差を `CoordinateNormalizer` で解決する

### layout/ — レイアウト解析

- `LineReconstructor`: Y 座標の近接性でスパンを行に結合。閾値は `font_size × 0.5`
- `ColumnDetector`: X 座標のヒストグラム → ピーク検出 → クラスタリング。ページを Y 方向に 4 ゾーンに分割して混在レイアウトに対応
- テストは座標を手動で構築した合成データで行う（実 PDF は統合テストで）

### structure/ — 構造構築

- `BlockGrouper`: 行間、フォント変化、太字変化でブロック境界を検出
- `BlockClassifier`: 本文フォントサイズとの比率、既知パターン（"Fig.", "Table", "Abstract"等）でブロックタイプを分類
- `HeaderDetector`: 番号付きパターン（ローマ数字、アラビア数字）＋フォント属性＋既知セクション名のスコアリングで検出。閾値は score ≥ 4
- `SectionBuilder`: ヘッダー間でブロックを分割 → レベルに基づいてスタックベースの階層化
- `ReadingOrderSorter`: ページ順 → 全幅要素は Y 順 → カラム内は Y 順 → 左カラム優先

### figure/ — 画像処理

- `CaptionDetector`: 正規表現 `(?i)^(Fig\.?|Figure|TABLE|Tab\.)\s*(\d+)\s*[:.]?\s*(.*)` でキャプションを検出
- `ImageMatcher`: キャプションと画像の垂直距離（×10）+ 水平距離のスコアで最近傍マッチ。最大ギャップは 50pt
- キャプションが図の上にあるジャーナル（Nature 系等）にも対応するため、上下両方を探索可能にする

### table/ — テーブル処理

- テーブルは画像ではなく **Markdown テーブルとしてインラインテキスト化** する
- 検出は罫線ベース（`PathObject` のグリッドパターン）→ テキスト座標ベース（X アラインメント）の 2 段階
- `TableTextConverter`: Markdown テーブル変換時にセル内のパイプ(`|`)をエスケープ、改行を ` / ` に変換
- マルチヘッダーは `flatten_multi_header()` で `Metrics_Accuracy` のようにアンダースコア結合
- `TableRepresentation` の 3 レベル: `Markdown`（最良）→ `Csv`（中間）→ `PlainText`（フォールバック）

### math/ — 数式処理

- **検出**: フォント名パターン（`CM*`, `*Math*`, `*Symbol*`, `MT*`, `STIX*`）と Unicode 数学記号で判定
- **上付き/下付き**: ベースラインからの Y オフセット（`font_size × 0.3` 以上）かつフォントサイズが 85% 未満
- **変換の優先度**: Auto モードでは CM フォント検出時は LaTeX、それ以外は Unicode
- **出力形式**: LaTeX（`$E = mc^{2}$`）、Unicode（`E = mc²`）、PlainText（`E = mc^2`）の 3 形式
- 数式の完全な LaTeX 復元は困難。分数 (`\frac`) やルート (`\sqrt`) の構造推定は Phase 3

### selector/ — セクション選択

- `TocGenerator`: セクション → `SectionEntry`（推定トークン数、図・テーブル数を含む軽量メタデータ）
- `SectionSelector`: 名前（部分一致、大文字小文字無視）/ インデックス / レベル / ページ範囲 / 述語の 5 通りで選択
- 親セクションを選択すると子セクションも自動的に含まれる
- `LlmTextGenerator`: テーブルをインラインテキスト、図をプレースホルダ `[IMAGE: Fig. 1 path]` として出力

### output/ — 出力生成

- `MarkdownGenerator`: セクション構造に沿って再帰的に出力。画像は `![Fig. 1](path)` + イタリックキャプション
- `JsonGenerator`: `serde_json::to_string_pretty` で PaperDocument をシリアライズ
- `Chunker`: セクション境界優先で分割。セクションが `max_tokens` を超える場合は段落単位で分割
- トークン数概算: 英語 ~4 文字/トークン、日本語 ~1.5 文字/トークン

## テスト方針

### ユニットテスト

各モジュール内に `#[cfg(test)] mod tests` を配置。座標を手動で構築した合成データでテストする。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_span(text: &str, left: f64, top: f64, font_size: f64) -> TextSpan {
        TextSpan {
            text: text.to_string(),
            font_name: "Regular".to_string(),
            font_size,
            is_bold: false,
            is_italic: false,
            bbox: Rect { left, top, right: left + text.len() as f64 * font_size * 0.5, bottom: top - font_size },
            page: 0,
        }
    }

    fn make_bold_span(text: &str, left: f64, top: f64, font_size: f64) -> TextSpan {
        let mut span = make_span(text, left, top, font_size);
        span.is_bold = true;
        span.font_name = "Bold".to_string();
        span
    }
}
```

### 統合テスト

`tests/fixtures/` に実際の論文 PDF を配置し、エンドツーエンドの出力を検証。

```
tests/
├── fixtures/
│   ├── ieee_two_column.pdf
│   ├── elsevier_single_column.pdf
│   ├── nature_caption_above.pdf
│   ├── arxiv_latex_heavy.pdf
│   └── expected/
│       ├── ieee_two_column.json
│       └── ieee_two_column.md
└── integration/
    ├── test_ieee_format.rs
    ├── test_section_selection.rs
    ├── test_table_extraction.rs
    └── test_math_detection.rs
```

### テスト対象ジャーナル形式

| ジャーナル | レイアウト | 特徴 |
|-----------|-----------|------|
| IEEE | 2段組 | ローマ数字セクション番号、全大文字見出し |
| Elsevier | 1段組 | アラビア数字セクション番号 |
| Nature/Springer | 1段組 | 太字セクション名、キャプション上配置 |
| ACS | 2段組 | 狭い段幅 |
| arXiv preprint | 混在 | LaTeX 由来、多様なスタイル、数式が多い |

## パフォーマンス目標

| 項目 | 目標値 |
|------|--------|
| 12 ページ論文の全解析 | < 3 秒 |
| 画像抽出込み | < 5 秒 |
| メモリ使用量（12 ページ） | < 200 MB |
| 10 論文バッチ（並列） | < 30 秒 |

## よくある作業パターン

### 新しいジャーナル形式への対応

1. テスト PDF を `tests/fixtures/` に追加
2. `pdf-lay-cli` の `debug-layout` コマンドでレイアウトを可視化
3. `structure/header_detector.rs` にヘッダーパターンを追加（必要なら）
4. `structure/block_classifier.rs` のスコアリング閾値を調整（必要なら）
5. 統合テストを追加して回帰を防止

### テーブル抽出の精度改善

1. 問題のある PDF のテーブル領域を `debug-layout` で確認
2. 罫線が検出されているか `extract/` のパス出力を確認
3. 罫線なし → `table/detector.rs` のテキストアラインメント検出を調整
4. セル割り当ての誤り → `table/grid_builder.rs` のクラスタリング閾値を調整
5. マルチヘッダー → `table/text_converter.rs` の `flatten_multi_header` を修正

### 数式変換の改善

1. `math/detector.rs` の `math_font_patterns` にフォント名パターンを追加
2. `math/symbol_map.rs` にシンボルマッピングを追加
3. 上付き/下付きの Y オフセット閾値は `MathConfig::superscript_y_threshold` で調整

### Python API の拡張

1. `pdf-lay-core` に Rust 実装を追加
2. `pdflay-python/src/lib.rs` に `#[pymethods]` or `#[pyfunction]` を追加
3. `#[pyclass]` 型は必ず `Clone` を実装
4. `maturin develop` でビルドし `pytest` でテスト

## ドキュメント

| ファイル | 内容 |
|---------|------|
| `docs/01_SPECIFICATION.md` | 機能要件、API 仕様、入出力仕様、テスト要件 |
| `docs/02_DESIGN.md` | アーキテクチャ、モジュール詳細設計、PyO3 バインディング設計、開発フェーズ計画 |
| `AGENTS.md` | このファイル。AI エージェント向けガイドライン |
| `README.md` | ユーザー向けクイックスタート（未作成） |

仕様・設計の詳細は上記ドキュメントを必ず参照してから実装すること。
