# Phase 0 — 正確な土台（bug-fix / quick wins）

**対象読者:** 本フェーズを実装するコーディングエージェント（Codex 等）およびレビュアー
**上位規範:** 本書は `docs/refactor/00_REVIEW_POLICY.md`（以下「方針書」）に従属する。食い違う場合は方針書を優先。
**位置づけ:** `REFACTORING_PROPOSAL.md` §3 Phase 0。訴え①（文章が全て文書化できていない）・③（画像埋め込み）＋横断課題（数式が動かない）の**根治**。

---

## 前文

### 目的

後続フェーズ（セクション再設計・LLM出力一本化）の土台となる**バグ修正の集合**。
いずれも「壊れているものを正しく動かす」変更であり、大規模な再設計は含まない。破壊的変更は最小で、
影響範囲が局所的なものから着手する。この5〜6タスクだけでユーザーの体感品質（①③・数式）が大きく改善する。

### なぜ Phase 0 が最優先か

- **P0-1（ページ寸法）** が直らない限り、A4 日本語 PDF では上端帯が毎ページ脱落し続け、
  後続の「セクション再現率」「網羅率」メトリクスが上流バグに汚染されて**測定不能**になる。
- **P0-3（数式ON）** は既存の数式サブシステムが「一度も呼ばれない死にコード」である状態の解消であり、
  新規実装ではなく配線ミスの是正。低コスト・高効果。
- **P0-4（相対パス）** は③の直接原因。

### 重要な前提事実（Phase 4 調査で確定）

`pdf_oxide 0.3.8` は **`get_page_info() -> PageInfo { media_box, crop_box, rotation }` を公開している**
（`~/.cargo` レジストリの `pdf_oxide-0.3.8/src/document.rs` 付近、`Document::get_page_info`）。
したがって現行 `pdf_reader.rs:72-77` のコメント「pdf_oxide does not expose a non-rendering page-size API」は**虚偽**であり、
P0-1 は**実 MediaBox を直接読む根治**が可能。スパン bbox 推定へのフォールバックは第2候補に留める。

> 実装者は着手時に、使用中の `pdf_oxide 0.3.8` の実 API シグネチャを
> `cargo doc -p pdf_oxide --open` もしくはレジストリソース（`~/.cargo/registry/src/**/pdf_oxide-0.3.8/`）で
> **必ず確認**すること。API名／戻り値が本書の記述と違う場合は方針書 §7 に従い、確認結果を PR に明記する。

### タスク一覧

| ID | 内容 | 重大度 | 依存 |
|----|------|--------|------|
| P0-1 | 実ページ寸法の取得（`get_page_info` / MediaBox） | CRITICAL | 独立（他タスクの前提） |
| P0-2 | 領域/カラムフィルタの全域化（No Silent Drop） | HIGH | P0-1 |
| P0-3 | 数式変換のデフォルト有効化＋`--math-format` | HIGH | 独立 |
| P0-4 | 画像リンクの相対パス計算 | HIGH | 独立 |
| P0-5 | drop 対象ブロック分類の厳格化 | HIGH | 独立（P0-2 と相補） |
| P0-6 | カバレッジ計測（抽出→出力到達） | MED | P0-1〜P0-5 の後 |

**推奨実施順:** P0-1 → P0-2 → P0-5 → P0-3 → P0-4 → P0-6。
（P0-3/P0-4 は独立なので並行可。ただし P0-6 は他タスクが入った後に計測基準を確定するため最後。）

---

### P0-1: 実ページ寸法の取得

**目的:** `page_dimensions()` が返す 612×792 固定値を撤廃し、各ページの実寸法（MediaBox）を返す。

**重大度 / 依存:** CRITICAL / 独立（P0-2・P0-6・座標正規化・カラム検出の全てが正しい寸法に依存）

**対象ファイル:** `crates/pdf-lay-core/src/extract/pdf_reader.rs:63-86`

