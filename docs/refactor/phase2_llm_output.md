# Phase 2 — ローカルLLM出力の一本化（本命）

**対象読者:** 本フェーズを実装するコーディングエージェント（Codex 等）およびレビュアー
**前提:** 本書は `docs/refactor/00_REVIEW_POLICY.md`（以下「レビュー方針」）と
`docs/refactor/REFACTORING_PROPOSAL.md`（以下「提案書」）の下位文書である。
食い違う場合はレビュー方針を優先する。特に §1 の6原則（No Silent Drop / 単一の高忠実度レンダリングコア /
分類は一度だけ / 後方互換はフラグと `#[serde(default)]` / パニックしない / マジックナンバーは設定へ）は本フェーズ全タスクの憲法である。
**依存:** Phase 0（数式デフォルト有効化 P0-3・画像相対パス P0-4・drop厳格化 P0-5）に依存する。Phase 1（セクション判定）から利益を受けるが必須依存ではない。

---

## 前文

### このフェーズが解くこと

本ツールの存在意義は「長尺の学術論文PDF → ローカルLLMが解釈できる中間表現」である。ところが現状、
**4つの出力経路（markdown / llm_text / chunk / json）がそれぞれ別々の忠実度を持ち**、RAGの主経路である chunk は
最も忠実度が低い。具体的には次の構造的欠陥がある（提案書 §1.4 L1〜L8, I2〜I4 に対応）。

1. **chunk 本文が生テキスト。** `Chunker` は `Section::full_text()`
   （`crates/pdf-lay-core/src/types/text.rs:178-193`）を使う。これは `block.text` を素朴に連結するだけで、
   数式変換もテーブルmarkdownも図placeholderも通らない。結果、全チャンクに未変換の数式グリフ（PUA文字化け）が入り、
   表・図はチャンク本文から消える。markdown / llm_text は独自の数式変換を持つのに chunk だけ持たない。
2. **`include_section_context` がデッド設定。** `ChunkConfig`
   （`crates/pdf-lay-core/src/config.rs:263-264`）に存在するが `chunker.rs` から一度も読まれない。
   チャンクにパンくず（親セクション経路）も見出し行も付かず、RAGで位置情報が失われる。
3. **トークン推定が固定ヒューリスティック。** `Chunker::estimate_tokens`
   （`chunker.rs:310-314`）は ASCII/4 + 非ASCII/1.5 の固定式。`chunk_by_tokens` は
   `max_chars = max_tokens * 4`（`chunker.rs:244`）と ASCII 比を直書きするため、**CJKチャンクが予算を大幅超過**する。
4. **`TokenCount` / `Paragraph` 戦略が section 属性・figure・table を全脱落**させる
   （`chunker.rs:225-305`）。`SectionBoundary` でも巨大な単一段落はサブ分割されず1つの予算超過チャンクになる。
5. **画像リンクが LLM 経路で生パス漏れ。** `LlmTextConfig` に `image_base` が無く
   （`config.rs:213-238`）、`llm_text.rs:193-208` の `write_figure` は `fig.image.path` の生ディスクパス（絶対パスも）を
   そのまま埋め込む。さらに図はセクション末尾に一括ドレイン（`llm_text.rs:181-185`）され、`insertion_point` を無視する。
6. **CLIに json / chunks / llm-text サブコマンドが無い**（`crates/pdf-lay-cli/src/main.rs:164-178` は `toc` と `markdown` のみ）。
   RAGの主経路がPython専用で、CLIのみのエージェント（リリースバイナリ）は RAG を実行できない。`PyChunk` にも `tables` getter が無い
   （`crates/pdflay-python/src/lib.rs:695` に figures getter はあるが tables は無い）。
7. **JSON が生 serde ダンプ**（`crates/pdf-lay-core/src/output/json.rs:10-12`）。bbox/フォント幾何を含む重い IR しか出せず、
   content-only の軽量射影が無い。
8. **テーブルが colspan/rowspan 非対応・マルチヘッダを最終行に潰す**
   （`table/grid_builder.rs:78-93`, `table/text_converter.rs:19-27`）。

### このフェーズのゴール

- **4出力を単一の「ブロック→リッチテキスト」変換層（render-core）の射影にする**（レビュー方針 §1-2）。
  markdown / llm_text / chunk が同じ数式変換・図表描画を共有し、忠実度の乖離を消す。
- **RAGギャップを閉じる。** chunk 本文が数式・テーブル・図placeholder・パンくずを含み、CLI だけで RAG が完結する。
- **実 tokenizer を差せる。** `Tokenizer` trait を導入し、既定は改良ヒューリスティック、`--tokenizer <model-or-path>` で
  `tokenizers` クレートの実 BPE を差し込む。文字予算とトークン予算の不整合（CJK超過）を解消する。

### タスク一覧（依存順）

| ID | タイトル | 重大度 | 依存 |
|----|----------|--------|------|
| P2-1 | chunk本文を render-core 経由に（render-core 確立） | HIGH | Phase 0（P0-3/P0-5） |
| P2-2 | `include_section_context` 実装（パンくず＋見出し行） | HIGH | P2-1 |
| P2-3 | `Tokenizer` trait 導入・CJK予算不整合の解消 | HIGH | P2-1 |
| P2-4 | 全戦略で section 属性・figure/table 保持＋巨大段落サブ分割 | HIGH | P2-1, P2-3 |
| P2-5 | `LlmTextConfig` に `image_base` 追加・図を insertion_point 順に | HIGH | Phase 0（P0-4） |
| P2-6 | CLI サブコマンド追加（json / chunks / llm-text）＋ `PyChunk.tables` | HIGH | P2-1, P2-2, P2-3, P2-5 |
| P2-7 | JSON content-only 射影オプション | MED-HIGH | P2-1 |
| P2-8 | テーブル改善（colspan/rowspan・マルチヘッダ保持・罫線なし緩和） | MED（一部さらに調査） | なし（独立可） |

各タスクは **1タスク = 1ブランチ = 1PR**（レビュー方針 §2）。依存が未マージなら着手しない。

---

## render-core 設計

本フェーズの中核は、現在 markdown.rs と llm_text.rs に**二重実装**されているブロック変換ロジックを
1つに統合し、chunker からも呼べるようにすることである。

### 現状の二重実装（統合対象）

- `crates/pdf-lay-core/src/output/markdown.rs:24-93` `convert_block_text_with_math`
  — 数式検出→変換→`MathFormatter::format_for_markdown`、**非数式spanは `escape_for_markdown_text` でエスケープ**。
- `crates/pdf-lay-core/src/selector/llm_text.rs:12-73` `convert_block_text_for_llm`
  — ほぼ同一ロジックだが `MathFormatter::format_for_llm` を使い、**エスケープしない**。

両者の差は「エスケープの有無」と「format_for_markdown / format_for_llm」だけである
（`format_for_llm` は現状 `format_for_markdown` に委譲＝完全同一。`converter.rs:261-268`）。
したがって差分は **EscapeMode** 1パラメータに畳める。

### 新モジュール `crates/pdf-lay-core/src/output/render_core.rs`

`output/mod.rs`（現状 `crates/pdf-lay-core/src/output/mod.rs:1-9`）に `pub mod render_core;` と
必要な `pub use` を追加する。以下を定義する（**pseudo-code。実コードは書かない**）。

```
/// テキストのエスケープ方針。
/// Markdown 出力は HTML タグ / Markdown リンク注入を無害化する。
/// LLM text / chunk は生テキストのまま（LLM入力なのでエスケープ不要）。
pub enum EscapeMode { Markdown, Plain }

/// render-core が全出力へ供給する描画オプション。
/// serde 対象ではない（実行時に各 *Config から組み立てる）。
pub struct RenderOptions<'a> {
    pub math_config: Option<&'a MathConfig>, // None なら数式変換オフ（生グリフ）
    pub escape: EscapeMode,
    pub include_headers: bool,               // セクション見出し行を先頭に出すか
    pub include_figures: bool,
    pub include_tables: bool,
    pub figure_format: FigureTextFormat,     // 既存 config::FigureTextFormat を再利用
    pub image_base: String,                  // 図リンクの基底パス（P0-4/P2-5 の相対規則）
}

/// 1ブロック → リッチテキスト。数式変換とエスケープの唯一の実装。
/// markdown.rs::convert_block_text_with_math と
/// llm_text.rs::convert_block_text_for_llm を置換する。
pub(crate) fn render_block(
    block: &TextBlock,
    detector: Option<&MathDetector>,
    converter: Option<&MathConverter>,
    math_config: Option<&MathConfig>,
    escape: EscapeMode,
) -> String

/// セクション「自身」の本文をリッチテキスト化（子セクションには再帰しない）。
/// 呼び出し側が再帰を制御する（chunker は per-section chunk、
/// markdown/llm_text は write_section 内で子を回す）。
/// 手順:
///   1. include_headers かつ header 有りなら見出し行を出す。
///   2. block を順に render_block（block_type の drop 規則は下記）。
///   3. figure / table を insertion_point 順に interleave（markdown.rs:199-255 と同一ロジック）。
pub(crate) fn render_section_content(section: &Section, opts: &RenderOptions) -> String

/// Markdown 用エスケープ（現 markdown.rs の private 関数を移設して共有）。
pub(crate) fn escape_for_markdown_text(s: &str) -> String
```

