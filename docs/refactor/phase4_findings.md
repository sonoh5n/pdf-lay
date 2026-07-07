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

## P4-3: ベクター図・inline画像・Form XObject の抽出 + 保存形式

**実施日:** 2026-07-06
**担当範囲:** 実装 + 着手時の合成PDF裏取り（P4-1 §6「条件付きGO」で指示されたとおり、
inline画像・Form XObject再帰は本タスクの事前調査として`TestPdfBuilder`で改めて検証した）。

### 1. 着手時の合成PDF裏取り（事前調査、実装前に実施）

`crates/pdf-lay-core/src/extract/pdf_reader.rs` の `#[cfg(test)] mod tests` に追加した
合成PDFで、pdf_oxide 0.3.8 の `PdfDocument::extract_images` を直接呼び、以下を確認した
（対応するテストは `dct_filtered_xobject_image_is_tagged_as_jpeg_source`,
`spec_conformant_inline_image_is_silently_not_extracted`,
`inline_image_is_extracted_only_with_a_non_conformant_subtype_key`,
`image_inside_form_xobject_is_extracted_recursively_by_pdf_oxide`,
`image_xobject_hints_detects_smask`,
`jpx_filtered_image_is_silently_absent_from_extract_images_with_no_error`,
`image_xobject_hints_detects_unsupported_jpx_filter`）:

#### 1.1 Form XObject 再帰 — **確認: 動作する**

Form XObject（`/Subtype /Form`）自身の `/Resources` に宣言された Image XObject を、
ページの `Do` → Form の `Do`（2段のネスト）を経て正しく再帰抽出することを実測した。
P4-1は`document.rs`のソースコメント（"Recursively processes Form XObjects"）を読んで
「対応」と判定していたが、本タスクで実際に合成PDFを通して**実証**した。

#### 1.2 inline画像（BI/ID/EI）— **重要な訂正: P4-1の「対応」判定は不正確**

P4-1は「`Operator::InlineImage`という列挙子がある」ことをソースで確認し「対応」と
記録したが、**実際に合成PDFを通すと、PDF仕様どおりの（`/Subtype`キーを持たない）
inline画像は抽出されない**ことが判明した。

- **根本原因:** `extract_image_from_inline`（`document.rs:5791`）は
  `expand_inline_image_dict`で略記キー（`W`/`H`/`CS`/`BPC`等）を展開した後、
  **XObject用の関数 `extract_image_from_xobject` をそのまま再利用**している。
  この関数は `dict.get("Subtype").and_then(|obj| obj.as_name()).ok_or_else(...)?` で
  **`/Subtype`キーの存在を必須としている**（`extractors/images.rs`、"XObject missing
  /Subtype" エラー）。
- **PDF仕様上、inline画像の辞書に`/Subtype`キーは存在しない**（ISO 32000-1:2008 §8.9.7。
  `BI`演算子自体が「これは画像である」ことを示すため、`/Subtype`はXObject辞書の語彙であり
  inline画像の辞書には現れない）。実際の合成PDFで確認: `/Subtype`キーを**持たない**
  仕様準拠のinline画像 → `extract_images`は**空のVecを返す**（エラーにもならない）。
  同じ画像に**非準拠**の`/Subtype /Image`キーを追加すると初めて抽出に成功する。
- **サイレントな喪失:** `extract_images`のinline画像処理ループは
  `if let Ok(image) = self.extract_image_from_inline(...) { images.push(image); }`
  （`document.rs:5527`付近）で、`Err`を握りつぶして継続する。つまり**ごく普通の
  inline画像を含むページでも、warningはおろかエラーにすらならず、その画像だけが
  無言で消える**（本ノート冒頭からの一貫した「pdf_oxideはper-image失敗を一切
  シグナルしない」というパターンと同型）。