**現状（問題）:**
```rust
/// Dimensions of the specified page in points.
///
/// Parses the page's MediaBox from the PDF dictionary.               // ← 虚偽
/// Falls back to US Letter size (612 × 792 pt) if the page cannot be found.
pub fn page_dimensions(&mut self, page: u32) -> Result<PageDimensions, PdfLayError> {
    let total = self.page_count();
    if page >= total {
        return Err(PdfLayError::PageOutOfRange(page, total));
    }
    // pdf_oxide does not expose a non-rendering page-size API, so we derive ... // ← 虚偽
    Ok(PageDimensions {
        page_number: page,
        width: 612.0,   // ← 定数
        height: 792.0,  // ← 定数
    })
}
```
コメントは「MediaBox をパース」と書くが実際は US Letter 定数を返す。A4（595×842）では
`block_grouper.rs:82`（`l.bbox.top <= region.y_top`、`y_top<=height=792`）により
`bbox.top > 792` の上端帯（タイトル・著者・最初の見出し）が毎ページ脱落する。横長ページも `center_x>=612` を失う。

**変更後の期待動作:** `page_dimensions(page)` は当該ページの実 MediaBox（回転考慮後）の幅・高さを points で返す。
取得不能なページのみ、そのページの抽出済みスパンの実測（max right / max top）にフォールバックし、
どちらも不能なら従来の 612×792 を最終フォールバックとしつつ **警告を積む**。

**実装手順:**
1. `pdf_oxide 0.3.8` の `Document::get_page_info(page)`（または相当 API）の実シグネチャを確認する
   （`~/.cargo/registry/src/**/pdf_oxide-0.3.8/src/document.rs`）。戻り値 `PageInfo` の `media_box`（`[f64;4]` 想定 `[x0,y0,x1,y1]`）と `rotation`（度）を使う。
2. `page_dimensions` を次の優先順で実装:
   - (a) `get_page_info(page)` 成功時: `w = (x1-x0).abs()`, `h = (y1-y0).abs()`。
     `rotation` が 90/270 の場合は `w` と `h` を入れ替える。
   - (b) (a) が失敗（Err/None）時: 当該ページの抽出スパンを走査し `width = max(span.bbox.right)`,
     `height = max(span.bbox.top)` を用いる。この経路では `PdfLayWarning::PageSkipped` ではなく
     新設 `PdfLayWarning::PageDimensionsFallback { page, method: "span-bbox" }`（下記）を積む。
   - (c) (b) も不能（スパン0件）時: 612×792 を返し `PageDimensionsFallback { page, method: "letter-default" }` を積む。
3. `error.rs` に警告バリアント `PageDimensionsFallback { page: u32, method: &'static str }` を追加し、
   `Display` 実装（PDF由来テキストを含めない方針を踏襲）を書く。
4. コメント（`pdf_reader.rs:63-80`）を実装と一致するよう全面改訂。**虚偽コメントを残さない**（方針書 §6）。
5. `pipeline.rs:61-72` の `page_dimensions` 呼び出しは変更不要だが、返る警告が `warnings` に載ることを確認。

**シグネチャ変更:** `page_dimensions` の pub シグネチャは不変。`error.rs` に `PdfLayWarning::PageDimensionsFallback` を追加（enum 追加は後方互換）。

**後方互換:** API 不変。フォールバック時のみ挙動が変わるが、これは修正であり許容。

**受け入れ基準（Given/When/Then）:**
- Given A4（595×842）の PDF、When `page_dimensions(0)`、Then `height ≈ 842`（±1pt）を返し 792 ではない。
- Given 90度回転ページ、When `page_dimensions`、Then 幅高が入れ替わって返る。
- Given get_page_info が使えないページ、When 呼び出し、Then span-bbox フォールバック値を返し `PageDimensionsFallback` 警告が1件積まれる。
- Given 従来の Letter サイズ PDF、When 呼び出し、Then 612×792 を返す（回帰なし）。