### drop 規則（No Silent Drop 準拠）

`render_block` / `render_section_content` は「本文を無言で捨てない」。
どの `block_type` をスキップするかは **Phase 0 P0-5 で厳格化された規則に従う**（P0-5 未マージなら現行踏襲）。
現行踏襲時のスキップ対象は `Caption | PageNumber | RunningHeader | RunningFooter`
（`markdown.rs:205-209`, `llm_text.rs:139-144`, `text.rs:181-189` と一致）。
**本フェーズで新たな drop 経路を作ってはならない。** スキップした要素は既存どおり別枠（`section.figures` 等）へ退避されているか、
上流で warning 済みであること。新たにスキップを増やす場合は `AnalysisResult.warnings` に記録する。

### 各出力の射影（統合後の姿）

- **markdown.rs** … `render_block(escape=Markdown)` を呼ぶ。図表 interleave は既存の insertion_point ロジックを render_core に移すか、当面 markdown 側に残して `render_block` のみ共有（P2-1 の最小案）。
- **llm_text.rs** … `render_block(escape=Plain)` を呼ぶ。図は insertion_point 順（P2-5）。
- **chunker.rs** … `render_section_content(section, opts_plain)` で `chunk.text` を得る（P2-1）。
- **json.rs** … content-only 射影で math 変換済みテキストを埋める（P2-7）。

### スコープの畳み方

render-core の「確立」（module 新設 + `render_block` 統合 + chunker 移行 + markdown/llm_text の
block 変換委譲）を **P2-1 に含める**。図表 interleave の完全共有・content-only 射影・tokenizer は後続タスクへ分割する。

---

## タスク詳細

### P2-1: chunk本文を render-core 経由に（render-core 確立）

**目的:** `chunk.text` に数式変換済みテキスト・インラインテーブルmarkdown・図placeholderを含め、生 `full_text()` を廃する。同時に render-core を確立し markdown/llm_text の block 変換を単一実装へ統合する。

**重大度 / 依存:** HIGH / Phase 0（P0-3 数式デフォルト有効・P0-5 drop厳格化）。Phase 0 未マージでも実装可能だが、数式が実際に変換されるのは P0-3 マージ後。

**対象ファイル:**
- 新規 `crates/pdf-lay-core/src/output/render_core.rs`
- `crates/pdf-lay-core/src/output/mod.rs:1-9`（module 追加）
- `crates/pdf-lay-core/src/output/chunker.rs:28-118, 120-207, 225-305`（`full_text()` 依存の置換）
- `crates/pdf-lay-core/src/output/markdown.rs:24-93, 199-226`（block変換を委譲）
- `crates/pdf-lay-core/src/selector/llm_text.rs:12-73, 126-157`（block変換を委譲）
- `crates/pdf-lay-core/src/types/text.rs:178-193`（`full_text` は残すが chunker から不使用に）

**現状（問題）:**
`chunker.rs` は 6箇所（`chunk_sections` 33, `chunk_section_recursive` 85, `split_section_text` 50/103,
`chunk_by_tokens` 227-232, `chunk_by_paragraph` 280-285）で `section.full_text()` を使う。`full_text`
（`text.rs:178-193`）は:
```
self.blocks.iter().filter(非本文除外).map(|b| b.text.as_str()).collect().join("\n\n")
```
と **生 `block.text` を連結するだけ**。数式は生グリフのまま、テーブルは `section.tables` にしか無く本文に現れず、
図placeholderも入らない。一方 markdown/llm_text は `convert_block_text_*` で数式変換済みなので、
**同じ論文でも chunk だけ数式が壊れる**（提案書 L2）。さらに markdown/llm_text は同一ロジックを二重実装している。

**変更後の期待動作:**
- `chunk.text` = render-core が生成するセクション本文（数式変換済み・テーブルmarkdown・図placeholderを含む）。
- markdown / llm_text は `render_core::render_block` を呼び、`convert_block_text_with_math` /
  `convert_block_text_for_llm` は削除（または render_block への薄いラッパに縮退）。
- 出力の忠実度が3経路で一致する（数式・テーブルの有無が経路で変わらない）。

**実装手順:**
1. `render_core.rs` を新設し、上記「render-core 設計」の `EscapeMode` / `RenderOptions` /
   `render_block` / `render_section_content` / `escape_for_markdown_text` を定義する。
   `render_block` の中身は `markdown.rs:24-93` をベースに、`escape` が `Plain` のときは
   `escape_for_markdown_text` を呼ばず span テキストをそのまま連結する分岐にする。
   `MathFormatter` は `escape` に応じて `format_for_markdown` / `format_for_llm` を選ぶ
   （現状同一だが将来の分岐点として残す）。
2. `render_section_content(section, opts)`:
   - `opts.include_headers && section.header.is_some()` なら見出し行（`opts.escape` に応じ
     `escape_for_markdown_text` を適用）を先頭に出す。chunk 用途では `include_headers=false`（P2-2 で
     パンくずと一緒に別途付与）を既定にする。
   - `section.blocks` を順に走査し、drop 規則（上記）でスキップしない block を `render_block` で変換して
     `"\n\n"` 区切りで連結。
   - `opts.include_figures` / `opts.include_tables` が真なら figure / table を
     `insertion_point.after_block_index` 順に interleave（`markdown.rs:199-255` の VecDeque ロジックを流用）。
     placeholder 文字列は `opts.figure_format`（`FigureTextFormat`）に従う。パスは `opts.image_base`＋
     `fig.image.path.file_name()`（P0-4/P2-5 の相対規則に整合）。
   - **子セクションには再帰しない**（呼び出し側が制御）。
3. `chunker.rs` に `Chunker` 用の描画オプション組み立てヘルパ `fn render_opts(&self) -> RenderOptions`
   を追加。chunk 用途の既定は `escape=Plain, include_headers=false, include_figures=true,
   include_tables=true, figure_format=Placeholder, math_config=<Chunkerが保持するMathConfig>`。
   → `Chunker` に `math_config: Option<MathConfig>` を持たせるため `ChunkConfig` にフィールド追加（下記シグネチャ変更）。
4. `chunker.rs` の全 `section.full_text()` 呼び出しを
   `render_core::render_section_content(section, &self.render_opts())` に置換する:
   - `chunk_section_recursive`（85）… `let section_text = render_section_content(...)`。
   - `split_section_text` へ渡すテキスト（50, 103）… 同上（生 `full_text` ではなく rich text を段落分割）。
   - `chunk_by_tokens`（227-232）/ `chunk_by_paragraph`（280-285）… 各セクションを
     `render_section_content` してから連結（section 属性保持は P2-4 で本格対応。P2-1 では文字列内容のみ rich 化）。
5. `markdown.rs:211-222` の block 変換分岐を `render_core::render_block(block, detector, converter, mc, EscapeMode::Markdown)` に置換。`convert_block_text_with_math` を削除。既存の図表 interleave（199-255）はそのまま残す（完全共有は非スコープ）。
6. `llm_text.rs:130-152` の block 変換を `render_core::render_block(..., EscapeMode::Plain)` に置換。`convert_block_text_for_llm` を削除。
7. `text.rs:178-193` の `full_text` は他呼び出し（`document.rs:174-180` の `estimated_text_size`）が残るため**削除しない**。ただし chunker からは不使用にする。ドキュメントコメントに「LLM出力には render_core を使うこと」と追記。

**シグネチャ変更:**
- `ChunkConfig`（`config.rs:255-265`）に **`#[serde(default)] pub math_config: Option<MathConfig>`** を追加
  （None=生グリフ、後方互換）。`Chunker::new` は変更なし（config から読む）。
  Before: `ChunkConfig { max_tokens, overlap_tokens, split_strategy, include_section_context }`
  After: 上記＋ `math_config`。
- `render_core::render_block` / `render_section_content` を新設（`pub(crate)`）。
- `markdown.rs::convert_block_text_with_math`・`llm_text.rs::convert_block_text_for_llm` を **削除**
  （private 関数のため公開APIには影響しない）。
- `ChunkConfig` へのフィールド追加に伴い、リテラル構築している全箇所を更新:
  `chunker.rs` tests（367-374, 435-440, 451-456）, `pdflay-python/src/lib.rs:140-145, 372-377`。
  `..Default::default()` を使うか `math_config: None` を明示追加。

**後方互換:**
- `ChunkConfig.math_config` は `#[serde(default)]`（既定 None）。既存の serialized config は読める。
- `math_config: None` のとき chunk.text は従来同様「数式変換なし（ただしテーブル/図placeholderは新たに入る）」。
  テーブル/図が chunk.text に入るのは仕様改善であり、既存 golden があれば更新して PR で正当性を説明する。

