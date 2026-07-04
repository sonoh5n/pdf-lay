# Phase 4 — 抽出堅牢化（要調査）

**対象読者:** 本フェーズを実装するコーディングエージェント（Codex 等を含む）
**位置づけ:** 本書は `docs/refactor/00_REVIEW_POLICY.md`（以下「方針書」）と
`docs/refactor/REFACTORING_PROPOSAL.md`（以下「提案書」）に従属する。食い違う場合は方針書を優先。
**対応する提案書:** §3 Phase 4（P4-1〜P4-4）、問題点 T5 / T7 / I5 / I6。

---

## 前文 — このフェーズは「調査ゲート付き」である

Phase 4 は他フェーズと**性質が違う**。CJK・縦書き・スキャン・ベクター図・inline画像といった対象は、
どこまでが `pdf_oxide` ライブラリ側の限界で、どこからが pdf-lay 側の実装で救えるのかが
**コードを読むだけでは確定しない**。したがって本フェーズの各タスクは次の順序を厳守する。

> **investigate-first（調査優先）の鉄則**
> 1. **まず実 PDF で `pdf_oxide` の挙動を観測する**（下記「§0 事前調査プロトコル」）。
> 2. 観測結果を **アウトカムノート**（後述の指定パス）に記録する。
> 3. その記録を根拠に、初めてコードを書く。
> **観測なしに「たぶんこう動く」で実装を始めることを禁止する（方針書 §6「推測実装」）。**

限界がライブラリ側（`pdf_oxide`）にあると判明した場合は、**推測で回避コードを盛らず**、
方針書 §7 のエスカレーション手順に従う。具体的には:

- 最小限の安全側（No Silent Drop・後方互換維持・パニック回避）に倒し、
- 「pdf_oxide のここが限界だった」を観測ログ付きで PR 説明の「レビュアーへの質問」に書き、
- 受け入れ基準を満たせないならその旨を明記して人間の判断を待つ。

本フェーズは提案書 §4 で「要調査・後回し可」に分類されている。ただし提案書 §4 の但し書き
「**日本語 PDF で①がP0-1後も残るなら P4-1 を前倒し**」に該当する場合は優先度が上がる。

### Phase 0 への依存（重要）

Phase 4 の全タスクは、**Phase 0 の P0-1（実ページ寸法の取得）がマージ済み**であることを前提とする。
現状 `crates/pdf-lay-core/src/extract/pdf_reader.rs:67-86` の `page_dimensions()` は
MediaBox を読まず `612×792` を定数返ししており、そのドキュメントコメント
（「Parses the page's MediaBox」）は**虚偽**である。

本フェーズの調査で判明した事実として、`pdf_oxide 0.3.8` は
**`PdfDocument::get_page_info(page) -> PageInfo { media_box, crop_box, rotation }`**
（`document.rs:4501`、`PageInfo` は `document.rs:29-36`）を公開しており、MediaBox / CropBox / 回転角は
非レンダリングで取得できる。つまり pdf_reader のコメントは事実に反する。P0-1 はこの API を使って
ページ寸法を正すはずである。**P4 の各タスクはページ寸法が正しい前提で書く。** もし P4 着手時点で
P0-1 が未マージなら、着手せず P0-1 の完了を待つ（方針書 §2 手順4）。

---

## §0 事前調査プロトコル（全タスク共通の観測手順）

各タスクの「事前調査」節はここを参照する。**まずこれを実行し、結果をアウトカムノートに残す。**

### 0.1 フィクスチャ（横断タスク X-1 依存）

方針書 §8.2 の実 PDF フィクスチャを使う。最低限、以下を `tests/fixtures/` に用意する
（未整備なら X-1 で用意されるまで待つか、調査用に一時 PDF を各自入手してパスを記録する）:

- `ja_a4.pdf` … **A4・1段組・日本語**論文（本文が埋め込みテキスト。CJK/Identity-H の検証に必須）
- `ja_vertical.pdf` … 縦書きを含む日本語 PDF（入手できれば。無ければ「入手不可」と記録）
- `scanned.pdf` … スキャン/画像onlyページを含む PDF（埋め込みテキストがほぼ無い）
- `vector_fig.pdf` … ベクター図（線画・matplotlib 由来等）とキャプションを含む PDF

> フィクスチャが揃わない項目は「未検証（フィクスチャ未整備）」とアウトカムノートに明記する。
> **黙って飛ばさない。**

### 0.2 観測ハーネス

追加クレートを作らず、`crates/pdf-lay-core` の `#[cfg(test)] #[ignore]` テスト、または
`cargo run -p pdf-lay-cli -- toc <pdf>` / `-- markdown <pdf> -o /tmp/out.md` で観測する。
pdf_oxide 生挙動を直接見たい場合は、`extract` 配下に **調査専用の `#[ignore]` テスト**を置き、
`PdfReader::open` → `inner_doc()`（`pdf_reader.rs:185`）経由で
`extract_spans` / `extract_images` / `get_page_info` を直接叩いて `eprintln!` で観測する
（このテストは本タスクの成果物ではなく調査用。PR に含めるかは各タスクの指示に従う）。

### 0.3 記録先（アウトカムノート）

観測結果は **`docs/refactor/phase4_findings.md`** に追記する（無ければ新規作成）。
各タスクはこのノートの該当セクションを埋めてから実装に入る。ノートに書くこと:

- 使った PDF（ファイル名・ページ数・種別）
- 観測コマンドと生出力の要約（文字化けの有無、脱落箇所、画像数、形式）
- **診断:** 損失が pdf_oxide 側か pdf-lay 側か（下記デシジョンツリー参照）
- 採った選択肢と理由（エスカレーションした場合はその旨）

---

## 参考: pdf_oxide 0.3.8 能力調査（実ソース確認済み）

> 出典は `~/.cargo/registry/src/**/pdf_oxide-0.3.8/src/` の実ソース。行番号はそのツリー内。
> **実装前にこの表を鵜呑みにせず、§0 の実 PDF 観測で裏を取ること**（ソースにあっても実 PDF で効くとは限らない）。

