# Phase 4 — アウトカムノート（観測記録）

**位置づけ:** `docs/refactor/00_REVIEW_POLICY.md` §7・`docs/refactor/phase4_extraction.md` §0.3 に従う観測記録。
各タスク（P4-1〜P4-4）はこのノートの該当セクションを埋めてから実装に入る。

このノートは **実行して確認した事実のみ** を「観測」として記録する。未検証の推測は明示的に
「未検証」と書く（方針書 §6「推測実装」禁止に対応）。

---

## P4-1: pdf_oxide の CJK / 縦書き / スキャン挙動の検証

**実施日:** 2026-07-06
**担当範囲:** 調査のみ。プロダクションコード変更はゼロ（`convert_span` の degenerate-bbox drop は
本タスクでは変更していない — 理由は下記§5参照）。追加したのは `crates/pdf-lay-core` 内のテストのみ。

### 0. 使用したフィクスチャ

方針書 §8.2 / 本設計書 §0.1 が要求する実 PDF フィクスチャ（`ja_a4.pdf` / `ja_vertical.pdf` /
`scanned.pdf` / `vector_fig.pdf`）は **リポジトリに存在しない**（`tests/fixtures/` には
`README.md` のみ。横断タスク X-1 が未着手）。

> **未検証（フィクスチャ未整備）:** 実日本語論文・実スキャンPDF・実縦書きPDFでの挙動は
> このノートでは確認できていない。下記 §4「実フィクスチャが必要な項目」に、確認済みの合成PDFでの
> 観測結果と、実フィクスチャで確認すべき残作業を分けて記載する。

代わりに、`crates/pdf-lay-core/src/extract/pdf_reader.rs` の `#[cfg(test)] mod tests` に
xref 正しい最小PDFを手で組み立てるテスト用ビルダー（`TestPdfBuilder`、既存の `build_minimal_pdf`
と同じ手法）を追加し、以下の5種類の合成PDFで **pdf_oxide 0.3.8（ワークスペースが実際に固定している
バージョン。`Cargo.lock` で確認済み）を直接・および pdf-lay の `PdfReader` 経由で** 観測した:

| 合成PDF | 内容 | 目的 |
|---|---|---|
| text_sanity | 標準14フォント(Helvetica) + `Tj` | ハーネス自体の健全性確認 |
| image_only | テキスト演算子なし、Image XObject 1枚（`Do`経由） | スキャンページの最小形 |
| cjk_tounicode (Identity-H) | CIDコード`<0001><0002>` + 埋め込みフォントなし + ToUnicode CMap（`0001→U+65E5`,`0002→U+672C`） | ToUnicodeのみでのCJKデコード可否 |
| cjk_tounicode (Identity-V) | 上と同じだが `/Encoding /Identity-V` | 縦書き指定時のデコード・bbox形状 |
| rotated_text | `/Rotate 90` + 通常テキスト | ページ回転とspan bboxの整合性 |

**実CJKフォントの埋め込みは行っていない**（設計書の指示どおり、埋め込みフォント無しで検証可能な
ToUnicode CMap 経路のみを試した）。実際のグリフ描画・実フォントのCIDToGIDMap経路は未検証。

再現コマンド:
```bash
cargo test -p pdf-lay-core extract::pdf_reader::tests:: -- --nocapture
```
（`--ignored` 無しで全て実行される。追加した観測用テストは通常のCIテストとして緑になっている。）

### 1. 能力表の再検証（登録ソース `pdf_oxide-0.3.8` を実際に grep/読解）