**受け入れ基準（Given/When/Then）:**
- Given 数式spanを含むセクション（CMMI10 の α）と `ChunkConfig.math_config = Some(LaTeX)`,
  When `Chunker::chunk`, Then 生成 chunk の `text` に `\alpha` が含まれ、生グリフ `α` の PUA 化けが無い。
- Given テーブル1つを持つセクション, When `chunk`, Then chunk.text にそのテーブルのmarkdown（`| --- |` を含む）が入る。
- Given 図1つ（insertion_point 指定）を持つセクション, When `chunk`, Then chunk.text に図placeholder（`[IMAGE:` 等）が insertion 位置に入る。
- Given 同一ドキュメント, When markdown / llm_text / chunk を生成, Then 3経路の本文テキスト（エスケープ差を除く）で
  数式・テーブルの出現が一致する。

**追加テスト:**
- `render_core` unit: `render_block_converts_math_latex`（α→`\alpha`）,
  `render_block_plain_does_not_escape`（`<b>` がそのまま）, `render_block_markdown_escapes_html`（`<b>`→`&lt;b&gt;`）,
  `render_section_interleaves_figure_at_insertion_point`。
- `chunker` unit: `chunk_text_contains_converted_math`, `chunk_text_contains_table_markdown`,
  `chunk_text_contains_figure_placeholder`。
- 既存 `markdown.rs` / `llm_text.rs` の数式テスト（`test_math_inline_*`, `test_math_display_*`）が
  委譲後も緑であること（回帰）。
- golden: 実PDF（横断 X-1 のフィクスチャ導入後）で chunk 本文に数式・表が現れることを確認。

**非スコープ:** tokenizer 改良（P2-3）, section 属性/巨大段落（P2-4）, パンくず（P2-2）, 図表 interleave の markdown 側完全共有。

**検証:**
```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p pdf-lay-core
```

---

### P2-2: `include_section_context` 実装（パンくず＋見出し行）

**目的:** デッド設定 `ChunkConfig.include_section_context` を実装し、各 chunk 先頭に親セクション経路（パンくず）と見出し行を付与して RAG の位置情報を回復する。

**重大度 / 依存:** HIGH / P2-1（render-core と Chunker のオプション組み立てに乗る）。

**対象ファイル:**
- `crates/pdf-lay-core/src/output/chunker.rs:19-25, 67-118, 120-207`
- `crates/pdf-lay-core/src/config.rs:263-264`（コメントを実装に一致させる）

**現状（問題）:**
`ChunkConfig.include_section_context`（`config.rs:263-264`, doc「Whether to prepend the section path as context」）は
`Default` で `true`（`config.rs:272`）だが、`chunker.rs` 内に参照が一切無い。`chunk_by_section` は
`chunk_section_recursive`（78-118）でセクションを個別に chunk 化するが、**親経路を一切保持しない**。
`chunk.section` には自セクションの `header_text()` しか入らず（92, 41）、`METHODS > Data Collection` のような
パンくずは失われる。レビュー方針 §3「コメントと実装の乖離」に該当する死にコード。

**変更後の期待動作:**
- `include_section_context == true` のとき、各 chunk.text の先頭に次を付与:
  ```
  [Context: <A> > <B> > <C>]
  # <C の見出し（clean_text）>

  <本文...>
  ```
  `<A> > <B> > <C>` は祖先セクション見出し（clean_text）を ` > ` で連結したパンくず。`C` は当該セクション。
  ヘッダーの無いセクション（preamble）は経路要素から除外する（空文字は連結しない）。
- `include_section_context == false` のとき従来どおり（本文のみ、パンくず・見出し行なし）。

**実装手順:**
1. `chunk_by_section`（67-76）→ `chunk_section_recursive` に `breadcrumb: &[&str]`（祖先の clean_text 群）を渡す引数を追加。
   ルートは `&[]` で開始。
2. `chunk_section_recursive` 内:
   - `let own = section.header_text();`
   - 子へ再帰する際の breadcrumb は `let mut child_bc = breadcrumb.to_vec(); if !own.is_empty() { child_bc.push(own の借用) }`。
   - 当該セクションの chunk を作る前に、`include_section_context` が真なら
     `let prefix = build_context_prefix(breadcrumb, &own);` を計算し、`render_section_content` の結果の**先頭に prepend**する。
   - パンくずに使う「当該セクションまでの経路」= `breadcrumb`（祖先）＋ `own`。
3. `fn build_context_prefix(ancestors: &[&str], own: &str) -> String`:
   - 経路 = ancestors のうち空でないもの ＋（own が空でなければ own）を ` > ` で連結。
   - 出力:
     ```
     format!("[Context: {}]\n# {}\n\n", path, own)   // own が空なら "# ..." 行は省く
     ```
   - 経路が空（全て headerless）のときは prefix を出さない。
4. `split_section_text`（120-207）で大セクションを分割する場合も、**各サブ chunk の先頭に同じ prefix を付ける**
   （継続 chunk でも位置情報が欲しいため）。`is_first` に関わらず prefix を付与し、`has_continuation` の有無とは独立。
   prefix はトークン見積り（`estimated_tokens`）にも含める。
5. `chunk_by_tokens` / `chunk_by_paragraph` は section 属性が P2-4 で入るまでは prefix 対象外（section 不明のため）。
   P2-4 マージ後に per-section 属性が付いたら同じ prefix ロジックを適用する（本タスクでは SectionBoundary のみ対象）。
6. `config.rs:263-264` の doc コメントを実装挙動（パンくず＋見出し行を付与）に一致させる。

**シグネチャ変更:**
- private `chunk_section_recursive` に `breadcrumb: &[&str]` 引数追加（内部のみ、公開APIなし）。
- 新 private `build_context_prefix`。
- 公開APIの変更なし。

**後方互換:**
- 既定は `include_section_context: true`（現状の Default 済み）。挙動が変わる（prefix が付く）ため、
  golden があれば更新。旧挙動が欲しい呼び出し側は `false` を指定できる。Python の `to_chunks`
  （`lib.rs:144, 376`）は現状 `true` 固定なので、後述 P2-6 で引数化するまでは true 固定のまま（挙動変化を PR に明記）。

**受け入れ基準（Given/When/Then）:**
- Given 親 `METHODS`＋子 `Data Collection` の階層, `include_section_context=true`,
  When `chunk`, Then 子由来 chunk.text が `[Context: METHODS > Data Collection]` で始まり、続いて `# Data Collection` 行がある。
- Given `include_section_context=false`, When `chunk`, Then chunk.text にパンくず行が無い。
- Given headerless preamble セクション, When `chunk`, Then パンくずに空要素が入らず（`[Context:  > ...]` のような二重区切りが無い）、prefix 自体が出ないか own のみ。
- Given prefix 付与, Then `estimated_tokens` が prefix 分を含む（prefix 文字列のトークンが加算されている）。

**追加テスト:**
- `chunker` unit: `breadcrumb_prefix_for_nested_section`,
  `no_prefix_when_context_disabled`, `headerless_section_no_empty_breadcrumb`,
  `split_chunks_all_carry_prefix`（大セクション分割で全サブ chunk に prefix）。

**非スコープ:** TokenCount/Paragraph への適用（P2-4 後）, Python 引数化（P2-6）。

**検証:** `cargo test -p pdf-lay-core`（上記標準セット）。

---

### P2-3: `Tokenizer` trait 導入・CJK予算不整合の解消

**目的:** トークン数の算出を trait 化し、既定を改良ヒューリスティック、`--tokenizer <model-or-path>` で実 BPE を差せるようにする。`chunk_by_tokens` の文字予算（`max_tokens*4`）がCJKで破綻する不整合を解消する。

**重大度 / 依存:** HIGH / P2-1。

**対象ファイル:**
- 新規 `crates/pdf-lay-core/src/output/tokenizer.rs`（trait と既定実装）
- `crates/pdf-lay-core/src/output/mod.rs`（`pub use`）
- `crates/pdf-lay-core/src/output/chunker.rs:19-25, 225-275, 307-314`
- `crates/pdf-lay-core/Cargo.toml`, `Cargo.toml`（workspace deps、任意 feature）
- `crates/pdf-lay-cli/src/main.rs`（`--tokenizer` フラグ、P2-6 と協調）

**現状（問題）:**
`Chunker::estimate_tokens`（`chunker.rs:310-314`）:
```
ascii_chars / 4 + (non_ascii_chars as f64 / 1.5) as usize
```
は静的関数で差し替え不能。非ASCIIを 1.5 char/token として**過小評価**する（実 BPE では日本語は概ね 1 char ≈ 1〜2 token）。
さらに `chunk_by_tokens`（244）:
```
let max_chars = effective_max_tokens * 4;   // ASCII 前提の直書き
```
は「1token=4char」を固定するため、CJK本文では 4000token 予算が 16000 CJK 文字＝実 8000〜10000+ token となり
**チャンクが予算を大幅超過**する（提案書 L5, L8）。`overlap_chars = overlap_tokens * 4`（245）も同断。