- **結論:** P4-1の能力表「inline画像: 対応」は、ソースの型定義（`Operator::InlineImage`
  という列挙子の存在）は真だが、**実際にend-to-endで動くかは別問題**であり、
  実際には**動かない**（少なくとも0.3.8・仕様準拠のinline画像に対しては）。
  設計書§7「エスカレーション」の方針どおり、**本タスクではpdf-lay側でinline画像用の
  独自content-streamパーサを書くことはしない**（禁止されている「手組みパーサ」に
  該当するため）。inline画像のサポートは**据え置き**とし、pdf_oxide側の修正
  （`extract_image_from_inline`が`extract_image_from_xobject`を呼ぶ前に
  `Subtype: Name("Image")`を辞書へ補完で挿入する、など）を待つのが妥当な選択肢として
  記録する。

#### 1.3 SMask — **確認: pdf_oxideは非適用、辞書には現れる**

`PdfImage`（返り値）自体にはSMaskの情報は一切現れない（P4-1の確認どおり）。しかし
Image XObjectの**辞書**（`/Resources/XObject/<name>`）には`/SMask`キーがそのまま
残っているため、`pdf-lay`側で辞書を直接読めば**存在の検出は可能**であることを実証した
（本タスクで追加した`PdfReader::image_xobject_hints`で実装、下記§2参照）。

#### 1.4 JPX — **確認: エラーにならず、無言でVecから消える**

`/Filter /JPXDecode` のImage XObjectを含むページで`extract_images`を呼ぶと、
**`Ok(vec![])`（画像0件、エラーなし）**が返ることを実測した。ページに全く画像が
無い場合と**区別がつかない**。これはJPX特有の話ではなく、`extract_images`の
Do処理ループ全体が`if let Ok(mut xobj_images) = self.extract_images_from_xobject_do(...)
{ images.append(...) }`という形（`document.rs:5513`付近）で個別XObjectの失敗を
握りつぶす構造になっているためで、**「pdf_oxideのextract_imagesはエラーを一切
返さない per-image 失敗機構を持つ」という、より一般的な事実の一例**である。

### 2. 実装したこと

1. **per-image回復:** `image_extractor.rs`の`extract_page_images`を、1画像ずつ
   `save_one_image`（新設の非公開メソッド）で保存し、失敗した画像だけ
   `PdfLayWarning::ImageDecodeFailed`を出してスキップ、他の画像は継続して保存する
   ように変更（旧: `?`で1画像の保存失敗がページ全体の抽出失敗になっていた）。
   ユニットテストで実際に「1枚だけ不正なピクセルバッファ→即座に失敗、隣の正常な
   画像は保存される」ことを固定化（`one_bad_image_does_not_prevent_saving_a_good_one`）。
2. **形式の尊重:** `raw_img.data()`が`ImageData::Jpeg`なら`.jpg`で`save_as_jpeg`
   （JPEGソースはバイト列そのまま書き込みなので無劣化）、`Raw`なら
   `Config.image_format`（`ImageOutputFormat::Png`/`Jpeg`、`config.rs`に既存も
   これまで未使用だったフィールド）に従って`.png`/`.jpg`で保存。新設
   `Config.force_png: bool`（既定false）で、trueの場合は常にPNGへ強制再エンコード
   （JPEGソースもデコードしてPNG化）する旧挙動に戻せる。`ImageInfo.format`を
   実フォーマットへ正しく設定。
3. **bbox欠損の扱い:** 原点プレースホルダ（`Rect::new(0,1,1,0)`）をやめ、
   `ImageInfo`に`bbox_known: bool`を追加（`#[serde(default = "default_bbox_known")]`で
   既定値`true`、後方互換）。
   bbox不明/退化時は`bbox_known=false`＋ゼロサイズのダミーRectとし、
   `PdfLayWarning::ImageBboxUnknown`を出す。`ImageMatcher`は`bbox_known=false`の
   画像をマッチング対象から除外（設計書の指示どおり「除外＋warning」に統一）。
   `CoordinateNormalizer::estimate`も`bbox_known=false`の画像をスケール推定の
   母集団から除外する（そうしないとゼロサイズのダミーRectがスケール推定を歪める、
   または「画像はあるがbboxが全部不明」なページで無意味な`CoordinateFallback`
   warningが出てしまうため）。