設計書の能力表（`phase4_extraction.md` §「pdf_oxide 0.3.8 能力調査」）を、
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/pdf_oxide-0.3.8/src/` を実際に読んで
裏取りした結果:

| 項目 | 設計書の主張 | 再検証結果 |
|---|---|---|
| CID/Type0, `cid_to_gid_map`/`cid_system_info` | 対応。`fonts/font_dict.rs:22-73` | **確認**。`cid_to_gid_map: Option<CIDToGIDMap>` は `font_dict.rs:51`、`cid_system_info: Option<CIDSystemInfo>` は `font_dict.rs:54`。 |
| ToUnicode CMap（`LazyCMap`, bfchar/bfrange） | 対応。`fonts/cmap.rs:1-460` | **確認 + 実証**。`LazyCMap` 構造体は `cmap.rs:210`、`impl LazyCMap` は `cmap.rs:223`。**合成PDFで実際にToUnicodeのみ（埋め込みフォント無し）から `"日本"` が正しく decode されることを確認**（後述 §2.1）。 |
| Identity-H/V | 対応（`font_dict.rs:1139,1782-1804` 等） | **確認**。`"Identity-H" | "Identity-V" => Ok((Encoding::Identity, empty_map))` が `font_dict.rs:1139`。 |
| `layout::TextSpan` に回転/writing-mode フィールドが無い | 主張どおり | **確認**。`src/layout/text_block.rs:21-79` の `TextSpan` フィールドは `text, bbox, font_name, font_size, font_weight, is_italic, color, mcid, sequence, split_boundary_before, offset_semantic, char_spacing, word_spacing, horizontal_scaling, primary_detected` のみ。回転・writing-mode・生CID値は無い。 |
| inline画像（BI/ID/EI） | 対応。`content/operators.rs:414` 等 | **確認**。`Operator::InlineImage` は `content/operators.rs:414`。`document.rs:5439` の `extract_images` が `Operator::InlineImage { dict, data }` を処理し `extract_image_from_inline`（`document.rs:5791`）を呼ぶ。 |
| Form XObject 再帰 | 対応 | **確認**。`document.rs:5439` の doc コメントに「Recursively processes Form XObjects」、`extract_images_from_xobject_do` が実装（`document.rs:5535`〜）。 |
| CMYK→RGB、SMask 非対応、JPX 非対応 | 主張どおり | **確認**。`cmyk_to_rgb`（`extractors/images.rs:819`）実装済み。`SMask`/`smask` は `extractors/images.rs` に一切出現せず（`compliance/` と `writer/` にのみ出現）。`decoders/` ディレクトリに `ascii85, ascii_hex, brotli, ccitt, dct, flate, jbig2, lzw, predictor, runlength` はあるが jpx/jpeg2000 は無い。 |
| `ocr` feature・`needs_ocr`・`extract_spans_with_ocr` | 内蔵。`document.rs:2510-2560`, `ocr/mod.rs:48-56` | **一部訂正**。関数の実体は `ocr/mod.rs:86`(`needs_ocr`)・`document.rs:2510`(`extract_text_with_ocr`)・`document.rs:2536`(`extract_spans_with_ocr`) で、設計書の行番号 `ocr/mod.rs:48-56` は実装ではなく re-export ブロックのコメント行を指していた（軽微な誤差）。より重要な訂正: **`pub mod ocr;` 自体が `#[cfg(feature = "ocr")]` でゲートされている**（`src/lib.rs:197-199`）。**pdf-lay のワークスペース `Cargo.toml`（`pdf_oxide = "0.3.8"`, features指定なし, `pdf_oxide` 側 `default = []`）では `ocr` featureが有効化されていないため、`pdf_oxide::ocr` モジュール自体が現時点のビルドに存在しない。** P4-2 は新規 cargo feature を追加する前提だが、これは「機能が既にあるので呼ぶだけ」ではなく「featureを新たに有効化する」変更であることを明記する。 |
| `get_page_info`/`PageInfo`（前文で既出の前例） | `rendering` feature 依存と既に本文に記載 | **確認（前例どおり）**。`PageInfo` 定義（`document.rs:29-36`）にも `get_page_info`（`document.rs:4501`）にも `#[cfg(feature = "rendering")]` が付く。pdf-lay は `rendering` feature を有効化していないため使用不可 — これは既に `pdf_reader.rs` 側が `get_page_for_debug`（feature非依存の内部公開API, `document.rs:5957` 付近）を使うことで正しく回避済み（P0-1 実装済みで確認）。 |

**新規に発見した点（設計書に無かった情報）:**