**変更後の期待動作:**
- `Tokenizer` trait を通じてトークン数を数える。既定 `HeuristicTokenizer` は CJK を過小評価しない改良式。
- `chunk_by_tokens` は「文字数×4」ではなく **tokenizer の実カウントで予算判定**し、CJKでも `max_tokens` を超えない。
- `--tokenizer <model-or-path>`（例 `--tokenizer Qwen/Qwen2.5-7B` や `--tokenizer ./tokenizer.json`）指定時は
  `tokenizers` クレートで実 BPE を用いる（feature 有効時のみ）。未指定は既定ヒューリスティック。

**実装手順:**
1. `tokenizer.rs` に trait を定義:
   ```
   pub trait Tokenizer: Send + Sync {
       /// テキストのトークン数を返す。
       fn count(&self, text: &str) -> usize;
   }
   ```
2. 既定実装 `HeuristicTokenizer`（改良式）:
   - 文字を3分類: ASCII（`c.is_ascii()`）/ CJK（統合漢字・かな・ハングル等の範囲）/ その他非ASCII。
   - `count = ascii/4 + cjk/1 + other_non_ascii/2`（端数切り上げでなく従来同様 floor で可、ただし
     **CJKは 1 char = 1 token 換算**として過小評価を止める）。定数（4 / 1 / 2）は
     `HeuristicTokenizer` の doc コメントに根拠を明記する（マジックナンバーはコード直書きせず、
     `HeuristicTokenizer` のフィールド or 名前付き `const` として定義しレビュー方針 §1-6 を満たす）。
   - CJK 範囲判定は補助関数 `fn is_cjk(c: char) -> bool`（U+3040–30FF かな, U+3400–9FFF 漢字, U+AC00–D7AF ハングル,
     U+FF00–FFEF 全角 等）。範囲はコメントで列挙。
3. `Chunker` に tokenizer を持たせる:
   - `pub struct Chunker { pub config: ChunkConfig, tokenizer: Box<dyn Tokenizer> }`。
   - `Chunker::new(config)` は `HeuristicTokenizer` を既定でセット（後方互換）。
   - `pub fn with_tokenizer(config: ChunkConfig, tokenizer: Box<dyn Tokenizer>) -> Self` を追加。
   - `estimate_tokens`（静的）は `self.tokenizer.count(s)` を使う **インスタンスメソッド** `fn count_tokens(&self, s: &str) -> usize` に置換。
     既存の静的 `estimate_tokens` は `#[deprecated]` にしてもよいが、テスト（420）が使うため
     `HeuristicTokenizer::default().count()` へ委譲する薄いラッパとして残すのが安全。
   - **注記:** `tokenizer` は trait object のため serde 不可。よって `ChunkConfig` には入れず `Chunker` フィールドとする。
     `ChunkConfig` には識別子だけ持たせる案（`#[serde(default)] pub tokenizer_spec: Option<String>`）も可だが、
     実ロードは Chunker 構築側（CLI/Python）で行う。本タスクでは `Chunker::with_tokenizer` を seam とする。
4. `chunk_by_tokens`（225-275）を token 予算ベースに書き換え:
   - `max_chars = max_tokens*4` を撤廃。
   - アルゴリズム: `chars` を走査し、`start` から文字を足しながら
     `self.count_tokens(&candidate)` が `effective_max_tokens` を超える直前で切る。
     計算量を抑えるため、まず粗く 1トークン≈4文字で仮 end を置き、そこから ±方向に
     数文字単位で `count_tokens` を再評価して境界を調整する（線形でも可、**必ず前進**を保証）。
   - overlap も token 基準: 直前 chunk 末尾から `overlap_tokens` 相当の文字を `count_tokens` で逆算して戻す。
   - 無限ループ防止（`max_tokens.max(1)`, `advance >= 1`）は現行同様に維持する。
5. CLI（P2-6 の `chunks` サブコマンド）に `--tokenizer <SPEC>` を追加。SPEC が空なら Heuristic、
   非空なら feature `real-tokenizer` 有効時に `tokenizers::Tokenizer::from_pretrained` / `from_file` でロードし
   `HfTokenizer`（trait 実装ラッパ）を `Chunker::with_tokenizer` へ渡す。feature 無効時に SPEC 指定されたら
   `error:` を stderr に出して exit 1（推測実装せず明示エラー）。
6. `tokenizers` 依存:
   - workspace `Cargo.toml` に `tokenizers = { version = "0.20", optional = true }`（バージョンは要確認・PR に記載）。
   - `pdf-lay-core/Cargo.toml` に `tokenizers = { workspace = true, optional = true }` と
     `[features] real-tokenizer = ["dep:tokenizers"]`。既定 feature には**含めない**（ビルド重量回避）。
   - `HfTokenizer` は `#[cfg(feature = "real-tokenizer")]` でガード。

**シグネチャ変更:**
- Before: `pub struct Chunker { pub config: ChunkConfig }`, `pub fn estimate_tokens(text:&str)->usize`（static）。
- After: `Chunker` に `tokenizer: Box<dyn Tokenizer>` 追加。`Chunker::with_tokenizer` 追加。
  内部トークン算出は `count_tokens(&self, &str)`。static `estimate_tokens` は Heuristic 委譲で温存。
- 新 trait `output::tokenizer::Tokenizer`, 実装 `HeuristicTokenizer`, cfg-gated `HfTokenizer`。
- `pdf-lay/src/lib.rs` と `pdf-lay-core/src/lib.rs` に `Tokenizer` / `HeuristicTokenizer` を re-export（CLI が使う）。

**後方互換:**
- `Chunker::new` の挙動は「既定 Heuristic（改良式）」。トークン数値は変わる（CJKで増える）。
  chunk 分割位置が変わりうるため golden 更新＋ PR で正当性説明。
- `real-tokenizer` は opt-in feature。既定ビルドは `tokenizers` を引かない。

**受け入れ基準（Given/When/Then）:**
- Given 日本語のみ 1000文字のセクション, `max_tokens=300`, `TokenCount`,
  When `chunk`, Then 各 chunk の `count_tokens(text) <= 300`（**予算超過しない**）。旧実装では超過していたことを回帰テストで対比。
- Given ASCII 100文字, When `HeuristicTokenizer.count`, Then `25`（旧 ASCII 挙動を維持）。
- Given CJK 100文字, When `HeuristicTokenizer.count`, Then `100`（1char=1token、旧値 66 より大）。
- Given `--tokenizer` 指定かつ feature 無効, When 実行, Then `error:` を出して非ゼロ終了（黙って heuristic に落ちない）。

**追加テスト:**
- `tokenizer` unit: `heuristic_ascii_matches_legacy`（100→25）, `heuristic_cjk_not_underestimated`（100→100）,
  `heuristic_mixed`。
- `chunker` unit（**CJK chunk-size test**）: `token_count_strategy_respects_budget_for_cjk`
  （1000 CJK 文字 / max_tokens=300 → 全 chunk が予算内、かつ chunk 数 >= 4）。
- `chunk_by_tokens_overlap_is_token_based`。
- feature ゲート下の `HfTokenizer` は CI で feature 有効時のみ実行する軽い smoke（ローカル `tokenizer.json` フィクスチャ）。

**非スコープ:** section 属性の付与（P2-4）, 具体 tokenizer モデルの同梱。

**検証:**
```
cargo test -p pdf-lay-core
cargo build -p pdf-lay-core --features real-tokenizer   # feature がコンパイルできること
```

---

### P2-4: 全戦略で section 属性・figure/table 保持＋巨大段落サブ分割

**目的:** `TokenCount` / `Paragraph` 戦略でも section 名・figure・table を保持し、`max_tokens` を超える単一段落をサブ分割する。

**重大度 / 依存:** HIGH / P2-1（render-core）, P2-3（tokenizer）。

**対象ファイル:** `crates/pdf-lay-core/src/output/chunker.rs:120-207, 225-305`

**現状（問題）:**
- `chunk_by_tokens`（225-275）は全セクションを `full_text()` で1本の文字列に連結（227-232）し、
  機械的に窓分割する。生成 chunk は `section: String::new()`（257）, `figures: vec![]`（259）,
  `tables: vec![]`（260）で、**section 属性も図表も完全脱落**。
- `chunk_by_paragraph`（279-305）は `empty_section`（287-295, figures/tables 空）を使って
  `split_section_text` を呼ぶため、同様に図表・section 名が失われる。
- `split_section_text`（120-207）は段落を `"\n\n"` で分割（129-132）して詰めるだけ。**1段落が `max_tokens` を超えても
  そのまま1 chunk になる**（141 の条件は `!current_text.is_empty()` を要求するため、巨大段落単独では分割されない。提案書 L8）。
- `SectionBoundary` の分割時、figures/tables は `is_first` の先頭サブ chunk にだけ寄せられる
  （149-158, 185-194）。挿入位置を無視して全図を先頭に集約する。

**変更後の期待動作:**
- `TokenCount` / `Paragraph` でも各 chunk が「その内容が属するセクション名」を持ち、対応する figure/table を保持する。
- 単一段落が `max_tokens` を超える場合、文（sentence）→ 文字窓の順でサブ分割し、各サブ chunk が予算内になる。
- 図表は「その chunk の本文に含まれる insertion_point」に基づき、先頭寄せではなく**該当 chunk に割り当てる**。