4. **SMask/JPX検出:** `PdfReader::image_xobject_hints`（新設、`pub(super)`）が
   ページの`/Resources/XObject`辞書を直接読み（`page_media_box`と同じ、
   レンダリング非依存の辞書アクセスパターン）、`/SMask`キーの有無と
   `/Filter /JPXDecode`の有無を検出する。`ImageExtractor`はこれを使って
   `PdfLayWarning::ImageSMaskIgnored`・`PdfLayWarning::ImageDecodeFailed`を出す。
   **inline画像・Form XObject内の画像はこのスキャン対象外**（ページ直下の
   `/Resources/XObject`のみ走査。内容ストリームは一切解析しない — 「手組み
   パーサ禁止」を遵守するため）。
5. **ベクター図（最小スコープ）:** 新設 `figure::VectorFigureClusterer`
   （`crates/pdf-lay-core/src/figure/vector_figure.rs`）。ラスタ画像に
   マッチしなかったFigure/Scheme/Chart系キャプションについて、同ページの
   `extract_all_paths()`の`PathObject`をUnion-Findで空間クラスタリングし
   （`Config.figure_vector.cluster_gap_pt`以内なら結合）、`min_paths`以上の
   パスを持つクラスタがキャプション近傍（`caption_max_gap_pt`以内）にあれば、
   `image: None`（ラスタ実体なし）・`raw_bbox`/`normalized_bbox`にクラスタの
   領域bboxを持つ`FigureInfo`として記録する。ラスタ化（SVG/PNG描画）は非スコープ
   のまま。`ImageInfo.path`を`Option<PathBuf>`化（レンダラ側は
   `ImageInfo::filename()`が`None`を返すことで「ベクター図でリンクを捏造しない」
   処理に対応、Markdown/LLMテキスト/JSON全出力経路で確認）。

### 3. 非スコープ・据え置き（エスカレーション）

- **inline画像の抽出:** 上記§1.2のとおり、pdf_oxide 0.3.8はPDF仕様準拠の
  inline画像を抽出できない（`/Subtype`必須というXObject向けチェックの誤流用）。
  pdf-lay側でcontent-streamを手組みパースして代替実装することは方針書§6
  「手組みパーサ禁止」に抵触するため行わない。**エスカレーション:** pdf_oxideの
  修正（バージョンアップまたはIssue報告）を待つのが妥当。実際の学術論文PDFで
  inline画像（BI/ID/EI）がどの程度使われるか（多くの生成ツールはXObject画像を
  使うため、影響は限定的である可能性がある）は本タスクでは未検証。
- **ベクター図のラスタ化:** 設計書どおり非スコープ。`rendering` feature依存が
  重いため（P4-1の`get_page_info`調査と同じ理由）。
- **SMaskの合成適用・JPXデコード・色管理:** 設計書どおり非スコープ（検出＋warning
  のみ）。
- **`vector_fig.pdf`実フィクスチャでの`UnmatchedCaption`減少の実測:** 横断タスク
  X-1のフィクスチャが未整備のため、実PDFでの効果測定はできていない
  （`VectorFigureClusterer`のクラスタリング判定自体は合成`PathObject`で
  ユニットテスト済み）。

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

## 付録2: P4-3で追加したテスト一覧

`crates/pdf-lay-core/src/extract/pdf_reader.rs`（`#[cfg(test)] mod tests`、合成PDF、
すべて非ignoredで緑）:

- `dct_filtered_xobject_image_is_tagged_as_jpeg_source` — `/Filter /DCTDecode`の
  Image XObjectが`ImageData::Jpeg`としてタグ付けされることを確認（§1）。
- `dct_filtered_xobject_image_saves_as_jpg_through_image_extractor` — 上記が
  `ImageExtractor`経由で無劣化`.jpg`として保存されることをend-to-endで確認。
- `spec_conformant_inline_image_is_silently_not_extracted` — **重要な発見**の固定化
  （§1.2）: `/Subtype`キーを持たない仕様準拠のinline画像は抽出されない。