| 機能 | 対応状況 | 根拠（pdf_oxide 内 file:line） |
|------|----------|-------------------------------|
| ページ寸法（MediaBox/CropBox/回転） | **対応**。`get_page_info` が返す | `document.rs:4501`, `PageInfo` = `document.rs:29-36` |
| CID/Type0 フォント | **対応**。`FontDict.cid_to_gid_map`（Identity/Explicit）、`cid_system_info`（Adobe-Japan1/GB1/CNS1/Korea1） | `fonts/font_dict.rs:22-73` |
| ToUnicode CMap | **対応**。`LazyCMap`、bfchar/bfrange/notdefrange パーサ、>U+FFFF 対応 | `fonts/cmap.rs:1-460` |
| Identity-H（横書き CID） | **対応**（優先度2の predefined CMap 処理） | `fonts/font_dict.rs:1139,1782-1804` |
| ToUnicode 欠落時のフォールバック | **対応**。埋め込み TrueType cmap（「70-80% recovery」）＋ predefined CMap（UniJIS-UCS2-H 等） | `fonts/font_dict.rs:44-46,1899-1922`, `fonts/cid_mappings/*` |
| Identity-V（縦書きの**文字デコード**） | **部分**。文字コード→Unicode は Identity-H と同経路でデコードされる | `fonts/font_dict.rs:1139,1168,2019` |
| 縦書きの**レイアウト/読み順** | **限定的/未確認**。`pdf_oxide::layout::TextSpan`（pdf-lay が受け取る型）に**回転/writing-mode フィールドが無い**。`layout/text_block.rs:139-141` に orientation 概念はあるが span には露出しない | `pdf_reader.rs` の TextSpan 構築（`extract_spans`）／`layout/text_block.rs:139` |
| 回転ページ/回転テキスト | **要観測**。`get_page_info().rotation` はページ回転角を返すが、span bbox が回転適用済みかは実 PDF で確認 | `document.rs:29-36` |
| inline 画像（BI/ID/EI） | **対応**。`extract_images` が `Operator::InlineImage` を処理し `extract_image_from_inline` で `PdfImage` 化 | `content/operators.rs:414`, `extractors/images.rs`（`extract_images` の Do/Inline 分岐） |
| Form XObject | **対応**。`extract_images` が Do を辿り Form を**再帰**（テキストも `extractors/text.rs:4211` で再帰） | `extractors/images.rs`（`extract_images_from_xobject_do`）, `extractors/paths.rs:63` |
| 画像カラースペース | **広く対応**。`ColorSpace`: DeviceRGB/Gray/CMYK, Indexed, CalGray/RGB, Lab, ICCBased(n), Separation, DeviceN, Pattern | `extractors/images.rs:412-445` |
| CMYK 画像 | **対応**。`cmyk_to_rgb` で RGB 変換。`PixelFormat::CMYK` | `extractors/images.rs:819` |
| JPEG（DCT）画像 | **対応**。`ImageData::Jpeg` は DCT パススルー、`save_as_jpeg` あり | `extractors/images.rs:394-396,227` |
| CCITT / JBIG2（1bit bilevel、スキャン系） | **対応**（デコーダあり）。`CcittParams` を `PdfImage` が保持 | `decoders/ccitt.rs`, `decoders/jbig2.rs`, `extractors/images.rs:114-174` |
| **SMask / ソフトマスク（透明度）** | **未対応**。画像抽出経路で SMask を適用しない（`compliance/` と `writer/` のみが SMask を参照） | 全 grep で `extractors/images.rs` に SMask 無し |
| **JPX（JPEG2000）** | **未対応**。`decoders/` に jpx デコーダが存在しない | `decoders/`（ccitt/dct/jbig2/flate/lzw/… に jpx 無し） |
| 画像保存 API | `PdfImage::save_as_png` / `save_as_jpeg` / `to_png_bytes` / `to_dynamic_image`（`image::DynamicImage`） / `to_base64_data_uri` | `extractors/images.rs:196,227,247,303,280` |
| 画像メタ | `PdfImage::color_space()` / `bits_per_component()` / `data()`（`&ImageData`） / `bbox()` / `width()` / `height()` | `extractors/images.rs:134-174` |
| **OCR（スキャン）** | **内蔵**（`ocr` feature）。ONNX Runtime（`ort`）+ PaddleOCR系（DBNet++ 検出 + SVTR 認識）。**ONNX モデルファイル（det/rec/dict）が別途必要** | feature `ocr = [dep:ort, imageproc, ndarray]`（`Cargo.toml`）, `ocr/mod.rs`, `ocr/engine.rs` |
| OCR API | `extract_spans_with_ocr`（`#[cfg(feature="ocr")]`, `document.rs:2536`）, `extract_text_with_ocr`（`document.rs:2510`）, `ocr::needs_ocr`, `OcrEngine::new(det, rec, dict, OcrConfig)`, `OcrExtractOptions` | `document.rs:2510-2560`, `ocr/mod.rs:48-56` |

**pdf-lay 側が現在受け取っている型**（`convert_span` の入力、`pdf_reader.rs:211`）は
`pdf_oxide::layout::TextSpan`。そのフィールドは（`pdf_reader.rs` のテスト `351-367` で確認可能）
`text, bbox, font_name, font_size, font_weight, is_italic, color, mcid, sequence,
split_boundary_before, offset_semantic, char_spacing, word_spacing, horizontal_scaling,
primary_detected`。**回転角・writing-mode・CID 生値は露出していない。** よって縦書きレイアウトや
回転テキストの読み順補正は pdf-lay 側では bbox 幾何からの推定に頼るしかない（これが P4-1 の争点）。

### 診断デシジョンツリー（損失がどちら側かの判定）

```
実PDFで文字が欠落/文字化けしている
├─ pdf_oxide の extract_spans / extract_text の生出力を直接観測（§0.2）
│   ├─ 生出力ですでに欠落/化け → pdf_oxide 側の限界
│   │     ├─ CJK かつ ToUnicode 欠落が原因か？（フォント dict を観測）
│   │     │     → predefined CMap / truetype_cmap フォールバックが効いているか確認
│   │     ├─ スキャン（画像onlyでテキスト無し）→ P4-2（OCR/部分回復）へ
│   │     └─ それ以外（縦書き順・回転）→ pdf-lay で幾何補正できるか P4-1 で切り分け
│   │           救えないなら §7 エスカレーション（pdf_oxide upgrade / 代替抽出器 / OCR）
│   └─ 生出力は正しいのに pdf-lay 出力で欠落 → pdf-lay 側の問題
│         ├─ convert_span の degenerate-bbox drop（pdf_reader.rs:231-233）で落ちた？
│         ├─ per-page エラー skip（pdf_reader.rs:124-127）でページ丸ごと落ちた？
│         └─ 下流（block_grouper 領域フィルタ／レンダラの block_type continue）で落ちた？
│               → これらは Phase 0/1 のスコープ。P4 では「pdf-lay 側原因」と記録し該当フェーズへ回す
```

---

## タスク一覧

| ID | タイトル | 重大度 | 依存 | 主対象 | 種別 |
|----|----------|--------|------|--------|------|
| P4-1 | pdf_oxide のCJK/縦書き/スキャン挙動の検証 | HIGH（T5） | Phase 0 P0-1 | `pdf_reader.rs`, findings ノート | **調査主体** |
| P4-2 | スキャン/画像onlyページのOCR連携 or 部分回復 | MED（T5/T7） | P4-1 の観測 | `extract/`, `pipeline.rs`, `config.rs` | 実装（フラグ/feature） |
| P4-3 | ベクター図・inline画像・Form XObject の抽出 + 保存形式 | MED（I5） | P4-1 の観測 | `image_extractor.rs`, `figure/*`, `types/document.rs` | 実装 |
| P4-4 | caption 正規表現の拡張 | MED（I6） | なし（先行可） | `caption_detector.rs`, `config.rs` | 実装 |

> **推奨着手順:** P4-1（調査）→ その結果を見て P4-2 / P4-3 → P4-4 は独立に先行可。
> P4-2 と P4-3 は P4-1 のアウトカムノートが埋まってから着手する。

---

### P4-1: pdf_oxide のCJK/縦書き/スキャン挙動の検証

**目的:** 実 PDF（日本語A4・縦書き・スキャン）で `pdf_oxide 0.3.8` の抽出挙動を観測し、テキスト損失が
ライブラリ側か pdf-lay 側かを切り分けて、後続タスク（P4-2/P4-3）の方針を確定する。

**重大度 / 依存:** HIGH（提案書 T5） / Phase 0 P0-1（実ページ寸法）マージ済みであること。**本タスクは主に調査であり、
プロダクションコードの変更を最小化する**（コード変更ゼロで完了しうる。成果物は findings ノート）。

**対象ファイル:**
- 観測対象: `crates/pdf-lay-core/src/extract/pdf_reader.rs:93-130`（`extract_text_spans` / `extract_all_text_spans`、
  per-page エラー skip `124-127`）、`convert_span:211-240`（degenerate-bbox drop `231-233`）