**実装手順:**
1. **共通ヘルパの導入。** セクション木を深さ優先で平坦化し、各セクションの
   `(section_name, breadcrumb, rich_text, figures, tables, page_range)` を得る
   `fn flatten_render_units(&self, doc) -> Vec<RenderUnit>` を追加。`rich_text` は P2-1 の
   `render_section_content`（`include_figures/tables=false`＝本文のみ）で得る。図表は別フィールドで保持。
2. **`chunk_by_tokens` 改修（225-275）:**
   - `RenderUnit` を順に消費し、`self.count_tokens` で予算まで詰める。
   - chunk 境界がセクションをまたぐ場合、`chunk.section` は「その chunk の**先頭**内容が属するセクション名」を入れる
     （複数セクションが1 chunk に入るときは先頭セクション名＋`(+N sections)` の注記でも可。実装は先頭名で確定）。
   - figure/table は、その chunk に含めた本文が属するセクション群のものを集約して割り当てる
     （挿入位置が本文範囲に入るもの）。No Silent Drop: 予算の都合で本文に入れられなかった図表は
     次 chunk へ繰り越す（捨てない）。
   - page_range は含めた本文の実ページ範囲から算出（`(0, pages-1)` の丸め込みを廃止）。
3. **`chunk_by_paragraph` 改修（279-305）:** `empty_section` を廃止。`RenderUnit` 単位で
   `split_section_text` を呼び、section 名・figures・tables・breadcrumb を正しく渡す。
4. **巨大段落サブ分割（`split_section_text` 138-175）:**
   - `for para in &paragraphs` の中で、`para_tokens > max_tokens` の場合は
     `let subs = self.split_oversized_paragraph(para);` で段落自体を分割し、各 sub を通常フローに流す。
   - `fn split_oversized_paragraph(&self, para: &str) -> Vec<String>`:
     1. 文境界（`. ` `。` `! ` `? ` 等）で分割し貪欲に詰める。
     2. それでも1文が予算超なら **P2-3 の tokenizer による文字窓分割**（`chunk_by_tokens` と同じ境界探索）で
        予算内に割る。No Silent Drop: どの文字も落とさない。
5. **図表の chunk 割り当て（149-158, 185-194 の置換）:** `is_first` 先頭寄せをやめ、
   各 sub chunk が担当する block 範囲（`global_index` の範囲）を追跡し、
   `insertion_point.after_block_index` がその範囲に入る figure/table をその chunk に割り当てる。
   範囲判定が難しい場合の安全側フォールバックは「最後の chunk にまとめる」ではなく
   「先頭 chunk に残す（現行踏襲）＋ warning」ではなく、**No Silent Drop を満たすため必ずいずれかの chunk に入れる**。
   （範囲追跡が困難な場合は「そのセクションの全図表を、そのセクション由来の最初の chunk に集約」を暫定とし、
   PR の「レビュアーへの質問」に精緻化可否を記す。）

**シグネチャ変更:**
- private `flatten_render_units`, `split_oversized_paragraph`, 内部 struct `RenderUnit` を追加（公開APIなし）。
- 公開APIの変更なし（`Chunk` フィールドは既存のまま）。

**後方互換:**
- `Chunk` の構造は不変。`TokenCount`/`Paragraph` の出力内容（section 名・図表）が増えるのは仕様改善。
  golden 更新＋ PR 説明。

**受け入れ基準（Given/When/Then）:**
- Given 図・表を持つ複数セクション, `strategy=token`, When `chunk`, Then 生成 chunk のうち
  該当内容を含むものは `section` が非空で、対応する figure/table が `figures`/`tables` に入る（全図表がいずれかの chunk に存在＝脱落ゼロ）。
- Given 1つの巨大段落（`max_tokens` の3倍）, `strategy=section`, When `chunk`, Then 2つ以上の chunk に分割され、各 chunk が予算内。
- Given `strategy=paragraph`, When `chunk`, Then 各 chunk の `section` が空文字でない（属する見出し名を持つ）。
- No Silent Drop: 入力の全 figure/table 数 == 出力 chunk 群の figure/table 総数。

**追加テスト:**
- `chunker` unit: `token_strategy_preserves_section_and_figures`,
  `paragraph_strategy_has_section_name`, `oversized_single_paragraph_is_subsplit`,
  `all_figures_present_across_chunks`（脱落ゼロ）, `no_figure_lost_when_budget_forces_carryover`。

**非スコープ:** 図の本文参照（"as shown in Fig.3"）解析（Phase 4 域）。

**検証:** `cargo test -p pdf-lay-core`。

---

### P2-5: `LlmTextConfig` に `image_base` 追加・図を insertion_point 順に

**目的:** LLM text 経路の生パス漏れを解消し、markdown と同じ相対パス規則を適用する。図をセクション末尾一括ではなく `insertion_point` 順に挿入する。

**重大度 / 依存:** HIGH / Phase 0（P0-4 の相対パス規則）。P2-1 とは独立に着手可能だが、render-core があると図表 interleave を共有できる。

**対象ファイル:**
- `crates/pdf-lay-core/src/config.rs:213-238`（`LlmTextConfig` に `image_base`）
- `crates/pdf-lay-core/src/selector/llm_text.rs:108-209`（interleave 化・パス規則）
- `crates/pdflay-python/src/lib.rs:339-358`（`to_llm_text` に `image_base` 引数）

**現状（問題）:**
- `LlmTextConfig`（`config.rs:213-238`）に画像基底パスのフィールドが無い（提案書 I2）。
- `write_figure`（`llm_text.rs:193-209`）は:
  ```
  let path_str = fig.image.path.display().to_string();   // 生ディスクパス（絶対も）
  ...
  out.push_str(&format!("[IMAGE: {} {}]\n\n", fig.figure_id, path_str));
  ```
  と**生パスをそのまま**埋め込む。markdown（`markdown.rs:263-278`）は `file_name()`＋`image_base` を使うのに不一致。
- 図は `write_section`（181-185）で **`section.figures` をセクション末尾に一括ドレイン**し、
  `insertion_point.after_block_index`（markdown.rs:229-231 が使う）を無視する。表（159-178）も同様に末尾一括。

**変更後の期待動作:**
- `LlmTextConfig.image_base` を追加。`write_figure` は markdown と同じく
  `file_name()`＋`image_base`（Phase 0 P0-4 の相対パス計算関数があればそれを共有）でパスを組み立てる。
- 図・表を `insertion_point.after_block_index` に基づき本文ブロック間に interleave（markdown.rs:199-255 と同一挙動）。
  insertion 未指定のものは末尾へ（現行フォールバック維持）。

**実装手順:**
1. `config.rs:215-226` の `LlmTextConfig` に `#[serde(default)] pub image_base: String` を追加。
   `Default`（228-238）で `image_base: String::new()`（空＝ファイル名のみ、絶対パス漏れなし）。
2. `llm_text.rs::write_section`（108-191）を markdown.rs:199-255 と同型に改修:
   - `figure_queue` / `table_queue` を VecDeque で持ち、`for block in &section.blocks` の各 block 後に
     `insertion_point.after_block_index == Some(block.global_index)` のものを排出。
   - ループ後に残りを末尾へ flush。
   - **理想は P2-1 の `render_core::render_section_content`（escape=Plain, image_base=config.image_base,
     figure_format=config.figure_format）に丸ごと委譲**すること。P2-1 マージ済みなら委譲で二重実装を避ける。
3. `write_figure`（193-209）のパス生成を markdown.rs:263-275 と同一ロジックに:
   ```
   let filename = fig.image.path.file_name()... ;
   let path = if image_base.is_empty() { filename } else { format!("{}/{}", image_base, filename) };
   ```
   Phase 0 P0-4 が相対計算ヘルパ（例 `output::image_path::relative_link(image_dir, output_dir, path)`）を
   提供しているなら、markdown と**同じ関数**を呼ぶ（忠実度統一）。
4. `pdflay-python/src/lib.rs::to_llm_text`（339-358）に `image_base: &str`（signature 既定 `"./images"` 等）を追加し、
   `LlmTextConfig { image_base: image_base.to_string(), .. }` を組む。docstring 更新。

**シグネチャ変更:**
- `LlmTextConfig` に `image_base: String`（`#[serde(default)]`）追加。全構築箇所を更新:
  `llm_text.rs` tests（222-230, 418-421, 435-437, 452-455）, `pdflay-python/src/lib.rs:345-356`,
  （Rust 側で `LlmTextConfig` を構築する他箇所があれば grep で洗う）。`..Default::default()` を使うのが安全。
- Python `to_llm_text` の signature 変更（引数追加、既定値付きなので後方互換）。

**後方互換:**
- `image_base` は `#[serde(default)]`（空）。既存 serialized config は読める。
- 既定空だと従来の「生パス」ではなく「ファイル名のみ」になる＝**挙動改善だが変化**。旧来の絶対パス欲しい利用者は
  `image_base` に絶対ディレクトリを与える。PR で変化を明記。