- `PdfDocument::extract_chars(page) -> Result<Vec<TextChar>>`（`document.rs:3604`）という
  低レベルAPIが存在し、返す `TextChar`（`layout/text_block.rs` 内、`TextSpan` とは別型）は
  **`rotation_degrees: f32`（`text_block.rs` 内、テキスト変換行列から `atan2` で算出）を持つ**。
  つまり **文字単位でなら pdf_oxide は回転角を露出できる**（`TextSpan` には無いが `TextChar` には
  ある）。ただし doc コメントに「Characters are sorted in reading order (top-to-bottom,
  left-to-right)」とあり、**そもそも水平読み順を前提にソートされる** ため、縦書き文書の読み順を
  そのまま解決してはくれない。かつ §2.2 で示すとおり、**CJK（CID/Identity-H）テキストに対して
  `extract_chars` は文字化け（サロゲート半端値による `U+FFFD` の混入で文字数が実際の2倍になる）
  を起こす**ことを実測した。**`extract_chars` は現状 pdf-lay からは全く使われておらず
  （`grep -rn extract_chars crates/` はヒット無し）、今後 P4-2/P4-3 で「回転角が欲しいから
  `extract_chars` を使う」という設計判断をする場合は、この文字化けが解消されているか
  pdf_oxide 側の追加検証が要ることを明記する。**

### 2. 実測結果（合成PDF、pdf-lay の `PdfReader` 経由）

対応するテスト（`crates/pdf-lay-core/src/extract/pdf_reader.rs`、いずれも通常テストとして緑）:
`text_sanity_pdf_extracts_via_pdf_lay_wrapper`,
`cjk_tounicode_only_text_decodes_through_pdf_lay_wrapper`,
`identity_v_decodes_text_but_bbox_matches_identity_h_shape`,
`image_only_page_yields_no_spans_and_no_panic`,
`image_only_page_image_is_still_extracted`,
`rotated_page_span_bbox_is_not_adjusted_for_rotate_entry`。

#### 2.1 CJK（横書き, Identity-H）デコード可否 — 事前調査項目1

- **観測:** ToUnicode CMapのみ（埋め込みフォントプログラム無し、`FontDescriptor` に
  `FontFile2` 無し）で `PdfReader::extract_text_spans(0)` を呼ぶと、span は1個、
  `text == "日本"` （U+65E5, U+672C）と正しくUnicodeで出た。PUA・`?`・`□` への化けは無い。
- **診断:** pdf_oxide側でToUnicode CMapのbfcharパースが正しく動作している（**pdf_oxide側は
  正常**）。埋め込みフォントが無くてもテキスト抽出（非レンダリング）には支障がない。

#### 2.2 pdf-lay 経由での損失（degenerate-bbox drop・per-page skip）— 事前調査項目2

- **観測:** 上記CJK合成PDFのbboxは `width=52.8pt, height=24pt`（両方とも正、`ow<=0||oh<=0` の
  drop 条件（`pdf_reader.rs` の `convert_span`）に該当しない）。今回作った全ての合成PDF
  （通常テキスト・CJK・Identity-V・回転テキスト）で **degenerate bbox によるdropは一度も
  再現しなかった**。
- **診断:** **今回の観測範囲では、pdf-lay 側の `convert_span` drop がテキスト損失の主因である
  証拠は得られなかった。** 設計書の受け入れ基準は「実在すると観測された場合のみ修正する」なので、
  **`convert_span` は変更していない**（変更すると根拠のない差分になり、方針書§6「推測実装」に抵触する）。
  ただし、これは「実日本語PDFで絶対に起きない」ことの証明ではない — 実フォントの Tw/Tc の相互作用や、
  結合文字・異体字セレクタ、フォントによっては pdf_oxide が返す bbox が本当に縮退するケースが
  実論文PDFに存在する可能性は残る（§4「要実フィクスチャ」参照）。

  合わせて `extract_all_text_spans()` の per-page skip（`log::warn!` のみで
  `AnalysisResult.warnings` には載らない、`pdf_reader.rs:167-169`）は、コードを読んで再確認した
  とおり **依然として `PdfLayWarning` に載っていない**（`pipeline.rs` は `reader.extract_all_text_spans()`
  を1回呼ぶだけで、内部の per-page Err を観測できない）。これは設計書の指示どおり「warning経路の
  確認に留め、修正はP4-2のスコープ」として扱った（コード変更していない）。

#### 2.3 縦書き（Identity-V）— 事前調査項目3