- 成果物: `docs/refactor/phase4_findings.md`（新規/追記）
- 調査用 `#[ignore]` テスト: `crates/pdf-lay-core/src/extract/pdf_reader.rs` の `#[cfg(test)]`

**現状（問題）:**
提案書 T5 が指摘するとおり、CID/CJK(Identity-H)・縦書き・回転・スキャン PDF の取りこぼしが疑われるが、
**根本原因が未特定**。pdf-lay 側には損失を作りうる箇所が複数ある:

```rust
// pdf_reader.rs:230-233  convert_span — 退化 bbox を無言で捨てる（No Silent Drop 違反の疑い）
if ow <= 0.0 || oh <= 0.0 {
    return None;
}
```
```rust
// pdf_reader.rs:124-127  extract_all_text_spans — 1ページの pdf_oxide エラーでそのページ全テキスト破棄
Err(e) => {
    log::warn!("Skipping page {page} due to extraction error: {e}");
}
```
一方 §「能力調査」のとおり pdf_oxide は CID/ToUnicode/predefined CMap を実装している。よって
**日本語が化ける原因が pdf_oxide なのか pdf-lay の drop なのか**は、実観測しないと確定できない。

**事前調査（このタスクで確認すべきこと）:**
§0 のプロトコルで以下を**必ず**埋める。各項目に「どう確かめるか」を併記する。

1. **CJK（横書き）デコード可否** — `ja_a4.pdf` で `pdf_oxide::PdfDocument::extract_text(page)` の生文字列を観測。
   → 日本語が正しい Unicode で出るか？ PUA/`?`/□ に化けていないか？
   → 化けている場合、そのフォントの subtype/encoding を観測（`get_page_resources` からフォント dict を辿るか、
   pdf_oxide の debug バイナリ `bin/debug_extraction.rs` 相当を `#[ignore]` テストで再現）。
   ToUnicode 欠落か、predefined CMap 非対応かを切り分け。
2. **pdf-lay 経由での損失** — 同じ PDF で `extract_all_text_spans()`（pdf-lay）を通し、生 `extract_text` と
   **文字数・内容を突き合わせ**。差分が出たら、`convert_span` の `ow<=0||oh<=0` drop（`231-233`）で
   何スパン落ちたか、per-page skip（`124-127`）で何ページ落ちたかを計数（一時的に `eprintln!` を仕込む）。
3. **縦書き** — `ja_vertical.pdf` があれば、(a) 文字自体は読めるか（Identity-V デコード）、
   (b) 読み順が破綻していないか（bbox から縦組みの列順が復元できそうか）を観測。無ければ「未検証」と記録。
4. **回転ページ** — `get_page_info(page).rotation` の値と、span bbox が回転後座標かを観測。
5. **スキャン** — `scanned.pdf` で `extract_text`/`extract_spans` が空/ごく僅かかを確認し、`ocr::needs_ocr` 相当の
   判定（テキスト総量閾値）で「OCR 対象ページ」を列挙。→ P4-2 のインプットにする。

**変更後の期待動作:**
`docs/refactor/phase4_findings.md` に上記5項目の観測結果と診断（損失の主因がどちら側か）が記録される。
必要なら**低リスクな pdf-lay 側修正のみ**を本タスクに含めてよい:

- `convert_span` の `ow<=0||oh<=0` drop を **No Silent Drop 準拠に**（§後述の受け入れ基準）。
  ただし退化 bbox の扱い変更が下流に波及するため、**観測で「実際に文字が落ちている」ことを確認できた場合のみ**行う。
- per-page skip（`124-127`）の**部分回復は P4-2 のスコープ**。本タスクでは warning 経路の確認に留める。

pdf_oxide 側限界が主因と判明した場合は**コードを書かず**、findings とエスカレーションで完了とする。

**実装手順:**
1. §0.1 のフィクスチャを確認（無ければ X-1 待ち、または調査用 PDF のパスを findings に記録）。
2. 調査用 `#[ignore]` テストを `pdf_reader.rs` の tests に追加し、上記「事前調査」1〜5 を観測。出力を findings に貼る。
3. **診断デシジョンツリー**（§参考）に沿って各損失を「pdf_oxide 側 / pdf-lay 側」に分類。
4. 分岐:
   - **pdf-lay 側の drop が主因**（例: degenerate bbox で日本語スパンが落ちている）→ 本タスクで
     `convert_span` を No Silent Drop 準拠に最小修正（下記シグネチャ変更）。
   - **pdf_oxide 側が主因（CID デコード不能等）** → §7 エスカレーション。findings に「pdf_oxide 限界」と明記し、
     **選択肢を列挙**して PR の「レビュアーへの質問」に書く:
     - (A) pdf_oxide のバージョン更新（0.3.8 → 上位）で改善するか要確認、
     - (B) 代替抽出器の併用（該当フォントのみ）、
     - (C) OCR フォールバック（P4-2 に接続）。
     暫定案を1つ提示するが、**本タスクでは実装しない**。
   - **スキャンが主因** → OCR 対象ページ一覧を findings に残し P4-2 へ。
5. 縦書き/回転が pdf-lay の幾何補正で救えるか（bbox のアスペクト比・列クラスタリング）を検討し、
   救えるなら P4-1 で試作せず**別タスク化を提案**（スコープクリープ防止）。救えないなら §7。

**シグネチャ変更 / 新フラグ・feature:**
（原則コード変更なし。ただし pdf-lay 側 drop が主因と確定した場合のみ）
- `convert_span` の drop を warning 収集可能にするため、戻り値を
  `Option<TextSpan>` → `Result<TextSpan, DegenerateSpan>` 相当にするか、
  degenerate スパンを**呼び出し側（`extract_text_spans`/`extract_all_text_spans`）で計数**できるようにする。
  最小変更として、`extract_*` が `(Vec<TextSpan>, usize /*dropped*/)` を返す内部ヘルパを持ち、
  `pipeline.rs` で `PdfLayWarning` に記録する案を推奨（下記「後方互換」）。
- **新規 warning バリアント**（`crates/pdf-lay-core/src/error.rs:61` の `PdfLayWarning`）:
  `DegenerateSpanDropped { page: u32, count: usize }` を追加（No Silent Drop の受け皿）。

**後方互換:**
公開 API `extract_text_spans`/`extract_all_text_spans` のシグネチャは**変えない**（CLI/Python/`pdf-lay` crate が呼ぶ）。
drop 計数は内部ヘルパ or `pipeline.rs` 側の突き合わせで実現し、外部シグネチャを維持する。
新 warning は既存 `AnalysisResult.warnings` に積むだけなので後方互換。

**受け入れ基準（Given/When/Then）:**
- Given `ja_a4.pdf`、When 調査テストで `pdf_oxide::extract_text(0)` を観測、Then findings に
  「日本語が Unicode で出る/化ける」の**明確な結論**と根拠（フォント encoding）が記録されている。
- Given 生 `extract_text` と pdf-lay `extract_all_text_spans` の文字数、When 突き合わせ、Then 差分の
  内訳（degenerate drop 数・page skip 数・下流フィルタ）が findings に定量記録されている。
- Given degenerate bbox の日本語スパンが実在すると観測された、When 修正を入れる、Then そのスパンは
  破棄されず（近傍受け皿へ）または `PdfLayWarning::DegenerateSpanDropped` に計上され、**無言破棄が消える**。