**受け入れ基準（Given/When/Then）:**
- Given `fig.image.path = /abs/images/p000_img000.png`, `image_base="./img"`,
  When `to_llm_text`, Then 出力に `./img/p000_img000.png` が含まれ、`/abs/images/` を含まない。
- Given `image_base=""`, When 生成, Then `p000_img000.png`（ファイル名のみ、絶対パス漏れなし）。
- Given block(index=2) の後に insertion_point を持つ図, When 生成, Then 図placeholderが該当ブロック直後（セクション末尾ではない）に出る。
- Given markdown と llm_text を同一 doc・同一 image_base で生成, Then 両者の図パス文字列が一致する。

**追加テスト:**
- `llm_text` unit: `figure_uses_image_base_not_raw_path`, `figure_base_empty_is_filename_only`,
  `figure_inserted_at_insertion_point`, `table_inserted_at_insertion_point`。
- Python smoke（P2-6 と併せ）: `to_llm_text(image_base=...)` が例外なく動く。

**非スコープ:** 図の本文参照解析（Phase 4）, ベクター図抽出（Phase 4）。

**検証:**
```
cargo test -p pdf-lay-core
uvx maturin develop -m crates/pdflay-python/Cargo.toml
python -c "import pdflay; print('ok')"
```

---

### P2-6: CLI サブコマンド追加（json / chunks / llm-text）＋ `PyChunk.tables`

**目的:** Python 無しで RAG を完結させるため、CLI に `json` / `chunks`（JSONL） / `llm-text` を追加する。`PyChunk` に `tables` getter を足す。

**重大度 / 依存:** HIGH / P2-1（rich chunk）, P2-2（パンくず）, P2-3（tokenizer）, P2-5（llm image_base）。

**対象ファイル:**
- `crates/pdf-lay-cli/src/main.rs:12-13, 164-178, 249-320, 453-460`
- `crates/pdf-lay/src/lib.rs:79-81`（`JsonGenerator`/`Chunker` を re-export）
- `crates/pdflay-python/src/lib.rs:695-703`（`tables` getter）＋ 新 `PyTableInfo`

**現状（問題）:**
- `Commands`（`main.rs:164-178`）は `Toc` と `Markdown` のみ。json/chunks/llm-text が無く、RAG主経路がPython専用
  （提案書 L4, K4）。仕様書は `pdf-lay json` 等を約束しているが未実装（提案書 §1.6）。
- `pdf-lay` crate（`pdf-lay/src/lib.rs:79-81`）は output から `MarkdownGenerator` しか re-export しない。
  `JsonGenerator`・`Chunker` は未 re-export（`LlmTextGenerator`・`TocGenerator` は core 経由で re-export 済み）。
- `PyChunk`（`pdflay-python/src/lib.rs:638-714`）は `figures` getter（695-703）を持つが `tables` getter が無い。
  `PyTableInfo` 型自体が未定義。

**変更後の期待動作:**
- `pdf-lay json <PDF> [--content-only] [-o FILE]` … 文書を JSON 出力（`--content-only` は P2-7 の軽量 IR）。
- `pdf-lay chunks <PDF> [--max-tokens N] [--overlap N] [--strategy section|token|paragraph]
  [--section NAME]... [--tokenizer SPEC] [--no-section-context] [--image-base PATH] [-o FILE]`
  … **JSONL**（1行1 chunk）出力。各行に `chunk_id, section, text, estimated_tokens, page_range, figures, tables, has_continuation`。
- `pdf-lay llm-text <PDF> [--section NAME]... [--image-base PATH] [--figure-format ...] [-o FILE]`
  … LLM 向けプレーンテキスト。
- `PyChunk.tables` getter が `PyTableInfo` のリストを返す。

**実装手順:**
1. `pdf-lay/src/lib.rs` の output re-export（80）に `Chunker` と `JsonGenerator` を追加:
   ```
   pub use pdf_lay_core::output::{Chunker, JsonGenerator, MarkdownGenerator};
   ```
   （`LlmTextGenerator`・`TocGenerator` は既存 re-export。P2-3 の `Tokenizer`/`HeuristicTokenizer` も追加。）
2. `main.rs` の `Commands` enum（164-178）に3バリアント追加:
   ```
   Json(JsonArgs), Chunks(ChunksArgs), LlmText(LlmTextArgs)
   ```
   `about` / `long_about` / `after_long_help` は既存 `Toc`/`Markdown` と同じ密度で記述（レビュー方針「周囲を真似る」）。
3. 各 clap 構造体を `CommonArgs`（181-223）を flatten して定義（`MarkdownArgs` 249-320 に倣う）:
   ```
   #[derive(Args)] struct JsonArgs {
       #[command(flatten)] common: CommonArgs,
       /// content-only 軽量 IR（bbox/font を落とす。P2-7）。
       #[arg(long)] content_only: bool,
       #[arg(long, short='o', value_name="FILE")] output: Option<PathBuf>,
   }

   #[derive(Args)] struct ChunksArgs {
       #[command(flatten)] common: CommonArgs,
       #[arg(long, default_value_t = 4000, value_name="N")] max_tokens: usize,
       #[arg(long, default_value_t = 200,  value_name="N")] overlap: usize,
       /// section | token | paragraph
       #[arg(long, default_value = "section", value_name="STRAT")] strategy: String,
       #[arg(long = "section", value_name="NAME", num_args=1)] sections: Vec<String>,
       /// 実 tokenizer のモデル名 or tokenizer.json パス（空=改良ヒューリスティック）。
       #[arg(long, value_name="SPEC")] tokenizer: Option<String>,
       /// パンくず/見出し行の付与を無効化。
       #[arg(long)] no_section_context: bool,
       #[arg(long, default_value = "./images", value_name="PATH")] image_base: String,
       #[arg(long, short='o', value_name="FILE")] output: Option<PathBuf>,
   }

   #[derive(Args)] struct LlmTextArgs {
       #[command(flatten)] common: CommonArgs,
       #[arg(long = "section", value_name="NAME", num_args=1)] sections: Vec<String>,
       #[arg(long, default_value = "./images", value_name="PATH")] image_base: String,
       /// placeholder | markdown | caption | omit
       #[arg(long, default_value = "placeholder", value_name="FMT")] figure_format: String,
       #[arg(long, short='o', value_name="FILE")] output: Option<PathBuf>,
   }
   ```
4. ハンドラ:
   - `cmd_json(args)`: `run_analysis` → `--content-only` なら P2-7 の `JsonGenerator::generate_content_only`、
     でなければ `JsonGenerator::generate`。`write_output`（431-447）で出力。
   - `cmd_chunks(args)`: `run_analysis` →（`--section` があれば `doc.select_sections`）→
     `ChunkConfig { max_tokens, overlap_tokens: overlap, split_strategy: parse(strategy),
     include_section_context: !no_section_context, math_config: <Config.math_config を採用> }`。
     `--tokenizer` があれば `Chunker::with_tokenizer(cfg, load_tokenizer(spec)?)`、無ければ `Chunker::new(cfg)`。
     生成 chunk を **1件1行 JSON（JSONL）** で出力。各行:
     ```
     {"chunk_id":N,"section":"...","page_range":[s,e],"estimated_tokens":N,
      "has_continuation":bool,"text":"...","figures":[...],"tables":[...]}
     ```
     serde で `Chunk` を1件ずつ `serde_json::to_string`（pretty ではなく1行）して改行区切り。
     `strategy` が不正値なら `error:` を出して exit 1。
   - `cmd_llm_text(args)`: `run_analysis` →（section 選択）→ `LlmTextConfig { image_base,
     figure_format: parse(...), .. }` → `LlmTextGenerator::new(cfg).generate(&selected)`。全文書時は全 section を渡す。
     `figure_format` 不正値なら `error:` exit 1。
   - `strategy` / `figure_format` の parse は既存 Python（`lib.rs:135-139, 349-354`）と同じマッピングを CLI にも実装。
5. `main`（453-460）の `match` に3ハンドラを追加。
6. `--tokenizer` のロード関数 `fn load_tokenizer(spec: &str) -> Result<Box<dyn Tokenizer>, String>` を
   CLI 側に実装（feature `real-tokenizer` 有効時のみ実ロード、無効時はエラー。P2-3 手順5と一致）。
   CLI の `Cargo.toml` に `real-tokenizer` フィードスルー feature を追加してもよい。