**追加テスト:**
- `page_dimensions_reads_real_mediabox_a4`（ゴールデン: 日本語A4フィクスチャ、height>800 を assert）
- `page_dimensions_swaps_on_rotation`（回転フィクスチャ or 合成、可能なら）
- `page_dimensions_fallback_emits_warning`（get_page_info 不能を模した経路で警告発生を assert）
- 既存 `analyze_pdf` 系テストが緑であること

**非スコープ:** 縦書き（writing-mode）対応は Phase 4。ここでは寸法と回転のみ。

**検証:**
```bash
cargo test -p pdf-lay-core extract::pdf_reader
cargo run -p pdf-lay-cli -- markdown tests/fixtures/<japanese_a4>.pdf --no-page-numbers | head -40  # 上端のタイトル/見出しが出力に含まれることを目視
```

---

### P0-2: 領域/カラムフィルタの全域化（No Silent Drop）

**目的:** `block_grouper` がどの行も必ずいずれかの (region, column) に割り当て、行を無言で捨てないようにする。

**重大度 / 依存:** HIGH / P0-1（正しい寸法が前提。誤寸法のまま全域化しても割当先がズレる）

**対象ファイル:** `crates/pdf-lay-core/src/structure/block_grouper.rs:73-92`

**現状（問題）:**
```rust
for region in regions {
    for column in &region.columns {
        let col_lines: Vec<&TextLine> = page_lines
            .iter().copied()
            .filter(|l| {
                l.bbox.center_x() >= column.left      // X: center 基準
                    && l.bbox.center_x() < column.right
                    && l.bbox.top <= region.y_top      // Y: 完全内包を要求
                    && l.bbox.bottom >= region.y_bottom
            })
            .collect();
        ...
    }
}
```
どの (region,column) の条件も満たさない行は**どのブロックにも入らず消える**。
特に (a) 領域境界を跨ぐ行（`top>y_top` または `bottom<y_bottom`）、(b) カラム範囲外に `center_x` がある行、
(c) P0-1 未修正時の上端帯。X は `center_x`、Y は完全内包という基準の不整合も一因。

**変更後の期待動作:** ページ上の全行を、最も近い (region, column) に**必ず**割り当てる。取りこぼしゼロを不変条件とする。
Y は「領域の Y 範囲との重なり（overlap）」で判定し、重なる領域が無ければ中心 Y が最も近い領域に割り当てる。
X は中心が属するカラム、無ければ最近傍カラム。

**実装手順:**
1. `group()` を「フィルタして残す」方式から「各行を一意の割当先に振り分ける」方式へ変更する。
   ページごとに `assignments: HashMap<(region_idx, col_idx), Vec<&TextLine>>` を用意。
2. 各 `line` について:
   - 領域選択: `region.y_bottom <= line.center_y <= region.y_top` を満たす領域があればそれ。
     無ければ `|line.center_y - region_midY|` 最小の領域（最近傍）。
   - カラム選択: 選んだ領域内で `column.left <= line.center_x < column.right` を満たすカラム、
     無ければ `center_x` に最も近いカラム（`|center_x - clamp(center_x, left, right)|` 最小）。
   - `assignments[(r,c)].push(line)`。
3. `regions` が空のページは従来どおり単一カラム（`group_column_lines(..., column_index=0)`）。ただし全行が入ることを保証。
4. 各 (r,c) について既存 `group_column_lines` を呼ぶ。**割当済み行数の合計 == ページ内行数** を
   `debug_assert!` で保証（リリースでは警告に）。
5. 不変条件を破った場合（理論上起きないが防御的に）、余った行を単一カラム扱いで拾い、`No Silent Drop` を守る。

**シグネチャ変更:** `group()` の pub シグネチャ不変。内部ロジックのみ変更。

**後方互換:** 出力ブロック集合は「これまで落ちていた行が増える」方向にのみ変化。既存テストが行数を厳密固定している場合は、増加が正当である旨を PR に明記して期待値を更新。

**受け入れ基準（Given/When/Then）:**
- Given 領域境界を跨ぐ行を含む合成 `lines`＋`layouts`、When `group()`、Then その行がいずれかのブロックに含まれる（消えない）。
- Given ページ内 N 行、When `group()`、Then 全ブロックの行数合計 == N。
- Given 単一カラム1領域、When `group()`、Then 既存の段落分割結果が回帰しない。