- Given pdf_oxide 側限界が主因、When 本タスク完了、Then コードは変更されず、findings と PR に
  エスカレーション（選択肢A/B/C＋暫定案）が明記されている。

**追加テスト:**
- ユニット: `convert_span` を変更した場合のみ、degenerate bbox が「破棄されず計数される」ことを合成データで検証。
- ゴールデン（`#[ignore]`／フィクスチャ依存, X-1 待ち）: `ja_a4.pdf` で
  「出力到達文字数 ÷ pdf_oxide 生抽出文字数」が閾値以上（回帰させない）を測る `#[ignore]` テスト。
- **`ja_a4.pdf` と `scanned.pdf` のフィクスチャが前提**（横断タスク X-1 で整備）。未整備なら `#[ignore]` で置き、
  PR にその旨明記。

**非スコープ:**
OCR 実装（P4-2）、画像/ベクター図（P4-3）、caption 拡張（P4-4）、縦書きレイアウト補正の本実装、
block_grouper/レンダラの下流フィルタ修正（Phase 0/1）。本タスクは**観測と最小の No-Silent-Drop 修正のみ**。

**検証:**
- `cargo test -p pdf-lay-core`（新 `#[ignore]` テストは手動 `-- --ignored` で実行）。
- `cargo fmt --all --check` / `cargo clippy --all-targets --all-features -- -D warnings`。
- 手動: `cargo run -p pdf-lay-cli -- markdown tests/fixtures/ja_a4.pdf -o /tmp/ja.md` の出力を目視、
  日本語本文の欠落有無を確認。
- **アウトカムノートに記録:** §0.3 の全項目（使用 PDF、生出力要約、診断、採った選択肢）。findings が
  埋まっていることを PR のレビュアーへの質問欄に要約。

---

### P4-2: スキャン/画像onlyページのOCR連携 or 部分回復

**目的:** テキストの無い（スキャン/画像only）ページに対し、オプションの OCR 経路と部分回復経路を用意し、
1ページの抽出失敗が文書全体をゼロにしないようにする。

**重大度 / 依存:** MED（提案書 T5/T7） / **P4-1 の観測結果**（どのページが OCR 対象かの一覧）。

**対象ファイル:**
- `crates/pdf-lay-core/src/extract/pdf_reader.rs:112-130`（`extract_all_text_spans` の per-page skip）
- `crates/pdf-lay-core/src/extract/`（OCR 連携のシーム。新規 `ocr.rs` を置く候補）
- `crates/pdf-lay-core/src/pipeline.rs:56-72`（span 抽出と warning 収集）
- `crates/pdf-lay-core/src/config.rs:9`（`Config`）、`crates/pdf-lay-core/src/error.rs:61`（`PdfLayWarning`）
- CLI: `crates/pdf-lay-cli/src/main.rs`（`--ocr` フラグ追加）

**現状（問題）:**
```rust
// pdf_reader.rs:119-128  1ページのエラーで、そのページの全テキストを黙って失う（warning のみ、部分回復なし）
for page in 0..total {
    match self.inner.extract_spans(page as usize) {
        Ok(raw_spans) => { all.extend(raw_spans.into_iter().filter_map(|s| convert_span(s, page))); }
        Err(e) => { log::warn!("Skipping page {page} due to extraction error: {e}"); }
    }
}
```
- スキャンページは `extract_spans` が空 or エラーになり、**テキストが一切出ない**。OCR フォールバックが無い（T5）。
- per-page エラーは `log::warn!` のみで **`AnalysisResult.warnings` に載らない**（`pipeline.rs` は `extract_all_text_spans` の
  内部 skip を観測できない）。No Silent Drop の観点で穴（T7）。

**事前調査（このタスクで確認すべきこと）:**
1. **OCR 方式の決定**（§能力調査より二択）:
   - (A) **pdf_oxide 内蔵 OCR**（`ocr` feature）。ONNX Runtime（`ort`）と ONNX モデル（det/rec/dict）が必要。
     API: `extract_spans_with_ocr`（`document.rs:2536`, `#[cfg(feature="ocr")]`）, `OcrEngine::new(det,rec,dict,OcrConfig)`。
     → **要確認:** `ort` の追加が pdf-lay のビルド/配布（CLI バイナリ、Python wheel）に与える影響、モデルファイルの入手・同梱可否。
   - (B) **外部 OCR にシェルアウト**（`tesseract` が PATH にあれば呼ぶ）。ページ画像を一時 PNG に落として
     `tesseract <png> stdout -l jpn+eng` を実行し、テキストを回収。ネイティブ依存を pdf-lay に持ち込まない。
   → **どちらを既定にするか**は、`ort`/モデル同梱のコスト vs `tesseract` 前提の可搬性で判断。**迷ったら (B) を優先**
     （方針書 §7 の安全側：重い必須依存を増やさない）。両方を排他 feature/フラグにする案も可。findings に決定を記録。
2. **需要判定** — 「OCR 対象ページ」の判定閾値（ページのテキスト総文字数 < N）。P4-1 の観測で得た scanned.pdf の
   実測から N を決め、`Config` 化する（マジックナンバー禁止, 方針書 §1-6）。
3. **tesseract 検出方法**（(B) の場合）— `which tesseract` 相当（`std::process::Command` で `--version` を叩く）。
   無ければ OCR を**静かにスキップせず** warning を出す。

**変更後の期待動作:**
- **既定（フラグ無し）:** 挙動は不変。スキャン/エラーページは従来どおりテキスト無しだが、**必ず
  `PdfLayWarning` に記録**される（部分回復＝No Silent Drop の最低ライン）。
- **`--ocr` 指定時（かつ OCR feature/エンジン利用可能時）:** テキストが閾値未満のページに OCR をかけ、
  得られたテキストを `TextSpan` 化して合流。OCR も失敗したら warning。
- **部分回復:** `extract_all_text_spans` は 1ページのエラーで**そのページのみ**を欠損扱いにし、他ページは通す
  （現状も他ページは通すが、欠損が warnings に載る点を保証する）。

**実装手順:**
1. `PdfLayWarning`（`error.rs:61`）に **`PageTextRecovered { page, method }`**（OCR で回復）と
   **`PageTextMissing { page, reason }`**（テキスト無し/エラーで未回復）を追加。
2. **抽出シームを `pipeline.rs` 側に寄せる**。`extract_all_text_spans` の内部 skip を廃し、
   `pipeline.rs` が **ページ単位で** `reader.extract_text_spans(page)` を呼ぶループに変更
   （per-page の Ok/Err を pipeline が観測でき、warning に積める）。**公開 API は維持**（下記後方互換）。
3. OCR シームを `crates/pdf-lay-core/src/extract/ocr.rs`（新規）に置く。トレイト or 関数
   `fn ocr_page(reader: &mut PdfReader, page: u32, cfg: &OcrConfig) -> Result<Vec<TextSpan>, PdfLayError>`。
   - (A) 採用時: `#[cfg(feature = "ocr")]` で pdf_oxide の `extract_spans_with_ocr` をラップ。feature off 時は
     `--ocr` 指定でも「OCR ビルドされていません」warning を出して回復せず継続。
   - (B) 採用時: `std::process::Command` で tesseract をシェルアウト。ページ画像は
     `ImageExtractor` 経由 or pdf_oxide のページラスタライズ（`rendering` feature 要）で得る。
     **注意:** ページ全体のラスタライズ手段が無ければ、スキャンページに含まれる**全面画像 XObject**を
     `extract_images` で取り出して OCR に渡す（P4-3 と共有）。