- `inline_image_is_extracted_only_with_a_non_conformant_subtype_key` — 上記の
  原因を`/Subtype`キーの有無で切り分ける対照テスト。
- `image_inside_form_xobject_is_extracted_recursively_by_pdf_oxide` — Form
  XObject再帰が実際に動作することの確認（§1.1）。
- `image_xobject_hints_detects_smask` — 新設`image_xobject_hints`がSMask
  エントリを検出することの確認。
- `image_xobject_hints_detects_unsupported_jpx_filter` /
  `jpx_filtered_image_is_silently_absent_from_extract_images_with_no_error` —
  JPXが`extract_images`からエラーなく無言で消えること、および`image_xobject_hints`
  がそれを検出できることの確認（§1.4）。
- `image_xobject_hints_is_all_false_for_a_plain_image` — 通常画像で誤検知しない
  ことの回帰ガード。

`crates/pdf-lay-core/src/extract/image_extractor.rs`（`save_one_image`を合成
`PdfImage`で直接叩くユニットテスト、PDFファイル不要）:

- `jpeg_source_passes_through_as_jpg_without_reencoding` /
  `force_png_reencodes_even_a_jpeg_source` — フォーマット尊重・`force_png`の
  往復動作。
- `raw_source_defaults_to_png` / `raw_source_honors_configured_jpeg_output_format` /
  `force_png_overrides_configured_jpeg_format` — `Config.image_format`が
  Rawソース画像にも適用されること。
- `known_bbox_is_converted_and_marked_known` /
  `missing_bbox_is_reported_as_unknown_not_fabricated` /
  `degenerate_bbox_is_reported_as_unknown` — bbox不明/退化時に原点プレースホルダを
  作らず`bbox_known=false`になること。
- `one_bad_image_does_not_prevent_saving_a_good_one` — per-image回復（1枚失敗しても
  隣の画像は保存される）。

`crates/pdf-lay-core/src/figure/vector_figure.rs`（新規モジュール、合成`PathObject`）:

- `nearby_paths_cluster_into_one_region` / `distant_paths_do_not_cluster` —
  Union-Findクラスタリングの近傍判定。
- `dense_cluster_near_unmatched_caption_becomes_a_vector_figure` /
  `sparse_cluster_below_min_paths_is_not_a_figure` /
  `caption_too_far_from_cluster_is_not_matched` /
  `each_cluster_matched_at_most_once` — キャプション紐付けの閾値・排他制御。

`crates/pdf-lay-core/src/figure/image_matcher.rs`:

- `image_with_unknown_bbox_is_not_matched` — `bbox_known=false`の画像がマッチング
  対象から除外されること。

`crates/pdf-lay-core/src/extract/coordinate.rs`:

- `unknown_bbox_images_are_ignored_not_treated_as_a_scale_measurement` —
  `bbox_known=false`の画像がスケール推定・`CoordinateFallback`誤検知を汚さないこと。

`crates/pdf-lay-core/src/config.rs`:

- `image_and_vector_figure_defaults` — `force_png`/`figure_vector`の既定値。

## 検証結果（P4-3）

```
cargo fmt --all --check                                          => OK
cargo clippy --workspace --all-targets -- -D warnings             => OK（警告0）
cargo test --workspace  => 全バイナリ緑（pdf-lay-core lib: 376 passed, 3 ignored;
                            pdf-lay (e2e_api): 5 passed;
                            integration smoke_test: 29 passed, 11 ignored;
                            pdf-lay-cli: 23 passed;
                            他のクレートはunit test 0件で緑）
```

`vector_fig.pdf`実フィクスチャでの手動確認（`cargo run -p pdf-lay-cli -- markdown
tests/fixtures/vector_fig.pdf`）は、フィクスチャ未整備（横断タスクX-1）のため
未実施。フィクスチャ整備後にメンテナが実施し、本ノートに追記することを推奨する。

---

## P4-2: スキャン/画像onlyページのOCR連携 or 部分回復