**追加テスト:**
- `group_assigns_every_line_no_drop`（合成: 境界跨ぎ行・カラム外行を含め、行数保存を assert）
- `group_boundary_straddling_line_kept`
- 既存 block_grouper テスト緑

**非スコープ:** 読み順（reading order）の改善・3カラム対応は Phase 1/後続。ここは「落とさない」ことのみ。

**検証:**
```bash
cargo test -p pdf-lay-core structure::block_grouper
```

---

### P0-3: 数式変換のデフォルト有効化 ＋ `--math-format`

**目的:** 全経路で `math_config: None` に固定され一度も動かない数式変換を、既定で有効化し CLI/Python から制御可能にする。

**重大度 / 依存:** HIGH / 独立

**対象ファイル:**
- `crates/pdf-lay-cli/src/main.rs:404-428`（`cmd_markdown` の `math_config: None`＝L415）＋ `MarkdownArgs`
- `crates/pdflay-python/src/lib.rs:89-106`（`to_markdown` の `math_config: None`＝L104）
- `crates/pdf-lay-core/src/config.rs:83-95`（`MarkdownConfig::default`）

**現状（問題）:**
```rust
// main.rs cmd_markdown
let md_config = MarkdownConfig {
    ...
    math_config: None,   // ← 数式変換が絶対に走らない
};
```
```rust
// python lib.rs to_markdown
let config = MarkdownConfig { ... math_config: None };  // ← 同上
```
数式は `math/` サブシステムが実装済みにもかかわらず、生グリフ（数式フォントの PUA 文字化け）で出力される。

**変更後の期待動作:** CLI `markdown` に `--math-format latex|unicode|plain|off`（既定 `latex`）を追加。
`off` 以外なら `MarkdownConfig.math_config = Some(MathConfig { representation, ..default })` を構築。
Python `to_markdown` に `math_format: &str = "latex"` kwarg を追加し同様に構築。

**実装手順:**
1. `main.rs` の `MarkdownArgs` に clap 引数 `math_format`（`--math-format`、`value_parser` で `latex|unicode|plain|off`、
   default `"latex"`）を追加。長ヘルプは既存スタイル（`long_help`）に合わせる。
2. `cmd_markdown` で `math_format` を `MathRepresentationPreference` にマップ:
   `latex→LaTeX, unicode→UnicodeMath, plain→PlainText, off→None(math_config自体をNone)`。
   `off` 以外は `Some(MathConfig { representation: <mapped>, ..MathConfig::default() })`。
3. `pdflay-python/src/lib.rs` の `to_markdown` の `#[pyo3(signature = ...)]` に `math_format = "latex"` を追加し、
   同じマッピングで `math_config` を構築。docstring 更新。`PySectionSelector::to_markdown` も同様に対応（引数追加）。
4. `config.rs` の `MarkdownConfig::default()` の `math_config` は **`None` のまま**据え置く
   （ライブラリのデフォルトは非破壊のまま、CLI/Python の入口で既定 ON にする）。理由: `MarkdownConfig` を直接使う
   既存 Rust 利用者の挙動を変えないため。CLI/Python 利用者にのみ既定 ON を提供する。

**シグネチャ変更:**
- `to_markdown` Python: 引数 `math_format="latex"` 追加（後方互換: デフォルト付き）。
- `MarkdownArgs` に `math_format` フィールド追加。

**後方互換:** ライブラリ `MarkdownConfig::default` は不変。CLI は新フラグ既定 `latex` で**出力が変わる**（数式が LaTeX 化）→ rc 段階のため許容。旧挙動は `--math-format off` で再現可能。

**受け入れ基準（Given/When/Then）:**
- Given 数式を含む PDF、When `pdf-lay markdown x.pdf`（フラグ無し）、Then 出力に `$...$`/`$$...$$` が現れる。
- Given `--math-format off`、When 実行、Then 従来どおり数式変換なし（生 `block.text`）。
- Given Python `doc.to_markdown()`（引数無し）、Then LaTeX 数式が出る。`to_markdown(math_format="off")` で無効化。