4. `pipeline.rs` のループ: ページのネイティブ span が空/閾値未満なら、`config.ocr.enabled` かつ
   エンジン利用可なら `ocr_page` を試行 → 成功で `PageTextRecovered`、未回復で `PageTextMissing`。
5. `Config`（`config.rs:9`）に `#[serde(default)] pub ocr: OcrConfig` を追加。`OcrConfig` は
   `enabled: bool`（既定 false）, `min_native_chars: usize`（需要判定閾値, 既定は P4-1 実測から）,
   `lang: String`（既定 `"jpn+eng"`）, `engine: OcrEngineKind`（`Tesseract`/`Builtin`）。全フィールドに `#[serde(default)]`。
6. CLI（`main.rs`）に `--ocr`（bool）と任意で `--ocr-lang` を追加し `config.ocr` へマップ。Python バインディング側も
   OCR 設定を受けられるよう getter/引数を追加（公開 API 3面, 方針書 §4-4）。

**シグネチャ変更 / 新フラグ・feature:**
- 新 CLI フラグ: `--ocr`（既定 off）, `--ocr-lang <lang>`。
- 新 cargo feature（pdf-lay 側）: `ocr`（内蔵OCRを使う (A) 案のとき pdf_oxide の `ocr` feature を有効化）。既定 off。
- `Config.ocr: OcrConfig` 追加（`#[serde(default)]`）。
- `PdfLayWarning::{PageTextRecovered, PageTextMissing}` 追加。
- `extract_all_text_spans` は**維持**（後方互換）。pipeline は per-page 版 `extract_text_spans` を使う実装へ移行。

**後方互換:**
`--ocr`/`ocr` feature とも**既定 off**。OCR 無効時の唯一の挙動差は「スキャン/エラーページが warnings に載る」点で、
テキスト出力は不変。`OcrConfig` は全フィールド `#[serde(default)]` なので既存設定 JSON は壊れない。
公開関数シグネチャは変えない。

**受け入れ基準（Given/When/Then）:**
- Given `scanned.pdf`、When `--ocr` 無しで解析、Then テキスト出力は従来どおりだが各スキャンページに
  `PageTextMissing` warning が付き、**他ページの本文は失われない**。
- Given `scanned.pdf` かつ OCR エンジン利用可、When `--ocr` 付きで解析、Then スキャンページから
  テキストが回復し `PageTextRecovered` が付く（回復ゼロなら `PageTextMissing`）。
- Given 1ページだけ pdf_oxide がエラーを返す PDF、When 解析、Then そのページのみ欠損（warning 付き）で
  他ページは正常抽出される（**全文ゼロ化しない**）。
- Given `--ocr` 指定だが OCR feature 未ビルド/tesseract 不在、When 解析、Then パニックせず warning を出して継続。

**追加テスト:**
- ユニット: 需要判定（閾値未満で OCR 対象になる）、tesseract 不在時に warning を出し継続する分岐（`Command` は
  モック or 存在しないバイナリ名で失敗経路を検証）。
- ゴールデン（`#[ignore]`, X-1 の `scanned.pdf` 依存）: `--ocr` off で `PageTextMissing` が出ること、
  on で回復すること（OCR エンジンが CI に無ければ `#[ignore]` + 手動）。
- **`scanned.pdf` フィクスチャが前提（横断 X-1）。** 未整備なら `#[ignore]`。

**非スコープ:**
OCR の精度チューニング、モデルの同梱配布、縦書き OCR、ページ全面ラスタライズの高精度化。
（ベクター図/画像形式は P4-3、caption は P4-4。）