**実施日:** 2026-07-06
**担当範囲:** 実装。P4-1 の観測結果とエスカレーション方針（本ノート §6「P4-2: GO（着手可能）」）を
引き継ぎ、追加の実PDF調査は行わず着手した（フィクスチャ未整備の状況は P4-1 時点から変わっていない）。

### 1. OCR方式の決定（設計書の A/B 二択）

P4-1 findings §6 が既に「(B) tesseract シェルアウトの方が安全側というのは本調査でも支持する」と
記録済みだったため、本タスクで新たな二択判断はせず、その結論をそのまま採用した。再確認した理由:

- pdf_oxide 0.3.8 の `Cargo.toml` を再確認: `ocr = ["dep:ort", "dep:imageproc", "dep:ndarray"]`。
  有効化には pdf-lay-core の `pdf_oxide` 依存に `features = ["ocr"]` を追加する必要があり、
  ONNX Runtime (`ort`) を新規にビルド依存へ加えることになる。本タスクではこれを行っていない。
- (B) tesseract シェルアウトは `std::process::Command` のみで実装でき、**新規 Cargo 依存はゼロ**
  （`Cargo.lock` に変更なし。`cargo check -p pdf-lay-core --features ocr` 前後で依存グラフの差分は無し）。

### 2. 実装したこと

1. **ゼロ警告ギャップの解消（デフォルトfeatureで有効。P4-1 §2.5 が指摘したギャップ）**
   - `PdfReader::image_xobject_hints`（`extract/pdf_reader.rs`）に `has_any_image: bool` を追加し、
     可視性を `pub(super)` → `pub(crate)` に拡張（`pipeline.rs` から直接呼ぶため）。
   - `pipeline.rs`: テキスト抽出をページ単位ループに変更（下記3.）した後、各ページについて
     `native_chars_by_page[page] < config.ocr.min_native_chars` かつ `has_any_image` を
     「スキャンの可能性があるページ」と判定する。画像も無いページ（真の空白ページ）は対象外とし、
     誤検知を避ける。
   - 該当ページは常に警告になる: OCR無効時は理由付き `PdfLayWarning::PageTextMissing`、
     有効かつ回復成功時は `PdfLayWarning::PageTextRecovered`、有効だが失敗時も理由付き
     `PageTextMissing`。
   - `Coverage::ratio` の `extracted_chars == 0 → 1.0` を `→ 0.0` に修正（`pipeline.rs`
     ＋ `error.rs` のドキュメントコメント）。`0.0` は既定の `min_coverage_ratio`（0.9）を必ず
     下回るため、`extracted_chars == 0` のドキュメントは確実に `LowCoverage` 警告を出すようになった。
   - 新規 `PdfLayWarning` バリアント: `PageTextRecovered { page, method }`,
     `PageTextMissing { page, reason }`（`error.rs`、Display実装込み。PDF由来テキストは含めない）。