- **観測:** `/Encoding /Identity-V` にした同じCJK合成PDFで `extract_text_spans(0)` を呼ぶと:
  - **文字は正しくデコードされる**（`text == "日本"`, Identity-Hと同じ）。
  - **bboxは Identity-H と完全に同一の形状**（`width=52.8pt > height=24pt`、横長）。
    縦書きなら本来 width < height（縦長）になるはずだが、そうならない。
  - `h_spans[0].bbox.width() == v_spans[0].bbox.width()`、`height()` も同様に一致することを
    テストで確認済み（`identity_v_decodes_text_but_bbox_matches_identity_h_shape`）。
- **診断:** **pdf_oxide 0.3.8 側の限界。** 文字コード→Unicodeのデコードは正しいが、
  span の幾何形状（bbox・読み順）は水平書字と同じ扱いになっており、**縦書きレイアウトは
  pdf_oxide が全く反映していない**。`TextSpan` に回転/writing-modeフィールドが無いことと
  合わせて、**pdf-lay側がbbox幾何だけから縦書きを検出・補正する現実的な手がかりが無い**
  ことを実測で裏付けた（設計書の懸念が正しかったことの実証）。
  → §5「エスカレーション」参照。

#### 2.4 回転ページ — 事前調査項目4

- **観測:** `/Rotate 90` を持つページで:
  - `PdfReader::page_media_box(0)` は `(width=792, height=612, rotation=90)` を返す
    （P0-1 の仕様どおり width/height を正しくswap）。
  - 一方 `extract_text_spans(0)` が返す span の bbox は、**回転前の生座標系のまま**
    （`Td 72 700` の内容ストロークそのままなので `bbox.top ≈ 712pt`）。
  - `712pt > 612pt`（回転後ページ高さ）なので、**span が「回転後ページ」の範囲外にはみ出す**
    座標になる。
- **診断:** **pdf_oxide 側の仕様（span座標は回転非適用）と、pdf-lay 側の
  `page_media_box`（回転適用済み寸法）との間に座標系の不整合がある。** これは pdf_oxide の
  「限界」というより「pdf_oxideは回転前座標を返す」という**仕様**であり、
  **pdf-lay 側が呼び出し側でspan座標に /Rotate を適用する変換を追加していない、という
  pdf-lay側の未実装**に分類するのが正確（pdf_oxide が壊れているのではなく、
  pdf-layが「ページ寸法だけ回転させて座標は回転させていない」半端な状態）。
  実際の /Rotate 付き論文PDFが手元に無いため、**この不整合が実文書でどれだけ深刻か
  （/Rotate を使う論文PDFがどれだけあるか）は未検証**。ただし挙動そのものは合成PDFで
  再現・固定化した（`rotated_page_span_bbox_is_not_adjusted_for_rotate_entry`）。
  → 本タスクの非スコープ（縦書き/回転レイアウト補正の本実装）だが、**別タスク化を推奨**
  （§5参照）。

#### 2.5 スキャン（画像onlyページ）— 事前調査項目5

- **観測:** テキスト演算子ゼロ、Image XObject 1枚（`Do`経由、DeviceGray, 2×2, フィルタ無し）
  のページで:
  - `PdfReader::extract_text_spans(0)` → `Ok(vec![])`（**パニックしない、エラーにもならない**）。
  - `reader.inner_doc().extract_images(0)` → `Ok(vec![1 image])`（画像自体はちゃんと取れる）。
  - フルパイプライン `analyze_pdf_bytes(..)` を通しても **パニックしない**。結果は
    `document.metadata.pages == 1`、`sections.len() == 1`（`SectionBuilder` が空でも
    「最終セクション」を1つ必ず作るため；ヘッダー無し・ブロック無しの空セクション）、
    `coverage.extracted_chars == 0`、`coverage.emitted_chars == 0`。
  - **重要な発見:** `pipeline.rs` のカバレッジ計算 (`let ratio = if extracted_chars == 0 { 1.0 }
    else {...}`) により、**ページ全体でテキストが0文字のとき、`ratio` は「1.0（完全網羅）」
    として扱われ、`LowCoverage` warning は一切出ない**。つまり **現状、画像onlyのスキャン文書
    全体を解析しても `AnalysisResult.warnings` は空になりうる**（No Silent Drop の観点で
    穴になっている）。これは `Coverage::ratio` のdocコメント（「1.0 when nothing was
    extracted」）が明記する **意図的な仕様**（0除算回避）であり、バグではなく設計選択だが、
    「テキストが一切取れなかった」という事実そのものを警告として利用者に伝える経路が今は無い。