**追加テスト:**
- `cmd_markdown_enables_math_by_default`（CLI: `Cli::try_parse_from` で `math_format` 既定が `"latex"`）
- `math_format_off_disables_conversion`
- Python スモーク: `to_markdown` に `math_format` kwarg が存在

**非スコープ:** 数式変換アルゴリズム自体の改善（`\frac` 等）は Phase 2（P2-8 近傍で扱う）。ここは配線のみ。

**検証:**
```bash
cargo test -p pdf-lay-cli
uvx maturin develop -m crates/pdflay-python/Cargo.toml && \
  python -c "import pdflay,inspect;print('math_format' in inspect.signature(pdflay.PyPaperDocument.to_markdown).parameters if hasattr(pdflay,'PyPaperDocument') else 'skip')"
cargo run -p pdf-lay-cli -- markdown tests/fixtures/<arxiv>.pdf | grep -m1 '\$'
```

---

### P0-4: 画像リンクの相対パス計算

**目的:** Markdown 画像リンクを、出力ファイルの場所を基準に画像ディレクトリへの正しい相対パスで書く。

**重大度 / 依存:** HIGH / 独立

**対象ファイル:**
- `crates/pdf-lay-core/src/output/markdown.rs:263-278`（`write_figure`）
- `crates/pdf-lay-core/src/config.rs:63-95`（`MarkdownConfig`）
- `crates/pdf-lay-cli/src/main.rs:404-447`（`cmd_markdown` / `write_output`：出力パスを知っている場所）
- `Cargo.toml`（`pathdiff` 追加）

**現状（問題）:**
```rust
let filename = fig.image.path.file_name()...;          // basename だけ採用
let path = if self.config.image_base_path.is_empty() {
    filename
} else {
    format!("{}/{}", self.config.image_base_path, filename)  // 単純連結
};
```
オンディスクのディレクトリ（`--image-dir`）を捨て、`image_base` を素朴に前置するだけ。
出力先（`-o`）と画像ディレクトリの関係で相対パスを計算しないため、`-o docs/paper.md --image-dir out/images`
のように出力先を変えるとリンクが切れる。

**変更後の期待動作:**
- `--image-base` が**明示指定**された場合: 従来どおりその文字列を前置（利用者が意図した相対を尊重＝後方互換）。
- `--image-base` が未指定（既定）かつ `-o` でファイル出力する場合:
  リンクパス = `pathdiff::diff_paths(image_dir, output_dir)` ＋ `/filename`。
- stdout 出力（`-o` 無し）で `--image-base` 未指定の場合: 出力先ディレクトリが不定なので
  カレントディレクトリ基準（従来の `./images` 相当）にフォールバック。

**実装手順:**
1. `Cargo.toml` ワークスペース依存に `pathdiff = "0.2"` を追加、`pdf-lay-core` で使用宣言。
2. `MarkdownConfig` に `output_dir: Option<PathBuf>` と `image_dir: Option<PathBuf>`、
   `image_base_explicit: bool`（あるいは `image_base_path: Option<String>` に変更）を追加（`#[serde(default)]`）。
   → 「利用者が image_base を明示したか」を区別できるようにする。
3. `write_figure` のパス構築を次のロジックに置換:
   - `if let Some(base) = explicit image_base { format!("{base}/{filename}") }`
   - `else if let (Some(out), Some(img)) = (output_dir, image_dir) { diff_paths(img, out)/filename をスラッシュ表記に }`
   - `else { format!("./images/{filename}") 相当のフォールバック }`
   - パス区切りは Markdown 用に常に `/`（`pathdiff` 結果を `components` で `/` 連結）。末尾/先頭の重複スラッシュを除去。