2. **OCRシーム（`ocr` cargo feature、既定 off）**
   - `crates/pdf-lay-core/src/extract/ocr.rs`（新規）。`#[cfg(feature = "ocr")]` で
     `extract/mod.rs` から条件付きコンパイルされ、`engine_available(&OcrConfig) -> bool` と
     `ocr_page(&mut PdfReader, page, &OcrConfig) -> Result<Vec<TextSpan>, String>` を
     `pub(crate)` で再輸出。
   - `OcrEngineKind::Tesseract`（既定）: `tesseract --version` の起動成功で可用性チェック。
     `reader.inner_doc().extract_images(page)` から最大サイズの画像を取り出して一時PNGへ保存し、
     `tesseract <path> stdout -l <lang>` を実行してテキストを回収する。ページ全体を1つの
     `TextSpan` として合流させる（bboxは画像自身のbbox → 無ければ `page_media_box` →
     無ければ Letter既定、の順にフォールバック）。
   - `OcrEngineKind::Builtin`: pdf_oxide内蔵OCR用の**予約済みだが未実装**のプレースホルダ。
     選択しても常に「利用不可」（`PageTextMissing`）として扱われ、パニックしない。方針書の
     「わからないことを黙って埋めない」原則に沿って、将来 (A) 案を実装する場合の設定の受け皿を
     明示的な形（コメント付きの列挙子）で残した。
   - `crates/pdf-lay-core/Cargo.toml` に `ocr = []`（新規依存ゼロ、理由をコメントに明記）を追加。
     `pdf-lay`/`pdf-lay-cli`/`pdflay-python` の各 `Cargo.toml` にも `ocr = ["...../ocr"]` を
     forward（既存の `real-tokenizer` と同一パターン）。
   - CLI（`pdf-lay-cli/src/main.rs`）: `--ocr`（bool）・`--ocr-lang`（既定 `jpn+eng`）を追加し、
     `build_config` で `Config.ocr` にマップ。`ocr` cargo feature が未ビルドでも `--ocr` 自体は
     常にパースでき、実行時に「未ビルドのため回復できない」旨の `PageTextMissing` に落ちる。
   - Python（`pdflay-python/src/lib.rs`）: `pdflay.analyze(..., ocr=False, ocr_lang="jpn+eng")`
     引数を追加し、同じ `Config.ocr` にマップ。

3. **部分回復（1ページの抽出失敗が文書全体をゼロにしない）**
   - `pipeline.rs`: `reader.extract_all_text_spans()`（内部の per-page エラーを `log::warn!` する
     だけで `AnalysisResult.warnings` に載らない）の単発呼び出しをやめ、
     `reader.extract_text_spans(page)` をページ単位ループで呼ぶ実装に変更した。ページごとの
     `Err` は `PdfLayWarning::PageSkipped { page, reason }` として `warnings` に積み、
     他ページの処理はそのまま継続する。
   - `PdfReader::extract_all_text_spans`（公開API）自体はシグネチャ・実装とも変更していない
     （設計書の後方互換方針どおり）。呼び出し元は `pipeline.rs` のみだったため実質的に
     未使用になったが、外部呼び出し側との互換性のため残置した。
   - 実証: `/Pages` の `/Count` が2ページを主張するが、2つ目の `Kids` エントリ（`99 0 R`）が
     ファイル中に存在しないオブジェクトを指す合成PDFを構築し、`extract_text_spans(0)` は成功、
     `extract_text_spans(1)` は `Err("... Invalid PDF: Page index 1 not found by scanning")` を
     返すことを確認した
     （`extract::pdf_reader::tests::dangling_second_page_extracts_the_good_page_and_errors_on_the_missing_one`、
     および `pipeline::tests::single_page_extraction_failure_does_not_zero_the_whole_document`）。
     pdf_oxideのページツリー走査とスキャンフォールバックの**両方**が失敗して初めて `Err` になる
     ため、単に壊れたcontentストリームを持つページでは `Err` にならない
     （`get_page_content_data` の失敗は `Ok(Vec::new())` に縮退される —
     `pdf_oxide-0.3.8/src/document.rs:3450` 付近で実ソース確認済み）ことも実験的に確認した。

### 3. 判断・逸脱事項（エスカレーション不要な範囲での判断）

- **OCR対象ページの判定に画像存在チェックを追加した:** 設計書の記述は「テキスト総文字数 < N」の
  みだったが、本ノート §6（P4-1の推奨）は「テキスト閾値 + 画像有無の組み合わせ」を推奨していたため
  そちらを採用した。真に空白のページ（テキストも画像も無い）を誤って「スキャンページ」として
  警告するのを避けるため（回帰テスト
  `pipeline::tests::analyze_pdf_blank_page_is_not_flagged_as_scanned` で固定化）。
- **`OcrConfig::min_native_chars` の既定値 50:** 設計書は「P4-1実測から決める」としていたが、
  P4-1は実スキャンPDFフィクスチャが未整備のため定量的な実測値を残していなかった。代わりに
  pdf_oxide自身の `ocr::needs_ocr`（`pdf_oxide-0.3.8/src/ocr/mod.rs:86`、
  `native_text.trim().len() > 50`）が使っている閾値をソースから確認し、それに合わせた
  （マジックナンバーの根拠を pdf_oxide 自身の実装に求めた）。