- **診断:** **スキャンページの検出そのものは pdf_oxide 側で問題なく可能**（`extract_text` /
  `extract_spans` が空を返す、`extract_images` は画像を返す — pdf_oxide `ocr::needs_ocr`
  相当のロジック「テキストが50文字未満 かつ 画像がある」は自前でも再現可能）。
  **損失の所在は「pdf-lay 側」**: OCR実装が無いこと自体はP4-2のスコープだが、
  「ゼロテキスト文書で警告が一切出ない」ことは No Silent Drop の観点で **P4-2 が対処すべき
  既存ギャップ**として明記する（P4-1では修正しない。理由: 修正には
  `PdfLayWarning` の新設バリアント設計・閾値の`Config`化が必要で、これはP4-2の
  「需要判定・OCR対象ページ列挙」と表裏一体のため、P4-2と統合して実装した方が
  二重実装を避けられる）。

### 3. 診断デシジョンツリーによる分類まとめ

| 損失パターン | 生出力(pdf_oxide)の時点 | 診断 | 対応フェーズ |
|---|---|---|---|
| CJK ToUnicodeのみ | 正しくデコード | 問題なし | — |
| 縦書き(Identity-V)のbbox形状 | 文字は正、形状は水平のまま | **pdf_oxide側の限界**（TextSpanに回転/writing-mode無し、bboxが転置されない） | エスカレーション（本ノート§5）。実装するなら別タスク |
| 回転ページのspan座標 | pdf_oxideは回転前座標を返す（仕様） | **pdf-lay側の未実装**（page_media_boxは回転後、spanは回転前で不整合） | 別タスク提案（P4-1のスコープ外、§5） |
| 画像onlyページのテキスト0件 | 空(正常) | **pdf-lay側**（OCR無し=仕様、ただし警告が出ない点はNo Silent Drop違反） | P4-2 |
| degenerate bbox drop (`convert_span`) | (未再現) | **今回のケースでは非該当**（実CJK文書で再現されるまで保留） | 保留。実フィクスチャで再検証 |
| per-page 抽出エラー時のwarning欠落 | (pdf_oxide側エラー時のみ発生) | **pdf-lay側**（`log::warn!`のみ、`AnalysisResult.warnings`に不載） | P4-2（設計書の指示どおり） |

### 4. 実フィクスチャが必要な項目（未検証。合成PDFでは代替不可）

以下は **今回の合成PDFでは確認できず、実PDFでの確認が必須**。X-1でフィクスチャが揃い次第、
メンテナが以下のコマンドを実行して結果をこのノートに追記することを推奨する。

1. **実日本語論文のCJK抽出精度（フォント埋め込みあり、複数フォント混在）**
   ```bash
   cargo run -p pdf-lay-cli -- markdown tests/fixtures/ja_a4.pdf -o /tmp/ja.md
   ```
   確認観点: 日本語本文の文字化け有無、句読点・約物の抽出、フォントdictの`Encoding`が
   `Identity-H`以外（例: カスタムEncoding配列）のケースでの挙動。

2. **実スキャンPDFでのページ検出・警告**
   ```bash
   cargo run -p pdf-lay-cli -- toc tests/fixtures/scanned.pdf
   RUST_LOG=warn cargo run -p pdf-lay-cli -- markdown tests/fixtures/scanned.pdf -o /tmp/scan.md
   ```
   確認観点: 実際のスキャンPDF（JPEG/CCITT圧縮の全面画像 + わずかなOCR済みテキストレイヤー
   混在パターンを含む）で `extract_text`/`extract_spans` がどこまで空になるか。
   本ノート§2.5の「ratio=1.0でwarningが出ない」問題が実文書で実際にどれだけ利用者を
   混乱させるか（例えば10ページ中3ページだけスキャンの場合、全体ratioは薄まって見えなくなる
   可能性がある — この希釈効果は合成の単一ページPDFでは検証できない）。