7. **Python `PyChunk.tables`（695-703 の直後に追加）:**
   - 新 `#[pyclass(name="PyTableInfo")] struct PyTableInfo { inner: TableInfo }` を定義し、
     getter: `table_id`, `table_number`, `caption`, `page`, および表現テキスト
     `fn markdown(&self) -> Option<String>` / `fn as_text(&self) -> String`
     （`TableRepresentation` を文字列化。`document.rs:85-116` の3バリアントを分岐）。
   - `PyChunk` に:
     ```
     #[getter] fn tables(&self) -> Vec<PyTableInfo> {
         self.inner.tables.iter().map(|t| PyTableInfo{ inner: t.clone() }).collect()
     }
     ```
   - `#[pymodule]` 登録（`lib.rs:795` 付近の `m.add_class` 群）に `PyTableInfo` を追加。
   - `to_chunks`（123-151, 365-384）が P2-3 の tokenizer を活かせるよう、`tokenizer: Option<String>` /
     `section_context: bool` 引数を追加してもよい（既定値付きで後方互換）。最小実装では `tables` getter のみ必須、
     引数追加は任意（PR に判断を記す）。

**シグネチャ変更:**
- CLI: `Commands` に `Json`/`Chunks`/`LlmText` 追加。新 Args 構造体3つ。既存 `Toc`/`Markdown` は不変。
- `pdf-lay/src/lib.rs`: `Chunker`, `JsonGenerator`（＋`Tokenizer`,`HeuristicTokenizer`）re-export 追加。
- Python: `PyTableInfo` 新設、`PyChunk.tables` getter 追加、`#[pymodule]` に登録。

**後方互換:**
- 新サブコマンド追加は完全後方互換（既存コマンド不変。提案書 §5）。
- Python の getter 追加は後方互換。

**受け入れ基準（Given/When/Then）:**
- Given 実PDF, When `pdf-lay json paper.pdf`, Then valid JSON が stdout に出て終了コード0。
- Given 実PDF, When `pdf-lay chunks paper.pdf --strategy section`, Then 各行が独立 parse 可能な JSON で、
  `chunk_id`/`section`/`text`/`estimated_tokens`/`page_range`/`figures`/`tables` キーを持つ。
- Given `pdf-lay chunks paper.pdf --no-section-context`, Then chunk.text にパンくず行が無い。
- Given `pdf-lay llm-text paper.pdf --image-base ./img`, Then 出力の図パスが `./img/...`。
- Given `pdf-lay chunks paper.pdf --strategy bogus`, Then `error:` を stderr に出して非ゼロ終了。
- Given Python, When `doc.to_chunks()[0].tables`, Then `PyTableInfo` のリストが返る（例外なし）。

**追加テスト:**
- CLI unit（`main.rs` tests, 462-518 に倣う）: `json_subcommand_parses`,
  `chunks_subcommand_parses_all_flags`, `llm_text_subcommand_parses`,
  `chunks_long_help_lists_strategy`。clap の `try_parse_from` で構造検証。
- CLI smoke（フィクスチャ導入後、`tests/` またはシェル）: 各サブコマンドが実PDFで exit 0。
- Python: `test_pychunk_has_tables_getter`, `test_chunks_jsonl_roundtrip`（pytest 側があれば）。

**非スコープ:** バッチ処理・`debug-layout`・`analyze` 等の仕様書記載コマンド（Phase 3 の仕様同期で扱う）。

**検証:**
```
cargo test -p pdf-lay-cli
cargo run -p pdf-lay-cli -- json tests/fixtures/<sample>.pdf
cargo run -p pdf-lay-cli -- chunks tests/fixtures/<sample>.pdf --strategy section
cargo run -p pdf-lay-cli -- llm-text tests/fixtures/<sample>.pdf
uvx maturin develop -m crates/pdflay-python/Cargo.toml
python -c "import pdflay; d=pdflay.analyze('tests/fixtures/<sample>.pdf'); print(d.to_chunks()[0].tables)"
```

---

### P2-7: JSON content-only 射影オプション

**目的:** bbox/フォント幾何を落とし、数式変換済みテキストを含む軽量 IR を出力するオプションを追加する。

**重大度 / 依存:** MED-HIGH / P2-1（rich text 生成）。

**対象ファイル:**
- `crates/pdf-lay-core/src/output/json.rs:8-19`
- 新規 content-only 用の軽量型（`json.rs` 内 or `types/` に `content_ir.rs`）
- `crates/pdf-lay-cli/src/main.rs`（`--content-only` は P2-6 の `JsonArgs` で定義済み）
- `crates/pdflay-python/src/lib.rs:112-115`（`to_json` に `content_only` 引数、任意）

**現状（問題）:**
`JsonGenerator::generate`（`json.rs:10-12`）は `serde_json::to_string_pretty(doc)` の**生ダンプ**。
`PaperDocument` は `TextBlock.bbox`・`TextSpan.font_*`・`Rect` 等の幾何を全て含む（`types/text.rs`, `document.rs`）ため、
LLM に食わせるには重く、しかも `block.text` は**生テキスト**（数式未変換）。content-only の軽量射影が無い（提案書 P2-7）。

**変更後の期待動作:**
- content-only IR は「セクション階層＋数式変換済みテキスト＋図表の要約」のみを持ち、bbox/font/行span を持たない。
- `pdf-lay json --content-only` と（任意で）Python `to_json(content_only=True)` で出力できる。

**実装手順:**
1. 軽量型を定義（serde 対応、`Serialize` のみで可）:
   ```
   struct ContentDocument { paper_id, title, authors, doi, pages, sections: Vec<ContentSection> }
   struct ContentSection {
       header: Option<String>,     // clean_text
       level: u8,
       breadcrumb: Vec<String>,    // 祖先 clean_text（P2-2 と同アルゴリズム）
       text: String,               // render_section_content(escape=Plain, include_figures/tables=false)
       page_range: (u32,u32),
       figures: Vec<ContentFigure>,// figure_id, caption, image_path(basename), page
       tables:  Vec<ContentTable>, // table_id, caption, markdown/text, page
       children: Vec<ContentSection>,
   }
   ```
   bbox/font/`InsertionPoint`/`Rect`/`TextSpan`/`TextLine` は**含めない**。
2. `fn project_content(doc: &PaperDocument, opts: &RenderOptions) -> ContentDocument` を実装。
   text は P2-1 の `render_core::render_section_content`（数式変換済み）で生成。math_config は
   呼び出し側（CLI は `Config.math_config`）から供給。
3. `JsonGenerator` に:
   ```
   pub fn generate_content_only(doc: &PaperDocument, math_config: Option<&MathConfig>)
       -> Result<String, serde_json::Error>
   ```
   を追加。内部で `project_content` → `to_string_pretty`。
4. CLI `cmd_json`（P2-6）で `--content-only` 時にこれを呼ぶ。
5. （任意）Python `to_json`（112-115）に `#[pyo3(signature=(content_only=false))]` を足し、分岐。docstring 更新。

**シグネチャ変更:**
- `JsonGenerator::generate_content_only` 追加（既存 `generate`/`generate_sections` は不変）。
- 新 serialize 専用型（`pub` にするか `pub(crate)` かは公開範囲に応じて。CLI/Python から使うなら
  `output` から re-export）。
- （任意）Python `to_json` signature 変更（既定 false で後方互換）。

**後方互換:**
- 既定（`--content-only` 無し）は従来の生ダンプ。完全後方互換。

**受け入れ基準（Given/When/Then）:**
- Given 数式を含む doc, When `generate_content_only(.., Some(LaTeX))`, Then JSON の section.text に `\alpha` 等の変換済み表記が入り、`bbox`/`font_name` キーが**存在しない**。
- Given `generate_content_only`, Then valid JSON で `sections[].breadcrumb` を持つ。
- Given `generate`（従来）, Then 変わらず `bbox` を含む（回帰なし）。

**追加テスト:**
- `json` unit: `content_only_omits_geometry`（出力に `"bbox"` を含まない）,
  `content_only_includes_converted_math`, `content_only_is_valid_json`,
  `full_generate_still_includes_bbox`（回帰）。

**非スコープ:** JSON Schema の公開・バージョニング。

**検証:**
```
cargo test -p pdf-lay-core
cargo run -p pdf-lay-cli -- json tests/fixtures/<sample>.pdf --content-only
```

---

### P2-8: テーブル改善（colspan/rowspan・マルチヘッダ保持・罫線なし緩和）

**目的:** テーブルの結合セル（colspan/rowspan）をデータモデル化し、マルチヘッダを最終行に潰さず保持し、罫線なしテーブル検出の条件を緩める。

**重大度 / 依存:** MED（一部「best-effort, さらに調査」）/ なし（独立着手可）。ただし出力への反映は P2-1 の render-core と協調すると綺麗。

**対象ファイル:**
- `crates/pdf-lay-core/src/types/document.rs:85-116`（`TableRepresentation`）
- `crates/pdf-lay-core/src/table/grid_builder.rs:7-17, 78-113`（`TableGrid`）
- `crates/pdf-lay-core/src/table/text_converter.rs:14-67`（マルチヘッダ flatten）
- `crates/pdf-lay-core/src/config.rs:110-131`（`TableConfig`、罫線なし緩和の閾値）

**現状（問題）:**
- `TableGrid`（`grid_builder.rs:8-17`）は `header: Vec<Vec<String>>` / `rows: Vec<Vec<String>>` の**平坦セル**のみ。
  1セル = 1文字列で、colspan/rowspan の概念が無い（`build` 78-93 は各 block を単一 (row,col) に割り当てるだけ。提案書 L6）。