- **`Coverage::ratio` の修正方法:** タスク指示どおり「1.0のまま隠れて動くこと」を避ける安全側
  （`0.0`）を採用した。専用フラグ（例: `has_native_text: bool`）を追加する案も検討したが、
  `ratio` 自体の意味を正しくする方がシンプルで、新しい公開フィールドを増やさずに済むため
  `0.0` 化のみを採用した。
- **OCR復元テキストの粒度:** 1ページ=1 `TextSpan`（行単位に分割しない）。設計書が
  「OCRの精度チューニングは非スコープ」と明記しているため、レイアウト精度は追求していない。
- **`OcrEngineKind::Builtin` を未実装のまま追加した:** 設計書の `Config` 項目（`engine:
  OcrEngineKind (Tesseract/Builtin)`）をそのまま反映する一方、(A) 案自体は実装していない。
  選択時は常に「利用不可」として扱われることをテストで固定化済み
  （`extract::ocr::tests::engine_available_is_false_for_builtin_engine`,
  `ocr_page_returns_err_for_builtin_engine_without_panicking`）。

### 4. 検証結果

```
cargo fmt --all --check                                              => OK
cargo clippy --workspace --all-targets -- -D warnings                => OK（警告0）
cargo clippy --workspace --all-targets --all-features -- -D warnings => OK（警告0。ocr + real-tokenizer 含む）
cargo test --workspace                                                => 全緑
  pdf-lay-core lib（既定feature）: 382 passed, 3 ignored
  pdf-lay-core lib（--features ocr）: 386 passed, 3 ignored（+4: extract::ocr::tests）
  pdf-lay-cli: 25 passed（+2: ocr_flag_defaults_to_disabled, ocr_flag_and_lang_are_accepted_and_mapped_to_config）
  pdf-lay (tests/e2e_api.rs): 5 passed
  integration smoke_test: 29 passed, 11 ignored
cargo check -p pdf-lay-core --features ocr             => OK（コンパイル成功、新規依存なし）
cargo check -p pdf-lay --features ocr                  => OK
cargo check -p pdf-lay-cli --features ocr              => OK
cargo check -p pdflay-python --features ocr            => OK
```

**手動スモーク**（合成のスキャン形状PDF、`cargo run -p pdf-lay-cli -- toc <pdf> --no-extract-images`）:

- `--ocr` 無し: `[warning] page 0 has no usable text: page has an embedded image but little/no
  native text, and OCR is disabled ...` に続き `[warning] low text coverage: 0.0% ...` が出力される
  （旧挙動: 警告ゼロで "正常終了"）。
- `--ocr` あり・`ocr` cargo feature 未ビルド: `... OCR was requested (...) but this build was not
  compiled with the "ocr" cargo feature`。
- `--ocr` あり・`ocr` feature ビルド済み・本サンドボックス環境（`tesseract` 不在、`which tesseract`
  で確認済み）: `... configured OCR engine is not available on this machine (e.g. the "tesseract"
  binary was not found on PATH)`。
- 上記いずれもパニック・エラー終了せず、CLIは正常終了コードで完了する。

**未検証（tesseractバイナリが実際に利用可能な環境でのEnd-to-Endの成功パス）:** 本サンドボックス
環境に `tesseract` が無いため、`PageTextRecovered` が実際に発火する経路（tesseract成功→テキスト
回収→spanマージ）は自動テストでは「起動できない場合の失敗パスがグレースフルであること」までしか
検証できていない。`extract::ocr::tests` はこの失敗パス（`Builtin`選択時、および本環境での
tesseract不在時）のみを固定化している。tesseractが利用可能な環境での実際の recovery 成功と、
実スキャンPDF（`tests/fixtures/scanned.pdf`、横断タスクX-1で未整備）での効果測定は、
今後の宿題としてここに明記する。