3. **縦書きPDFでの読み順・段組検出**
   ```bash
   cargo run -p pdf-lay-cli -- markdown tests/fixtures/ja_vertical.pdf -o /tmp/vert.md
   ```
   確認観点: 本ノート§2.3で確認した「bboxが水平形状のまま」という限界が、実際の縦書き
   段組でどのような読み順の破綻を引き起こすか（例: 右→左の段送りが検出できずページ順が
   バラバラになる等）。`ColumnDetector`/`LineReconstructor` の挙動を実測する必要がある。

4. **実論文PDFでの `/Rotate` 使用有無**
   本ノート§2.4の座標不整合が実際に問題になるのは `/Rotate` を持つページが存在する場合のみ。
   IEEE/arXiv/日本語論文サンプルで `/Rotate` エントリを持つページがどの程度あるか
   （回転テーブルや横向き図表ページなど）を実フィクスチャで確認する必要がある。
   ```bash
   # ページごとの /Rotate を確認する簡易チェック（pdftk/qpdf 等の外部ツールでも可）
   cargo test -p pdf-lay-core extract::pdf_reader::tests::page_media_box_swaps_on_rotation -- --nocapture
   ```

5. **ベクター図・inline画像混在PDFでの `extract_images` 網羅性**（P4-3向け予備調査）
   ```bash
   cargo run -p pdf-lay-cli -- markdown tests/fixtures/vector_fig.pdf -o /tmp/fig.md
   ```

---

## §5 エスカレーション（方針書§7に基づく）

### 縦書き（Identity-V）のレイアウト非対応 — pdf_oxide 側限界

**事実:** `pdf_oxide::layout::TextSpan` は Identity-V を Identity-H と同一の水平形状で
返す（本ノート§2.3で実証）。回転/writing-modeフィールドも無い。低レベルの`extract_chars`は
回転角を持つが、CJKで文字化けする（本ノート§1「新規に発見した点」）ため現状使えない。

**選択肢:**
- **(A) pdf_oxideのバージョン更新** — 0.3.8以降で縦書きレイアウトサポートが改善されているか
  changelog/リリースノートの確認が必要（本タスクでは未確認・未実施）。
- **(B) pdf-lay側での幾何補正の自前実装** — `extract_chars`のCJK文字化けが解消されない限り、
  実用的な代替手段が無い。`TextSpan`のbboxのみからは縦書きの列順を復元する手がかりが
  無いことを実測した（bboxが水平形状のまま返るため、幅と高さの比較だけでは縦書きと判定できない）。
- **(C) OCRフォールバック（P4-2に接続）** — 縦書きページをOCR対象として扱う案。ただしOCRの
  レイアウト解析（PaddleOCR系）が縦書きに対応しているかは別途確認が必要。

**暫定案:** (A)のバージョン確認を最初に行うことを推奨する（コスト最小）。改善が無ければ、
縦書きレイアウト補正は「別タスク（P4-1のスコープ外）」として起票し、実`ja_vertical.pdf`
フィクスチャ入手後に着手判断する。**本タスクでは実装しない。**

### 回転ページの座標不整合 — pdf-lay側の未実装（要別タスク化）

**事実:** `page_media_box`は回転後寸法を返すが、`extract_spans`のbboxは回転前座標のまま
（本ノート§2.4）。

**推奨:** P4-1のスコープ外（縦書き/回転レイアウト補正の本実装は非スコープと明記されている）
だが、影響が軽微でない可能性があるため、**別タスクとして起票することを提案する**。
実装方針の候補: `page_media_box`が返す`rotation`を使い、`convert_span`のbbox変換時に
90/180/270度の座標変換を適用する（具体的な変換行列は本ノートでは未設計、実装タスクで検討）。
**本タスクでは実装しない。**

### スキャン文書で警告が出ない — P4-2で対処

本ノート§2.5の「`extracted_chars == 0` → `ratio = 1.0` → warningなし」は、
P4-2の「需要判定（OCR対象ページ）」実装と統合して直すことを推奨する
（新設する`PdfLayWarning::PageTextMissing`等をここでも使う設計にすれば二重実装を避けられる）。

---

## §6 P4-2 / P4-3 へのゲーティング推奨