4. `main.rs cmd_markdown`: `--image-base` が既定値のままか明示指定かを判定
   （clap の `ArgMatches::value_source` か、`Option<String>` 化して None=未指定）。
   `MarkdownConfig` に `output_dir = args.output.as_ref().and_then(|p| p.parent())`、
   `image_dir = Some(common.image_dir)` を渡す。
5. Python `to_markdown` は現状 CWD 相対運用が前提のため、`image_base_path` 明示扱いを維持（挙動不変）。
   将来 P2-5 で LLM 経路と統一する。

**シグネチャ変更:** `MarkdownConfig` にフィールド追加（`#[serde(default)]` で後方互換）。`write_output`/`cmd_markdown` 内部のみ変更。

**後方互換:** `--image-base` 明示時は完全に従来どおり。既定かつ `-o` 指定時のみ正しい相対に変わる（＝バグ修正）。`MarkdownConfig` 直接利用の Rust コードは新フィールド既定 `None` で従来挙動。

**受け入れ基準（Given/When/Then）:**
- Given `--image-dir out/images -o docs/paper.md`（image-base 既定）、When 実行、
  Then リンクは `../out/images/<file>` のように出力ファイルからの正しい相対になる。
- Given `--image-base ./images` 明示、When 実行、Then リンクは `./images/<file>`（従来どおり）。
- Given `-o` 無し（stdout）image-base 既定、When 実行、Then `./images/<file>` にフォールバック。
- Given 既存の markdown テスト、Then 回帰しない（明示 base 経路）。

**追加テスト:**
- `figure_link_is_relative_to_output_dir`（out/images と docs/ の関係で `../out/images/...` を assert）
- `explicit_image_base_preserved`
- `stdout_falls_back_to_default_base`

**非スコープ:** LLM/chunk 経路のパス（`llm_text.rs`）は P2-5。ここは markdown のみ。

**検証:**
```bash
cargo test -p pdf-lay-core output::markdown
mkdir -p /tmp/t/docs /tmp/t/out
cargo run -p pdf-lay-cli -- markdown tests/fixtures/<x>.pdf --image-dir /tmp/t/out/images -o /tmp/t/docs/p.md
grep -m1 '!\[' /tmp/t/docs/p.md   # 相対が ../out/images/ になっているか目視
```

---

### P0-5: drop 対象ブロック分類の厳格化（No Silent Drop）

**目的:** 本文を Caption / RunningHeader と誤分類して出力から消す事故を減らし、疑わしきは本文として残す。

**重大度 / 依存:** HIGH / 独立（P0-2 と相補的に「落とさない」を担保）

**対象ファイル:**
- `crates/pdf-lay-core/src/structure/block_classifier.rs:221-243`（`is_caption` / `is_running_header`）
- `crates/pdf-lay-core/src/output/markdown.rs:203-247`（block_type による `continue`）
- `crates/pdf-lay-core/src/types/text.rs:178-193`（`Section::full_text` の同種フィルタ）

**現状（問題）:**
```rust
fn is_caption(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.starts_with("fig.") || lower.starts_with("figure")
        || lower.starts_with("table") || lower.starts_with("tab.")
}
```
「Table 1 shows the results…」「Figure 2 illustrates…」等の**本文**が Caption 化し、
`markdown.rs:205-209` と `full_text` の無条件 `continue` で消える。
`is_running_header` も「単行 && font<0.85×body」だけで、小フォントの正当な1行を落とす。
さらに `markdown.rs` の `continue`（L209）は**図表ドレインもスキップ**する副作用がある（③の I4）。

**変更後の期待動作:**
- `is_caption`: 「Fig./Figure/Table/Tab. + 数字（省略可）」の**キャプション書式**にマッチし、かつ短い（例: ≤2行 かつ 実キャプション正規表現に合致）場合のみ Caption。単に "Table" で始まる長い本文は Caption にしない。
  → 実務上は `figure/caption_detector.rs` の正規表現を共有するのが望ましい（重複ロジックの一本化）。
- `is_running_header`: 反復検出（P1-2 で pipeline 接続予定の `detect_repeated_headers_footers`）と併用する前提で、
  Phase 0 では最低限「本文とみなせる長さ（例: `char` 数がしきい値超）なら RunningHeader にしない」ガードを足す。