**検証:**
- `cargo test -p pdf-lay-core -p pdf-lay-cli`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo fmt --check`。
- 手動: `cargo run -p pdf-lay-cli -- markdown tests/fixtures/scanned.pdf --ocr -o /tmp/ocr.md`（エンジンある環境）と
  `--ocr` 無しの差分を比較。warnings が出ることを確認。
- findings に「採用した OCR 方式（A/B）と理由、閾値 N の根拠、CI で OCR を回すか」を記録。

---

### P4-3: ベクター図・inline画像・Form XObject の抽出 + 保存形式

**目的:** ラスタ XObject 以外（ベクター図・inline画像・Form XObject）の図を救い、PNG 強制をやめて
実フォーマット（JPEG/その他）を尊重し、SMask/CMYK を検出して正しく扱う（少なくとも検出＋warning）。
さらに 1画像の失敗でページ全画像を捨てる挙動を per-image に緩める。

**重大度 / 依存:** MED（提案書 I5） / P4-1 の観測（対象 PDF での画像/ベクター図の実態）。ベクター図は
`reader.extract_all_paths()`（`pdf_reader.rs:174`）が既にテーブル検出で読んでいるので**再利用**する。

**対象ファイル:**
- `crates/pdf-lay-core/src/extract/image_extractor.rs:30-108`（`extract_all` の per-page all-or-nothing `36-41`、
  `extract_page_images` の PNG 強制 `61-70`・形式ハードコード `103`・欠損 bbox プレースホルダ `87-92`）
- `crates/pdf-lay-core/src/types/document.rs:28-37`（`ImageFormat`）, `29-37`
- `crates/pdf-lay-core/src/extract/pdf_reader.rs:143-180`（`extract_paths` / `extract_all_paths`）
- `crates/pdf-lay-core/src/types/path.rs`（`PathObject`, `PathType`）
- `crates/pdf-lay-core/src/pipeline.rs:74-87,158-179`（画像抽出とキャプション/図マッチ、`UnmatchedCaption`）
- `crates/pdf-lay-core/src/error.rs:61`（`PdfLayWarning`）, `config.rs:9`

**現状（問題）:**
```rust
// image_extractor.rs:35-42  1ページの extract_page_images エラーで、そのページの全画像を破棄
for page in 0..page_count {
    match self.extract_page_images(reader, page) {
        Ok(images) => all_images.extend(images),
        Err(e) => { log::warn!("Image extraction skipped for page {page}: {e}"); }
    }
}
```
```rust
// image_extractor.rs:61-70,103  常に PNG で保存し、format も Png 固定（JPEG/Other を無視）
let filename = format!("p{page:03}_img{num:03}.png");
raw_img.save_as_png(&path) ...
format: ImageFormat::Png,
```
```rust
// image_extractor.rs:87-92  bbox 欠損/退化 → 原点の単位矩形プレースホルダ（位置情報を捨てる）
Rect::new(0.0, 1.0, 1.0, 0.0)
```
- **ラスタ XObject のみ**を保存対象にしているように見えるが、§能力調査のとおり pdf_oxide の `extract_images` は
  **inline 画像・Form XObject 内の画像も返す**。つまり pdf-lay は既にそれらを受け取れる可能性が高い
  （P4-1/本タスクの観測で確認）。一方**ベクター図（線画）は画像 XObject ではない**ので `extract_images` には出ず、
  キャプションだけ残って `UnmatchedCaption`（`pipeline.rs:174`）になる。
- PNG 強制で JPEG 由来画像を再エンコード（劣化・肥大）。`ImageFormat::Jpeg`/`Other` は未生成（型はあるのに死んでいる）。
- SMask（透明）・CMYK の正しさは未検証（pdf_oxide は CMYK→RGB するが SMask は適用しない＝背景化けの恐れ）。
- 1画像の失敗でページ全画像喪失（all-or-nothing）。

**事前調査（このタスクで確認すべきこと）:**
1. **inline/Form 画像は本当に取れているか** — `vector_fig.pdf` と inline 画像を含む PDF で
   `reader.inner_doc().extract_images(page)` の返り値を観測。inline/Form 由来画像が `PdfImage` として
   返るか、bbox は妥当か（§能力調査では対応。**実 PDF で裏取り**）。
2. **各 `PdfImage` の実フォーマット** — `raw_img.data()`（`ImageData::Jpeg` か `Raw`）と
   `raw_img.color_space()`（`extractors/images.rs:144`）を観測。DCT(JPEG) なら `save_as_jpeg` で無劣化保存できる。
3. **SMask の有無** — pdf_oxide は SMask 非適用（§能力調査）。対象 PDF に SMask 付き画像があるか、
   あるなら見た目がどう壊れるか（黒背景・アルファ喪失）を観測。**救えないなら検出＋warning に留める**。
4. **JPX/JBIG2/CCITT** — 対象画像の Filter を観測。JPX は pdf_oxide 未対応（§能力調査）→ その画像は
   `extract_images` がエラー/未生成になるはず。エラー時に per-image で warning し継続できるようにする。
5. **ベクター図の抽出方針** — ベクター図は `extract_all_paths()` の `PathObject`（線分/矩形）の**空間的まとまり**。
   キャプション近傍の path 群を「figure 領域」として束ねられるか（P4-1/本タスクで簡易クラスタリングの可否を判断）。
   **本タスクのベクター図対応は「キャプションに path 群を紐付けて図として記録（＋領域 bbox）」までを最小スコープ**とし、
   ベクター→ラスタ変換（レンダリング）は非スコープ（`rendering` feature 依存が重いため。必要なら §7）。

**変更後の期待動作:**
- **形式の尊重:** `PdfImage.data()` が `Jpeg` なら `.jpg` で `save_as_jpeg`、`Raw` なら PNG で保存し、
  `ImageInfo.format` を実フォーマット（`Jpeg`/`Png`/`Other`）に正しく設定。ファイル名の拡張子も一致。
- **per-image 回復:** ある画像の保存/デコード失敗は**その画像だけ**スキップし warning、同ページの他画像は保存。
- **bbox 欠損の扱い:** 原点プレースホルダで**位置を捏造しない**。bbox 不明画像は
  `PdfLayWarning::ImageBboxUnknown` を出し、図マッチから除外（誤配置を避ける）か、
  ページ末尾配置＋warning のいずれか（No Silent Drop 準拠でどちらかに寄せ、既定は「除外＋warning」）。
- **SMask/CMYK:** CMYK は pdf_oxide が RGB 変換するのでそのまま。SMask 付きは検出し
  `PdfLayWarning::ImageSMaskIgnored` を出す（見た目劣化の可能性を告知）。
- **inline/Form 画像:** `extract_images` が返す限り、通常のラスタ画像と同様に保存・記録される
  （観測で「返っている」ことを確認済みであることが前提）。
- **ベクター図:** キャプション近傍の `PathObject` クラスタを図として記録し、`UnmatchedCaption` を減らす
  （最小スコープ：領域 bbox とキャプション紐付けまで。ラスタ化はしない）。

**実装手順:**
1. `image_extractor.rs` の `extract_page_images` を、`raw_img.data()` で分岐:
   `ImageData::Jpeg` → 拡張子 `.jpg` + `save_as_jpeg`（`extractors/images.rs:227`）、
   それ以外 → `.png` + `save_as_png`。`ImageInfo.format` を実フォーマットに設定
   （pdf_oxide の `PdfImage::color_space()`/`data()` から `crate::types::ImageFormat` へマップ。
   JPX 等未対応は `ImageFormat::Other("jpx")` として保存失敗を warning）。
2. **per-image エラー緩和:** `extract_page_images` 内の各画像処理を `for` で回し、
   `save_*` 失敗を `?` で早期 return せず、その画像を warning 化してスキップ・継続。
   （現状 `map_err(...)?` で 1画像失敗＝ページ全体失敗になっている `image_extractor.rs:66-70` を是正。）
3. `extract_all`（`36-41`）の per-page は既に継続だが、per-image 化により all-or-nothing が二重に解消される。
   ページ単位失敗の warning も `pipeline.rs` の warnings に載せる（現状 `pipeline.rs:80-85` は
   `extract_all` の Err のみ拾う。per-image warning を `ImageExtractor` が返せるよう
   `extract_all` の戻り値に warning を同伴させるか、`&mut Vec<PdfLayWarning>` を渡す）。
4. **bbox 欠損:** `image_extractor.rs:87-92` の原点プレースホルダをやめ、`Option<Rect>` を保持したまま
   `pipeline.rs` へ渡し、bbox 不明画像は `ImageMatcher`（`figure/image_matcher.rs`）のマッチ対象から除外
   ＋ `PdfLayWarning::ImageBboxUnknown`。`ImageInfo` の `raw_bbox`/`normalized_bbox` を `Option` にするか、
   別途 `bbox_known: bool` を持たせる（**公開型変更のため 3面同期に注意**、下記後方互換）。
5. **SMask 検出:** pdf_oxide が SMask 情報を露出していない場合、`PdfImage` からは判定できない。その場合は
   「SMask 検出は pdf_oxide 非対応のため未実装、透明画像は劣化しうる」と findings に明記し、
   **エスカレーション（§7）**（pdf_oxide への機能要望 or upgrade）。露出していれば warning を出す。
6. **ベクター図（最小）:** `pipeline.rs` の図マッチ後、未マッチの Figure キャプションについて、
   同ページの `extract_all_paths()` の `PathObject` を空間近傍でクラスタリングし、キャプション近傍に
   十分な path 群があれば `FigureInfo`（画像パス無し・領域 bbox 付き）として記録。閾値（近傍距離・最小 path 数）は
   `Config` 化。これで `UnmatchedCaption` が減る。**paths はラスタ化しない。**
7. 図に画像実体が無い（ベクター）ケースを `FigureInfo`/`ImageInfo`/レンダラが表現できるようにする
   （`ImageInfo.path` を `Option` にするか、`FigureInfo` に「vector（画像なし）」種別を持たせる。型変更は 3面同期）。

**シグネチャ変更 / 新フラグ・feature:**
- `types/document.rs` の `ImageInfo`: bbox 不明・ベクター図を表現するためのフィールド変更
  （`bbox` の `Option` 化 or `bbox_known: bool` 追加、`path: Option<PathBuf>` 化）。**`#[serde(default)]` 必須。**
- `PdfLayWarning` に `ImageSMaskIgnored { page }`, `ImageBboxUnknown { page }`,
  `ImageDecodeFailed { page, reason }` を追加。
- `Config` に `#[serde(default)] pub figure_vector: VectorFigureConfig`
  （`enabled: bool` 既定 true、`cluster_gap_pt: f64`、`min_paths: usize`）。全フィールド `#[serde(default)]`。
- `ImageExtractor::extract_all` が warning を同伴して返す（or `&mut Vec<PdfLayWarning>` 引数）。

**後方互換:**
`ImageInfo` の型変更は**破壊的**なので、CLI / Python（`pdflay-python`）/ `pdf-lay` crate の全参照を更新する
（方針書 §6「公開APIの片手落ち変更」禁止）。JSON 出力の後方互換は `#[serde(default)]` と、可能なら
旧フィールド名の維持で担保。形式尊重（JPEG保存）は既定 on だが、**旧挙動（常に PNG）に戻す
`Config.force_png: bool`（既定 false）** を用意して回帰を選べるようにする（提案書 §5 の互換方針）。
ベクター図記録は既定 on だが `figure_vector.enabled=false` で従来どおり `UnmatchedCaption` に倒せる。