1. **P4-2（OCR）:**
   - **`ocr` cargo feature は現在 pdf-lay 側で有効化されていない**（本ノート§1で確認）。
     設計書が示す (A)/(B) の二択のうち、(A) 内蔵OCR採用時は
     `pdf_oxide`依存に `features = ["ocr"]` を追加する必要があり、`ort`(ONNX Runtime)の
     追加ビルドコスト・配布コスト（CLIバイナリ肥大化、Python wheel、モデルファイル同梱）を
     必ず検証すること。設計書が推奨する(B) tesseractシェルアウトの方が安全側というのは
     本調査でも支持する（重い必須依存を増やさないという方針書§7の原則に合致）。
   - 「スキャン対象ページ」の判定は `extract_text`の文字数閾値 + `extract_images`の画像有無
     という組み合わせで実装可能（本ノートで動作確認済みの`extract_images`が空でないことを
     利用できる）。
   - **本ノート§2.5「ratio=1.0でwarningが出ない」ギャップをP4-2実装時に必ず塞ぐこと**
     （`PageTextMissing`導入と合わせて解消するのが最も自然）。
   - **GO**（着手可能）。ブロッカーなし。ただし上記のfeatureコスト検証は実装前に必須。

2. **P4-3（ベクター図・inline画像・Form XObject）:**
   - inline画像・Form XObject再帰は pdf_oxide側で実装されていることをソースで確認済み
     （本ノート§1）。実際にpdf-lay側の`ImageExtractor`がそれらを正しく受け取れているかは
     **実PDFで未検証**（本ノート§4項目5）。合成PDFでの`Do`経由XObject画像取得は確認済み
     （本ノート§2.5, `image_only_page_image_is_still_extracted`）ので、**単純なXObject画像
     経路は動作することが分かっている**。inline画像・Form再帰の合成PDFでの検証は本タスクの
     時間内では実施しなかった（P4-3着手時に同じ`TestPdfBuilder`パターンで追加検証することを推奨）。
   - SMask非対応・JPX非対応は設計書どおり確認済み（本ノート§1）。
   - **条件付きGO**: 基本のXObject画像経路はGOだが、inline/Form再帰の実PDF確認は
     P4-3の事前調査で改めて実施すること（本タスクでは基本経路のみ確認）。

3. **P4-4（caption正規表現）:** 既に別コミットで実装済み（`git log`確認: `1ccac47 feat(figure):
   broaden caption detection (Scheme/Chart/S-numbers/Japanese) (P4-4)`）。本ノートの対象外。

---

## 付録: 追加したテスト一覧

`crates/pdf-lay-core/src/extract/pdf_reader.rs`（`#[cfg(test)] mod tests`内、全て通常テストとして
実行され、`cargo test --workspace`で緑）:

- `text_sanity_pdf_extracts_via_pdf_lay_wrapper` — ハーネス健全性確認。
- `cjk_tounicode_only_text_decodes_through_pdf_lay_wrapper` — ToUnicodeのみでCJKデコード確認（§2.1）。
- `identity_v_decodes_text_but_bbox_matches_identity_h_shape` — Identity-Vのbbox非転置を固定化（§2.3）。
- `image_only_page_yields_no_spans_and_no_panic` — スキャンページ最小形でパニックしないことの保証（§2.5）。
- `image_only_page_image_is_still_extracted` — 同ページの画像自体は取れることの確認（§2.5）。
- `rotated_page_span_bbox_is_not_adjusted_for_rotate_entry` — 回転ページの座標不整合を固定化（§2.4）。

`crates/pdf-lay-core/src/pipeline.rs`（`#[cfg(test)] mod tests`内）:

- `analyze_pdf_image_only_page_does_not_panic_and_yields_empty_document` — フルパイプラインで
  スキャンページ相当の入力がパニックしないこと、および現状warningが出ないギャップ（§2.5）を
  固定化する回帰テスト。

いずれも `#[ignore]` を付けていない（合成データのみに依存し、外部フィクスチャを必要としないため、
通常の `cargo test --workspace` で常時実行される）。

## 検証結果

```
cargo fmt --all --check         => OK
cargo clippy --workspace --all-targets --all-features -- -D warnings   => OK（警告0）
cargo test --workspace          => 405 passed; 0 failed; 15 ignored（全バイナリ合計。
                                    pdf-lay-core lib: 348 passed, 3 ignored — 本タスクで
                                    追加した7テスト（下記付録）を含み、全て非ignoredで緑）
```