- `markdown.rs` の描画ループ: block_type による `continue` が図表ドレインをスキップしないよう、
  **本文出力の skip と図表ドレインを分離**する（`continue` を使わず、本文を書くか否かのフラグで制御）。

**実装手順:**
1. `is_caption` を、`caption_detector.rs` の正規表現（`^(Fig\.?|Figure|TABLE|Tab\.)\s*\d*` 相当）に合致し、
   かつ `text.chars().count()` が短い（例 ≤ 200 char）ものだけ true にする。長文誤爆を除去。
   可能なら判定関数を `caption_detector` 側に寄せて共有（重複排除）。
2. `is_running_header` に長さガードを追加: `block.text.chars().count()` が本文相当（例 > 60 char）なら false。
3. `markdown.rs:203-247` の `match block.block_type { Caption|PageNumber|Running* => continue, _ => {...} }` を、
   「本文を書くか」を決める `let emit_body = !matches!(block_type, Caption|PageNumber|RunningHeader|RunningFooter);`
   に置き換え、`if emit_body { 本文出力 }` としたうえで**図表ドレイン while ループは常に実行**する。
   これにより I4（キャプション型ブロックにアンカーされた図が末尾に飛ぶ）も解消。
4. `text.rs` `full_text` も同様に、明確な PageNumber/Running* のみ除外し、Caption は
   「実キャプションと確定したもの」だけ除外する方針に合わせる（過剰除外をやめる）。
5. しきい値（200 char / 60 char）は方針書 §1-6 に従い `Config`（新設 `ClassifierConfig` か既存 `HeaderDetectionConfig` 近傍）へ。直書きしない。

**シグネチャ変更:** 分類器の private 関数の中身のみ。しきい値を持つため `BlockClassifier` 構築に config を渡す形になり得る（その場合 `pipeline.rs:147` の `BlockClassifier::from_blocks` 呼び出しを更新）。

**後方互換:** 「これまで消えていた本文が出る」方向の変化のみ。既存テストが除外を固定していれば正当性を PR に明記して更新。

**受け入れ基準（Given/When/Then）:**
- Given 本文「Table 1 shows the accuracy of ...（長文）」、When 分類、Then `BodyText`（Caption ではない）で、Markdown/full_text に出力される。
- Given 実キャプション「Fig. 1: Overview ...」、When 分類、Then Caption のまま（回帰なし）。
- Given 図がキャプション型ブロックにアンカーされたセクション、When markdown 生成、Then 図が本文中の想定位置に出る（末尾に飛ばない）。
- Given 60char 超の小フォント1行、When 分類、Then RunningHeader にならない。

**追加テスト:**
- `body_sentence_starting_with_table_not_caption`
- `real_caption_still_classified`
- `figure_drain_not_skipped_by_caption_block`（markdown.rs）
- `long_small_font_line_not_running_header`

**非スコープ:** 反復ヘッダ/フッタ除去の pipeline 接続は Phase 1（P1-2）。ここは誤分類ガードと描画ループの修正のみ。

**検証:**
```bash
cargo test -p pdf-lay-core structure::block_classifier
cargo test -p pdf-lay-core output::markdown
```

---

### P0-6: カバレッジ計測（抽出→出力到達）

**目的:** 「抽出した文字がどれだけ出力に到達したか」を計測し、以降の全変更が網羅率を回帰させないための基準線を作る。

**重大度 / 依存:** MED / P0-1〜P0-5 が入った後（計測基準を確定するため最後）

**対象ファイル:**
- `crates/pdf-lay-core/src/pipeline.rs`（`analyze_pdf` 末尾で計測）
- `crates/pdf-lay-core/src/error.rs`（警告 or メトリクス型）
- `crates/pdf-lay-core/src/types/document.rs`（メトリクスを載せる場合）

**現状（問題）:** 抽出スパン総量に対し、どれだけが最終セクション本文に残ったかを計測する術がない。
「文章が全て文書化できていない」という主観的訴えを、客観指標で追えない。