**受け入れ基準（Given/When/Then）:**
- Given JPEG(DCT) 由来画像を含む PDF、When 抽出、Then `.jpg` で無劣化保存され `format == Jpeg`
  （`force_png=true` 時のみ従来どおり PNG 再エンコード）。
- Given 1ページに複数画像があり 1枚が JPX で失敗、When 抽出、Then その 1枚のみ warning でスキップされ
  **残りの画像は保存される**（ページ全滅しない）。
- Given bbox 不明の画像、When 抽出、Then 原点プレースホルダは作られず `ImageBboxUnknown` warning が出て、
  誤った位置に図が配置されない。
- Given SMask 付き画像、When 抽出、Then（pdf_oxide が情報を出す範囲で）`ImageSMaskIgnored` warning が出る、
  または findings に「pdf_oxide 非対応のため検出不可」と記録されエスカレーションされている。
- Given ベクター図＋キャプションの PDF、When 抽出、Then そのキャプションは `UnmatchedCaption` にならず、
  path クラスタと紐付いた図として記録される。

**追加テスト:**
- ユニット: `ImageData::Jpeg`/`Raw` → 保存拡張子・`format` マッピングの分岐（合成 `PdfImage`）。
  per-image 失敗時に他画像が残ること（1枚を失敗させるスタブ）。bbox 欠損時に warning＋マッチ除外。
  ベクター図クラスタリングの近傍判定（合成 `PathObject`）。
- ゴールデン（`#[ignore]`, X-1 の `vector_fig.pdf` 依存）: ベクター図キャプションが図化され `UnmatchedCaption` が減ること。
- **`vector_fig.pdf` フィクスチャが前提（横断 X-1）。** 未整備なら `#[ignore]`。

**非スコープ:**
ベクター図のラスタ化（SVG/PNG レンダリング）、SMask の合成適用、JPX デコード、色管理（ICC 厳密変換）、
OCR（P4-2）、caption 拡張（P4-4）。

**検証:**
- `cargo test --workspace`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo fmt --check`。
- Python/CLI の 3面ビルド（`ImageInfo` 型変更のため）: `cargo run -p pdf-lay-cli -- markdown ...` と
  `uvx maturin develop -m crates/pdflay-python/Cargo.toml` が通ること。
- 手動: `vector_fig.pdf` で生成画像の拡張子・サイズ（JPEG 無劣化）と、`UnmatchedCaption` 減少を確認。findings に記録。

---

### P4-4: caption 正規表現の拡張

**目的:** キャプション検出を `FIG.`/`Scheme N`/`Chart N`/`Figure S1`（supplementary）/日本語「図1」「表1」など
多様な表記に広げ、大小文字・空白ゆれを許容し、パターンを設定可能にする。

**重大度 / 依存:** MED（提案書 I6） / なし（他タスクに先行して着手可）。

**対象ファイル:**
- `crates/pdf-lay-core/src/figure/caption_detector.rs:52-67`（`CaptionDetector::new` の 2 本の狭い正規表現）
- `crates/pdf-lay-core/src/figure/caption_detector.rs:8-14`（`CaptionType`）
- `crates/pdf-lay-core/src/config.rs`（パターン設定の追加先）
- 連動: `crates/pdf-lay-core/src/figure/image_matcher.rs:61-64`（`CaptionType::Figure` のみマッチ）,
  `crates/pdf-lay-core/src/pipeline.rs:160-179`（caption 検出と Table 分岐）

**現状（問題）:**
```rust
// caption_detector.rs:55-64  行頭 + ASCII 数字 + 限定接頭辞のみ。狭い。
regex: Regex::new(r"(?i)^(Fig\.?|Figure)\s*(\d+)\s*[:.]?\s*(.*)")   // Figure
regex: Regex::new(r"(?i)^(Table|Tab\.)\s*(\d+)\s*[:.]?\s*(.*)")     // Table
```
- `FIG. 1a`（数字＋接尾字）、`Scheme 1`、`Chart 2`、`Figure S1`（supplementary の S）を取りこぼす。
- 日本語「図1」「表1」「図 1」を全く拾えない（ASCII 前提）。
- `\d+`（半角数字）のみ。全角「図１」や `S1` の混在番号に非対応。
- パターンがコード直書きで**設定不可**（方針書 §1-6 マジック相当）。

**変更後の期待動作:**
- 追加表記を検出し、`CaptionType` を `Figure` / `Table` に加えて `Scheme` / `Chart` を導入
  （または `Figure` に寄せるかは下記「実装手順1」で決める。**既定は種別を増やす**）。
- supplementary 番号（`S1`, `S12`）、接尾字（`1a`, `1b`）、全角数字「１」、日本語「図」「表」を許容。
- 大小文字・接頭辞と番号間の空白ゆれ（`Fig.1` / `Fig . 1` / `FIG　1`（全角空白））を許容。
- パターンは `Config` から**追加・上書き可能**（既定パターン＋ユーザ追加）。

**実装手順:**
1. `CaptionType`（`caption_detector.rs:8-14`）に `Scheme`, `Chart` を追加するか検討。
   下流 `ImageMatcher`（`image_matcher.rs:61-64`）は現在 `Figure` のみ画像マッチ、`Table` は無視。
   **`Scheme`/`Chart` は画像マッチ対象（Figure 相当）**として扱う（`match_all` のフィルタを
   `Figure | Scheme | Chart` に拡張）。`Table` は従来どおりテーブル経路。
2. 既定正規表現を以下の**新セット**に置換（`(?i)` 大小無視、`[ \t\u{3000}]*` で半角/全角空白許容、
   番号は `S?` supplementary＋半角/全角数字＋任意接尾字 1〜2 文字）:

   - **Figure（英）:** `(?i)^(fig(?:ure)?|fig\.)[ \t\u{3000}]*(s?\d+[a-z]?)[ \t\u{3000}]*[:.\-–]?[ \t\u{3000}]*(.*)`
   - **Table（英）:** `(?i)^(tab(?:le)?|tab\.)[ \t\u{3000}]*(s?\d+[a-z]?)[ \t\u{3000}]*[:.\-–]?[ \t\u{3000}]*(.*)`
   - **Scheme（英）:** `(?i)^(scheme)[ \t\u{3000}]*(s?\d+[a-z]?)[ \t\u{3000}]*[:.\-–]?[ \t\u{3000}]*(.*)`
   - **Chart（英）:** `(?i)^(chart)[ \t\u{3000}]*(s?\d+[a-z]?)[ \t\u{3000}]*[:.\-–]?[ \t\u{3000}]*(.*)`
   - **図（日）:** `^(図)[ \t\u{3000}]*([0-9０-９]+)[ \t\u{3000}]*[:：.、．\-–]?[ \t\u{3000}]*(.*)`
   - **表（日）:** `^(表)[ \t\u{3000}]*([0-9０-９]+)[ \t\u{3000}]*[:：.、．\-–]?[ \t\u{3000}]*(.*)`

   > 番号キャプチャ（group 2）は文字列で受け、`number: Option<u32>` へは
   > **全角→半角正規化後に先頭連続数字をパース**（`S1`/`1a` は `1`、全角「１」は `1`）。
   > supplementary の `S` 有無は `prefix`/`full_text` に保持（番号衝突を避けるなら別途 `is_supplementary` を検討、
   > ただし型追加は最小限に。既定は `full_text` 保持で足りる）。
3. 番号パースヘルパを追加（全角数字正規化 + 先頭数字抽出）。`number.parse()` 直呼び（現状
   `caption_detector.rs:88`）は `S1`/全角で失敗するため置換。**No Silent Drop:** 番号が取れなくても
   caption 自体は検出する（`number: None` で通す。現状も `.parse().ok()` で None 許容だが、
   全角/接尾字ケースを取りこぼさないこと）。
4. **設定化:** `Config`（`config.rs`）に `#[serde(default)] pub caption: CaptionConfig` を追加。
   `CaptionConfig { extra_figure_patterns: Vec<String>, extra_table_patterns: Vec<String>,
   enable_japanese: bool (既定 true), enable_scheme_chart: bool (既定 true) }`（全フィールド `#[serde(default)]`）。
   `CaptionDetector::new` を `CaptionDetector::from_config(&CaptionConfig)` へ拡張（`new` は既定設定で残す）。
   ユーザ追加パターンのコンパイル失敗は**パニックせず**、warning を出して当該パターンを無視
   （方針書 §1-5 パニック禁止。現状は `.expect(...)` だが、ユーザ入力パターンには使わない）。