- `text_converter.rs::to_markdown`（19-27）:
  ```
  // Use the last header row (handles multi-header by flattening to the deepest level).
  grid.header.last().unwrap().clone()
  ```
  と**マルチヘッダを最終行だけに潰す**。`to_csv`（73-79）も同様。上位ヘッダ（グループ見出し）が消える。
- 罫線なしテーブルは `TableConfig.use_text_alignment`（`config.rs:119`）に依存するが、
  検出条件（min_columns=2 等）が厳しく、罫線なし表を取りこぼす（提案書 L6）。

**変更後の期待動作:**
- セルモデルに colspan/rowspan を導入（**best-effort**）。少なくとも「上位ヘッダを保持し、Markdown/CSV/PlainText に反映」する。
- マルチヘッダ（複数ヘッダ行）を最終行に潰さず、全ヘッダ行を出力する。
- 罫線なし検出の条件を緩和（設定で調整可能に）。

**実装手順:**
1. **データモデル（確実に実施）:**
   - `grid_builder.rs` に:
     ```
     pub struct Cell { pub text: String, pub colspan: usize, pub rowspan: usize } // 既定 1,1
     ```
     を導入。`TableGrid` を段階的に:
     - まず `header`/`rows` の `Vec<Vec<String>>` はそのまま残しつつ、
       `pub header_rows: Vec<Vec<Cell>>` を**追加**（後方互換のため旧フィールドは維持し、Cell.text から導出可能）。
       （破壊的にせず、新フィールド追加で段階移行。）
   - `TableRepresentation::Markdown`（`document.rs:88-97`）に
     `#[serde(default)] header_rows: Vec<Vec<String>>`（複数ヘッダ行）を追加。既存 `header: Vec<String>` は
     「flatten 済み（後方互換）」として残す。
2. **マルチヘッダ保持（確実に実施）:**
   - `text_converter.rs::to_markdown`（14-67）を、`grid.header` が複数行のとき
     **全ヘッダ行を出力**するよう変更（Markdown は1行目をカラム名、以降を区切り前の追加行として `| ... |` で出す。
     Markdown 仕様上ヘッダは1行なので、上位ヘッダ行は**データ行の直前に太字行として出す**か、
     GFM 準拠なら1行に結合しつつ `header_rows` を representation に保持して情報損失を防ぐ）。
   - 最小確実案: `TableRepresentation::Markdown.header_rows` に全ヘッダ行を格納し、
     `markdown_text` では従来どおり最終行を `|` ヘッダにしつつ、**上位ヘッダ行を `markdown_text` の
     テーブル直前に太字注記**として付す（情報を捨てない＝No Silent Drop の精神）。
   - `to_csv`（70-106）は全ヘッダ行を CSV の先頭複数行として出力（潰さない）。
   - `to_plain_text`（109-125）は既に全 header 行を出す（112-115）ので維持。
3. **colspan/rowspan（best-effort, さらに調査）:**
   - `grid_builder.rs::build`（31-113）で、空セルの連続や block の bbox 幅がカラム境界を跨ぐ場合に
     colspan を推定する。rowspan は縦方向の空セル連続から推定。
   - **これは検出精度が不確実なため best-effort とし**、推定不能時は colspan=rowspan=1 に倒す（安全側）。
   - Markdown 出力は colspan/rowspan を表現できないため、`header_rows`/`Cell` はデータモデルとしては持つが
     Markdown では近似（結合セルは同一テキストを繰り返す or 空セル）に留める。**この近似方針の是非は
     PR の「レビュアーへの質問」に明記**する（推測実装しない）。
4. **罫線なし検出の緩和（設定化）:**
   - `TableConfig`（`config.rs:110-131`）に `#[serde(default)] pub borderless_min_rows: usize`（既定 3 等）と
     必要なら整列許容の緩和パラメータを追加。マジックナンバーはコードに直書きせず config へ（レビュー方針 §1-6）。
   - 罫線なし判定を使う箇所（`table/` の detector）で新設定を参照する。**検出ロジックの具体緩和は
     さらに調査**とし、まず設定フックだけ用意して既定は現行同等（回帰させない）にする。

**シグネチャ変更:**
- `TableGrid` に `header_rows: Vec<Vec<Cell>>`（or 段階的に `Cell` 導入）追加。
- `TableRepresentation::Markdown` に `#[serde(default)] header_rows: Vec<Vec<String>>` 追加。
- `TableConfig` に `#[serde(default)] borderless_min_rows: usize`（＋任意の緩和パラメータ）追加。
- 新 `pub struct Cell`。全構築箇所（`text_converter.rs` tests, `grid_builder.rs` tests）を更新。

**後方互換:**
- 追加フィールドは全て `#[serde(default)]`。既存 serialized データは読める。
- 既存 `header: Vec<String>` / `rows` は維持（flatten 済みビューとして）。golden 変化（マルチヘッダ保持で
  出力にヘッダ行が増える）は PR で正当性説明。罫線なし緩和は既定で現行同等にし回帰させない。

**受け入れ基準（Given/When/Then）:**
- Given 2行ヘッダ（グループ見出し＋サブ見出し）を持つ grid, When `to_markdown`, Then 上位ヘッダのテキストが
  出力に残る（最終行だけに潰れていない）。
- Given 同 grid, When `to_csv`, Then CSV 先頭に2行のヘッダが出る。
- Given colspan を持つと推定される表, When 変換, Then `Cell.colspan >= 2` がデータモデルに現れる（best-effort、
  推定できない場合は 1 で安全に倒れ、パニックしない）。
- Given `TableConfig.borderless_min_rows` 変更, When 検出, Then 設定が反映される（既定では現行と同結果）。

**追加テスト:**
- `text_converter` unit: `multi_header_not_flattened_markdown`, `multi_header_preserved_csv`。
- `grid_builder` unit: `cell_default_span_is_one`, `colspan_inferred_when_span_wide`（best-effort、
  検出できるケースのみ）。
- `config` unit: `table_config_borderless_default`。
- 既存テーブルテスト（`text_converter.rs` 169-335, `grid_builder.rs` 245-416）が緑であること（回帰）。

**非スコープ（さらに調査 / 別タスク）:**
- Markdown で結合セルを厳密表現すること（GFM 非対応。近似で妥協）。
- 罫線なし検出ロジックの本格的な精度改善（実PDFフィクスチャでの評価が前提。Phase 4 と協調）。
- Type3/サブセットフォント由来の表崩れ（Phase 4 域）。

**検証:** `cargo test -p pdf-lay-core`。

---

## フェーズ完了の定義（Phase 2 Definition of Done）

本フェーズは、レビュー方針 §4 の共通 DoD に加えて、以下を**すべて**満たして完了とする。

1. **P2-1〜P2-8 の全タスクが個別 PR としてマージ済み**であり、各タスクの受け入れ基準（Given/When/Then）を満たす。
2. **単一の render-core が確立**している。markdown / llm_text / chunk の3経路が
   `render_core::render_block` を共有し、`convert_block_text_with_math` /
   `convert_block_text_for_llm` の二重実装が消えている（`grep` で両関数が存在しないこと）。
3. **chunk.text が rich text** である。数式変換済みテキスト・インラインテーブルmarkdown・図placeholder・
   （`include_section_context=true` 時）パンくずと見出し行を含む。
4. **RAG が CLI だけで完結**する。`pdf-lay json` / `pdf-lay chunks`（JSONL） / `pdf-lay llm-text` が実在し、
   実PDFで exit 0。`pdf-lay chunks` の各行が独立 parse 可能で `figures`/`tables` を含む。
5. **トークン予算が全戦略で守られる**。CJK 本文でも各 chunk が `max_tokens` を超えない
   （P2-3 の CJK chunk-size テストが緑）。`--tokenizer` で実 BPE を差せる seam が存在する。
6. **No Silent Drop 不変条件**: 入力の figure/table 総数 == 全 chunk の figure/table 総数（P2-4 の脱落ゼロテスト）。
   本フェーズで新たな無言 drop 経路を作っていない。
7. **公開API 3面が整合**: `pdf-lay` crate 再エクスポート（`Chunker`/`JsonGenerator`/`Tokenizer` 等）、CLI、Python
   （`PyChunk.tables` / `PyTableInfo`）がビルドを壊さず一貫している。
8. `cargo fmt --all --check` / `cargo clippy --all-targets --all-features -- -D warnings` /
   `cargo test --workspace` が緑。`cargo build -p pdf-lay-core --features real-tokenizer` が通る。
9. 追加した全 `#[serde(default)]` フィールド（`ChunkConfig.math_config`, `LlmTextConfig.image_base`,
   `TableRepresentation::Markdown.header_rows`, `TableConfig.borderless_min_rows` 等）で
   既存 serialized 設定/データが読めることを確認済み。
10. ドキュメントコメントが実装と一致（特に `config.rs:263-264` の `include_section_context`、
    `document.rs:200-201` の `estimated_tokens`、`text.rs:178-193` の `full_text` の用途注記が更新されている）。