**変更後の期待動作:** `analyze_pdf` が抽出スパン総 char 数と、セクションに到達した本文 char 数、
脱落ブロック数を算出し、到達率が低い（例 < 0.9）場合に警告を積む。可能なら `AnalysisResult` に
`coverage: Coverage { extracted_chars, emitted_chars, dropped_blocks }` を追加。

**実装手順:**
1. `types/document.rs` に `pub struct Coverage { pub extracted_chars: usize, pub emitted_chars: usize, pub dropped_blocks: usize, pub ratio: f64 }`（serde 対応）を追加。
2. `pipeline.rs` で:
   - `extracted_chars` = `spans` の `text.chars().count()` 合計。
   - `emitted_chars` = 生成された `sections` を走査し `Section::full_text().chars().count()` 合計（＋ヘッダー）。
   - `dropped_blocks` = 抽出→ブロック化→セクション到達の各段の差分（最低限、group 後ブロック数 と セクション収容ブロック数の差）。
3. `ratio = emitted/extracted`（0除算ガード）。`ratio < config` しきい値なら
   `PdfLayWarning::LowCoverage { ratio }`（新設）を積む。しきい値は `Config` に（既定 0.9 など）。
4. `AnalysisResult` に `coverage: Coverage` を追加（構造体フィールド追加＝呼び出し側 CLI/Python の更新が必要）。
   CLI では `--verbose` 相当時に stderr へ、Python では属性として公開（任意、最小実装は警告のみでも可）。
5. 最小実装は「警告のみ」でも受理可。`AnalysisResult` 拡張まで行う場合は CLI/Python/`pdf-lay` crate の3面を更新（方針書 §4）。

**シグネチャ変更:** `AnalysisResult` にフィールド追加（採用時）。`error.rs` に `LowCoverage` 追加。

**後方互換:** 警告・メトリクスは加算のみ。既存出力は不変。`AnalysisResult` 拡張時は構造体更新式の呼び出し側を全て直す。

**受け入れ基準（Given/When/Then）:**
- Given 正常な PDF、When `analyze_pdf`、Then `coverage.ratio` が算出され、健全なら警告なし。
- Given 大量脱落を模した入力、When 実行、Then `LowCoverage` 警告が積まれる。
- Given 既存テスト、Then 緑（メトリクス追加で壊れない）。

**追加テスト:**
- `coverage_ratio_computed`
- `low_coverage_emits_warning`

**非スコープ:** セクション精度・読み順などの高次メトリクス（方針書 §8.3）は横断タスク／CI で。ここは文字到達率のみ。

**検証:**
```bash
cargo test -p pdf-lay-core pipeline
cargo run -p pdf-lay-cli -- markdown tests/fixtures/<x>.pdf 2>&1 1>/dev/null   # 警告に coverage 情報が出るか
```

---

## フェーズ完了の定義

Phase 0 は以下を**すべて**満たして完了とする:

1. P0-1〜P0-6 が各々別 PR でマージ済み（方針書 §2）。
2. 各タスクの受け入れ基準（Given/When/Then）を満たすテストが存在し緑。
3. `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` 緑。
4. 公開API変更（`AnalysisResult` / Python `to_markdown` / `MarkdownArgs`）が CLI・Python・`pdf-lay` crate の3面に反映済み。
5. **虚偽コメントが残っていない**（特に `pdf_reader.rs` の page_dimensions）。
6. ゴールデン用フィクスチャ（少なくとも日本語A4）で、P0-1 前は落ちていた上端テキストが出力に含まれることを目視確認。
7. P0-6 の `coverage.ratio` が基準線として記録され、以降のフェーズの回帰監視に使える状態。

> 横断依存: ゴールデンテスト用の実 PDF フィクスチャ（日本語A4・IEEE・arXiv）は方針書 §8.2 の横断タスクで用意する。
> 未整備の間は合成データ主体のユニットテストで受け入れ基準を満たし、フィクスチャ導入後にゴールデンを追加する。