5. `pipeline.rs` の caption 利用箇所（`160-179`）と `ImageMatcher` を新 `CaptionType` に追随させる
   （Table 分岐 `pipeline.rs:187-190` は `Table` のみ、図マッチは `Figure|Scheme|Chart`）。

**シグネチャ変更 / 新フラグ・feature:**
- `CaptionType` に `Scheme`, `Chart` バリアント追加（`caption_detector.rs:8`）。
- `CaptionDetector::from_config(&CaptionConfig)` 追加（`new()` は維持）。
- `Config.caption: CaptionConfig` 追加（`#[serde(default)]`）。
- `ImageMatcher::match_all` のフィルタ拡張（`Figure|Scheme|Chart`）。
- 新 CLI フラグは原則不要（設定ファイル/デフォルトで足りる）。必要なら `--no-japanese-captions` 等を検討（任意）。

**後方互換:**
既定でパターンが**広がる**ため、従来 caption 化しなかった行が caption 化されうる（提案書 §5 の許容範囲：rc段階）。
`enable_japanese` / `enable_scheme_chart` を既定 on にしつつ、`false` で旧挙動に近づけられる。
`CaptionType` 追加はライブラリ内部型だが、`serde` 直列化される（`FigureInfo` 等経由か要確認）ため、
JSON に露出する場合は既存の値を壊さない（新バリアント追加のみ）。`new()` を残すので既存呼び出しは不変。

**受け入れ基準（Given/When/Then）:**
- Given `"FIG. 1a Overview"`、When `detect`、Then Figure として検出され `number == Some(1)`。
- Given `"Scheme 2: Synthesis route"`、When `detect`、Then `CaptionType::Scheme` で検出され画像マッチ対象になる。
- Given `"Figure S1. Supplementary data"`、When `detect`、Then Figure として検出され `full_text` に `S1` を保持。
- Given `"図1 提案手法の概要"` / `"表 2：性能比較"`、When `detect`（`enable_japanese=true`）、Then
  それぞれ Figure/Table として検出される。
- Given `"This table shows ..."`（本文）、When `detect`、Then **検出されない**（行頭アンカー維持で過剰マッチを防ぐ）。
- Given ユーザ追加パターンが不正正規表現、When `from_config`、Then パニックせず warning を出し他パターンで継続。

**追加テスト:**
- ユニット（`caption_detector.rs` の `#[cfg(test)]` に追加、既存 `126-182` を踏襲）:
  `FIG. 1a` / `Scheme 1` / `Chart 3` / `Figure S1` / `Fig.1`（空白無し）/ `FIG　1`（全角空白）/
  「図1」「図 1」「表２」（全角数字）/ 本文非検出（`"Table 1 shows..."` は行頭だが説明文が続くケースの扱いを
  明記：行頭一致は許容しつつ、**過剰マッチは Phase 0 P0-5 の drop 厳格化と連携**するので本タスクでは
  「検出はする」に留め、本文脱落は起こさない）。
- 番号正規化: `S1`→`Some(1)`、`1a`→`Some(1)`、`１`（全角）→`Some(1)`、番号無し→`None` でも caption 検出。
- ゴールデン（任意, `#[ignore]`）: 日本語/化学系 PDF で `UnmatchedCaption` が減ること。

**非スコープ:**
キャプション-画像マッチングのアルゴリズム改良（`image_matcher.rs` の距離スコア。ただしフィルタ拡張は本タスク）、
本文からの図参照（"as shown in Fig. 3"）解決、caption の drop 厳格化（Phase 0 P0-5）。

**検証:**
- `cargo test -p pdf-lay-core`（新ユニットテスト全緑）、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo fmt --check`。
- 3面同期（`CaptionType`/`Config` 変更が JSON/Python に波及する場合）: `cargo run -p pdf-lay-cli -- toc <日本語pdf>`、
  `uvx maturin develop ...` が通ること。
- 手動: 日本語・化学系 PDF で「図N」「Scheme N」が図として拾えることを目視し findings に記録。

---

## フェーズ完了の定義

本フェーズは、各タスクが方針書 §4「Definition of Done」を満たしたうえで、**さらに**以下を満たしたとき完了とする。

1. **調査ゲートの成果物が存在する。** `docs/refactor/phase4_findings.md` に、P4-1 の CJK/縦書き/回転/スキャン
   観測結果、P4-2 の OCR 方式決定と閾値根拠、P4-3 の inline/Form/SMask/JPX 観測、P4-4 の日本語 caption 確認が
   **実 PDF の観測付きで**記録されている。「未検証（フィクスチャ未整備）」の項目はそう明記されている。
2. **No Silent Drop の穴が塞がれている。** テキスト/画像/図を破棄しうる新旧経路（degenerate span drop、
   per-page/per-image skip、bbox 欠損、未マッチ caption、SMask 無視、OCR 未回復）は、
   最近傍への割当か `AnalysisResult.warnings` への記録のいずれかで捕捉される（無言 `continue`/`filter` を新設しない）。
3. **オプション性が守られている。** OCR（P4-2）・ベクター図/形式変更の回帰（P4-3 の `force_png`）・
   日本語/Scheme caption（P4-4）は、既定挙動を大きく変えない or フラグ/`#[serde(default)]` 設定で旧挙動へ戻せる。
   OCR は既定 off。重い必須依存（`ort`/モデル）を無条件に増やしていない。
4. **エスカレーションが誠実に行われている。** pdf_oxide 側限界（縦書きレイアウト、SMask、JPX、CID デコード不能等）に
   突き当たったタスクは、推測実装で埋めず、findings と PR「レビュアーへの質問」に選択肢（upgrade/代替/OCR）と
   暫定案を提示して人間判断を仰いでいる（方針書 §7）。
5. **公開 API の 3面同期。** `ImageInfo`/`CaptionType`/`Config`/`PdfLayWarning` の変更が
   CLI・Python（`pdflay-python`）・`pdf-lay` crate に反映され、`cargo test --workspace` と
   `maturin develop` が緑。
6. **回帰指標を悪化させていない。** 方針書 §8.3 の指標（テキスト網羅率、キャプション-画像マッチ率）を
   本フェーズの変更が悪化させないことを、X-1 の実 PDF フィクスチャ（`ja_a4.pdf` / `scanned.pdf` / `vector_fig.pdf`）で
   確認している（フィクスチャ未整備分は `#[ignore]` と明記）。
