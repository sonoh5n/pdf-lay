# pdf-lay リファクタリング提案書

**対象:** ローカルLLMが長尺PDFを効率よく解釈できるCLIツールへの再設計
**日付:** 2026-07-04
**スコープ:** 全サブシステム（抽出→レイアウト→構造→図表→出力→Skills）の問題洗い出しと段階的リファクタリング案

---

## 0. エグゼクティブサマリ

現状の pdf-lay は「学術論文PDF → LLM向け中間表現」というコンセプトは妥当で、コア型 `PaperDocument` が
`serde` 対応の階層的IRになっている点は強い資産である。しかし **仕様書（`docs/arch/01_SPECIFICATION.md`）が
約束する機能と実装の乖離が大きく**、さらに **「テキストを黙って捨てる」経路が複数** 存在するため、
ユーザーが挙げた4つの不満はいずれも実コードで再現する構造的欠陥に対応している。

ユーザーの訴えと根本原因の対応:

| 訴え | 主因（最重要のみ） | 場所 |
|------|-------------------|------|
| ①文章が全て文書化できていない | **ページ寸法が 612×792 固定** → A4(595×842)の上端帯を毎ページ脱落 | `extract/pdf_reader.rs:67-86` |
| ②Skillsとして正しく呼べない | SKILL.md が slash-command 形式で誤梱包 + CLIに JSON/chunk 出力が無い | `.claude/skills/*/SKILL.md`, `pdf-lay-cli/src/main.rs:164-178` |
| ③画像を適切に埋め込めない（相対PATH等） | リンクパスを相対計算せず `image_base` を単純連結、LLM経路は生パス漏れ | `output/markdown.rs:263-278`, `selector/llm_text.rs:193-208` |
| ④セクション判定が曖昧 | header検出が `block_type` を無視 + no-header時に全文書が1セクション化 | `structure/header_detector.rs:84-90`, `structure/section_builder.rs:63-68` |
| （横断）数式が壊れる | `math_config` が全経路で `None` 固定 → 数式変換が**一度も動かない死にコード** | `main.rs:415`, `pdflay-python/src/lib.rs:104`, `output/chunker.rs` |

**最優先の3点**（これだけで体感品質が大きく変わる）:
1. 実ページ寸法の取得（①の根本治療）
2. 数式変換をデフォルトで有効化 or CLIフラグ化（横断課題の解消）
3. 画像リンクの相対パス計算（③の根本治療）

---

## 1. 問題点一覧（重大度別・file:line付き）

### 1.1 テキスト網羅性（訴え①）

| # | 重大度 | 問題 | 場所 |
|---|--------|------|------|
| T1 | **CRITICAL** | `page_dimensions()` が MediaBox を読まず **612×792 を定数返し**（コメントは「MediaBoxをパース」と虚偽）。A4では `bbox.top>792` の上端帯（タイトル/著者/最初の見出し）が `block_grouper` の領域フィルタで毎ページ脱落。横長ページも `center_x>=612` を喪失 | `extract/pdf_reader.rs:67-86` |
| T2 | HIGH | 領域/カラムフィルタが**非全域**。Y判定が「行が領域内に完全内包」を要求（`top<=y_top && bottom>=y_bottom`）、X判定は `center_x` と不整合。混在レイアウトで境界跨ぎ行が両領域から漏れて消える | `structure/block_grouper.rs:76-85` |
| T3 | HIGH | レンダラ／`full_text` が `Caption/PageNumber/RunningHeader/RunningFooter` を**無条件 `continue`**。誤分類された本文がMarkdown・chunk両方から消える | `output/markdown.rs:205-209`, `types/text.rs:178-193` |
| T4 | HIGH | `is_caption` が過剰マッチ。「Table 1 shows the results…」等の本文が `Caption` 化 → T3で脱落 | `structure/block_classifier.rs:221-227` |
| T5 | HIGH | CID/CJK(Identity-H)・縦書き・回転・スキャンPDFの取りこぼし。OCRフォールバック無し。`ow<=0||oh<=0` で退化bboxを捨てる | `extract/pdf_reader.rs:93-130,211-240` |
| T6 | MED | `is_running_header`: 単行 && `font_size<0.85×body` を機械的に RunningHeader 化 → 小フォントの正当な1行が脱落 | `structure/block_classifier.rs:237-243` |
| T7 | MED | 1ページのpdf_oxideエラーで**そのページ全テキストを破棄**（warningのみ、部分回復なし） | `extract/pdf_reader.rs:116-130` |
| T8 | MED | 3カラム以上を1カラムに潰す／2カラム分割が左端ピーク中点のみ → 読み順崩れ・誤バケット | `layout/column_detector.rs:194-224` |

### 1.2 セクション判定（訴え④）

| # | 重大度 | 問題 | 場所 |
|---|--------|------|------|
| S1 | HIGH | ヘッダー未検出時に**全文書が1つの巨大セクション**になる。以降 `select_sections` が何も返さない | `structure/section_builder.rs:63-68,117` |
| S2 | HIGH | `HeaderDetector::detect` が `block_type` を一切参照しない。caption/running head/reference もヘッダー候補になる（分類器の仕事を捨てている） | `structure/header_detector.rs:84-90` |
| S3 | HIGH | 反復ヘッダー/フッター除去 `detect_repeated_headers_footers` が**pipelineに未接続**（テストからのみ呼ばれる）。走りヘッダが毎ページ偽ヘッダー化 | `structure/block_classifier.rs:78` vs `pipeline.rs` |
| S4 | HIGH | `text.len()`（**バイト長**）でフィルタ + 英語専用シグナル（all-caps/既知名リスト）。CJK見出しは ~40字で120バイト超 → 除外。日本語論文で見出しが落ちる | `structure/header_detector.rs:96,205-216` |
| S5 | MED-HIGH | level を**ブロック単体**で決定（大域フォントクラスタリング無し）。ローマ数字は常にlevel1、`1.` もlevel1で番号体系混在に弱い。`1 Introduction`（ピリオド無し）は番号未検出 | `structure/header_detector.rs:160-203` |
| S6 | MED | 既知セクション名の**無制限 `contains`** が過剰発火（"METHOD"/"RESULT"等が短い行に含まれると+2点） | `header_detector.rs:215`, `block_classifier.rs:268` |
| S7 | MED | selector の名前マッチが無境界 substring + 一致時に**サブツリー丸呑み**。`"in"`→INTRODUCTION 等の誤爆、子セクションを個別に選べない | `selector/selector.rs:131-144` |
| S8 | MED | `size_ratio>1.8 && !既知名` の見出しを**削除**（英語以外の大きな章題・書籍見出しが消える） | `header_detector.rs:115` |
| S9 | LOW-MED | `SectionHeader.block_index` に enumerate 位置を格納し `global_index` で引く。現状は偶然一致。将来の並べ替え/フィルタで全ヘッダーが誤アンカー化する時限爆弾 | `header_detector.rs:88,177` vs `section_builder.rs:46` |

### 1.3 画像埋め込み（訴え③）

| # | 重大度 | 問題 | 場所 |
|---|--------|------|------|
| I1 | HIGH | **相対パスを一切計算しない**。`fig.image.path` のディレクトリを捨てて `file_name()` だけ取り、`image_base` を `format!("{}/{}")` で単純前置。`-o` で出力先を変えると即リンク切れ | `output/markdown.rs:263-278` |
| I2 | HIGH | LLM/RAG経路は `image_base` を**無視**して生ディスクパス（絶対パスも）をそのまま埋め込む。`LlmTextConfig` に base フィールドが無い。markdownと出力が不一致 | `selector/llm_text.rs:193-208`, `config.rs:217-235` |
| I3 | HIGH | **chunk.text に画像参照が入らない**。図は別枠 `Chunk.figures` のみ。`TokenCount`/`Paragraph` 戦略では figure を完全脱落。分割時は先頭サブチャンクに全図を寄せる | `output/chunker.rs:149-158,259,287-305` |
| I4 | MED-HIGH | 図の挿入位置がキャプション基準で、本文参照（"as shown in Fig.3"）非考慮。さらに `markdown.rs` の `continue`（T3）が**図のドレインもスキップ**し、キャプション型ブロックにアンカーされた図がセクション末尾に飛ぶ | `figure/image_matcher.rs:165-182`, `output/markdown.rs:205-247` |
| I5 | MED | PNG強制（`ImageFormat::Jpeg`/`Other` は未生成）。ベクター図・inline画像・Form XObjectは非抽出 → ベクター図はキャプションだけ残り `UnmatchedCaption`。SMask/CMYK検証なし。1画像の失敗でそのページ全画像を破棄 | `extract/image_extractor.rs:51-103,36-41` |
| I6 | MED | caption 正規表現が狭い（`Fig/Figure/Table/Tab.` + ASCII数字 + 行頭のみ）。"FIG. 1a"/"Scheme 1"/"Figure S1"/日本語「図1」を取りこぼす → 図脱落 | `figure/caption_detector.rs:56,61` |

### 1.4 LLM/RAG出力・数式・テーブル（横断）

| # | 重大度 | 問題 | 場所 |
|---|--------|------|------|
| L1 | HIGH | **数式変換が実質デッドコード**。`MarkdownConfig.math_config` デフォルト `None`、CLIは `None` 固定、Python `to_markdown` も `None`。数式が生グリフ（PUA文字化け）で出る | `config.rs:90`, `main.rs:415`, `pdflay-python/src/lib.rs:104` |
| L2 | HIGH | **Chunker が数式変換を通らない**。`Section::full_text()` の生テキストを使うため、全チャンクに未変換数式が入る。テーブルmarkdownも figure placeholder もチャンク本文に無い | `output/chunker.rs:33,85`, `types/text.rs:178-193` |
| L3 | HIGH | `include_section_context` が**読まれないデッド設定**。チャンクにパンくず（親セクション）も見出し行も付かない → RAGで位置情報が失われる | `config.rs:263-264`（`chunker.rs`で未使用） |
| L4 | HIGH | **CLIに json/chunk/llm-text サブコマンドが無い**。RAGの主経路がPython専用。CLIのみのエージェント（リリースバイナリ）は `pdf-to-llm` を実行不能。`PyChunk` にも `tables` getter が無い | `pdf-lay-cli/src/main.rs:164-178`, `pdflay-python/src/lib.rs:695` |
| L5 | HIGH | トークン推定が固定ヒューリスティック（ASCII/4 + 非ASCII/1.5）。実BPEとの差が CJK・数式で2-4倍。`TokenCount` の文字予算が ASCII比固定で **CJKチャンクが大幅にオーバー** | `output/chunker.rs:244-245,310-314` |
| L6 | HIGH | 結合セル（colspan/rowspan）非対応。マルチヘッダは最終行だけに潰す。罫線なしテーブルはキャプション必須 | `table/grid_builder.rs:78-93`, `table/text_converter.rs:19-27` |
| L7 | MED-HIGH | 「LaTeX」は記号置換のみ（`\frac`・被開平のグルーピング・積分限界なし）。数式検出はフォント名依存でサブセット/Type3フォントを取りこぼす | `math/converter.rs`, `math/detector.rs:55-92` |
| L8 | MED | 巨大な単一段落がサブ分割されず1つの予算超過チャンクになる | `output/chunker.rs:138-175` |

### 1.5 Skills統合（訴え②）

| # | 重大度 | 問題 | 場所 |
|---|--------|------|------|
| K1 | HIGH | `.claude/skills/*/SKILL.md` が **slash-command 形式で誤梱包**。`argument-hint`・`/pdf ...` 呼び出し・`$PDF_PATH` 等はモデル起動スキルでは無効。ユーザーが `/pdf` と打っても `.claude/commands/` が無いので起動しない | `.claude/skills/pdf*/SKILL.md:4,14-20` |
| K2 | HIGH | どのランタイムも置換しないプレースホルダ（`$PDF_PATH`,`$MAX_TOKENS`…）を本文が参照。Codex側は `$1` で正しく書けており、`.claude` 版が誤複製されたと分かる | `.claude/skills/*/SKILL.md` |
| K3 | MED | 実在しない `--format markdown\|json\|toc` を宣伝（実CLIはサブコマンド `toc`/`markdown`）。字義通り実行するとclapエラー | `pdf/SKILL.md:3-4,16-18` |
| K4 | MED | CLIに JSON/chunk 出力が無い（L4）ため、Python無しのエージェントは `pdf-to-llm` を満たせない。「fallbackで手動チャンク」は実質できない | 同上 |
| K5 | MED | 配布パッケージング無し（`plugin.json`/`marketplace.json` 無し）。スキルはこのリポジトリを cwd にした時しかロードされず、「Devin/Codex/Claudeで再利用」の主張と矛盾 | リポジトリ全体 |
| K6 | LOW | `allowed-tools: Bash(python *)` だが README は `python3`。`python` しか無い環境／`python3` しか無い環境で検出が滑る | `SKILL.md:5`, `README.md:112-114` |

### 1.6 仕様乖離（メタ問題）

`docs/arch/01_SPECIFICATION.md` は `pdf-lay json` / `llm-text` / `--sections "A,B"` / `--section-index` /
`--math-format` / `analyze` / `debug-layout` / バッチ処理を約束しているが、**実CLIは `toc` と `markdown` の2つだけ**。
Skills や README がこの「約束されたが未実装のCLI」を参照するため、②の混乱を増幅している。
リファクタリングでは「仕様書＝実装契約」として同期させることが前提。

---

## 2. リファクタリング設計方針

### 2.1 中核原則

1. **テキストを黙って捨てない（No Silent Drop）**
   すべての破棄地点で「破棄する代わりに最近傍へ割り当てる」か「破棄したなら warning + 別枠退避」を徹底。
   カバレッジ指標（抽出スパン数 → 出力到達文字数）を計測可能にする。

2. **単一の高忠実度レンダリングコア**
   現状 markdown / llm_text / chunker が**別々の忠実度**を持つ（数式・テーブル・図の扱いがバラバラ）。
   これを1つの「ブロック→リッチテキスト」変換コアに統合し、markdown/llm_text/chunk/json はその射影にする。
   → L2・L3・I3 が構造的に解消し、忠実度の乖離が消える。

3. **分類は一度だけ、下流は再導出しない**
   `BlockClassifier` の結果を `HeaderDetector` に供給し、caption/running head/reference を除外。
   → S2・S3・T4 が一括で改善。

4. **数式・トークナイザは差し込み可能に**
   トークナイザを trait 化してターゲットLLMの実tokenizer（`tokenizers` クレート等）を差せるようにする。

5. **仕様書と実装・Skills・READMEを1つの契約として同期**

### 2.2 目標アーキテクチャ（データフロー）

```
PDF
 └─ extract/       … 実ページ寸法・全グリフ・（将来）OCR/inline画像
     └─ layout/    … 全域割当（drop禁止）・カラム/読み順
         └─ structure/ … 分類→[分類結果を供給]→ヘッダー検出→階層化
             └─ figure/table/math … 資産抽出＋位置決定
                 └─ IR: PaperDocument（正、serde、安定ID付き）
                     └─ render-core（唯一の忠実度）
                         ├─ markdown（相対パス計算）
                         ├─ llm_text（同一パス方針）
                         ├─ chunk（math/table/figure/breadcrumb込み・実tokenizer）
                         └─ json（content-only射影オプション）
```

---

## 3. 段階的ロードマップ

### Phase 0 — 正確な土台（バグ修正・最優先 / 小～中）

体感品質を最も動かす修正。破壊的変更は少ない。

| 対応 | 内容 | 対象 |
|------|------|------|
| P0-1 | **実ページ寸法の取得**。`pdf_oxide` のページ幾何 or MediaBox をパース。取れない場合はスパンbboxの実測値（max top/right）でフォールバックし、612×792定数は撤廃 | `pdf_reader.rs:67-86` |
| P0-2 | **領域/カラムフィルタの全域化**。どの行も最近傍の(region,column)へ必ず割り当て、`center_x` と Y判定の基準を統一。取りこぼしゼロを不変条件に | `block_grouper.rs:76-85` |
| P0-3 | **数式変換をデフォルト有効化**（または `--math-format latex\|unicode\|plain\|off` を追加）。CLI/Python の `math_config: None` ハードコードを撤廃 | `main.rs:415`, `pdflay-python/src/lib.rs:104` |
| P0-4 | **画像リンクの相対パス計算**。`diff_paths(image_dir, output_dir)` + filename に置換（`pathdiff` 導入）。stdout時は `image_base` 明示指定を尊重 | `markdown.rs:263-278` |
| P0-5 | **drop対象ブロックの厳格化**。`is_caption`/`is_running_header` を「実際に図表へマッチした/反復検出された」場合に限定し、疑わしきは本文として残す | `block_classifier.rs:221-243`, `markdown.rs:205-209`, `text.rs:178-193` |
| P0-6 | **カバレッジ計測**を warnings に追加（抽出文字数 vs 出力到達文字数、脱落ブロック数） | `pipeline.rs` |

### Phase 1 — セクション判定の再設計（中）

| 対応 | 内容 | 対象 |
|------|------|------|
| P1-1 | 分類結果を `HeaderDetector` に供給し、caption/running/reference/footnote を候補から除外 | `header_detector.rs`, `pipeline.rs` |
| P1-2 | `detect_repeated_headers_footers` を pipeline に接続（ヘッダー検出前に走らせる） | `pipeline.rs` |
| P1-3 | **文書大域フォントクラスタリングで level 決定**（size/bold/caps/left-x の特徴量をクラスタ化 → ランク→level）。孤立閾値 1.1/1.15 を置換 | `header_detector.rs:160-203` |
| P1-4 | **番号を構造キー化**（`[2,1,3]` にパース、単調性検証、スキップ/重複検出、`1 Introduction` のピリオド任意、ローマ数字を任意level、appendix `A/B`、4階層超対応） | `header_detector.rs:184-203` |
| P1-5 | Unicode正規化（`chars()` カウント、CJK見出しヒューリスティック、既知名リスト拡張／多言語化） | `header_detector.rs:96,205-216` |
| P1-6 | **no-confident-header フォールバック**（フォント変化/段落でセグメント）。文書が1セクションに潰れるのを防止 | `section_builder.rs` |
| P1-7 | selector に exact / word-boundary / normalized マッチモード。サブツリー自動丸呑みを既定オフに | `selector/selector.rs:131-144` |
| P1-8 | `block_index` を `global_index` 直参照に修正（S9の時限爆弾除去） | `header_detector.rs:88,177` |

### Phase 2 — ローカルLLM出力の一本化（中～大・本命）

| 対応 | 内容 | 対象 |
|------|------|------|
| P2-1 | **chunk 本文を render-core 経由に**。数式変換・テーブルmarkdown・図placeholder を chunk.text に含める | `chunker.rs`, `text.rs` |
| P2-2 | `include_section_context` を実装（パンくず `METHODS > Data Collection` + 見出し行をチャンク先頭に） | `chunker.rs` |
| P2-3 | **`Tokenizer` trait 導入**。既定は改良ヒューリスティック、`--tokenizer <model>` で実tokenizer（`tokenizers`クレート）を差し込み。文字予算とトークン予算の不整合（CJK超過）を解消 | `chunker.rs:244-314` |
| P2-4 | `TokenCount`/`Paragraph` でも section 属性・figure/table を保持。巨大単一段落のサブ分割 | `chunker.rs:225-305,138-175` |
| P2-5 | `LlmTextConfig` に `image_base` を追加、図を `insertion_point` 順に挿入（markdownと統一） | `llm_text.rs`, `config.rs` |
| P2-6 | **CLIに `json` / `chunks`（JSONL） / `llm-text` サブコマンド追加**。Python無しでRAG完結。`PyChunk` に `tables` getter 追加 | `pdf-lay-cli/src/main.rs`, `pdflay-python/src/lib.rs` |
| P2-7 | JSON に content-only 射影オプション（bbox/フォント等の幾何を落とした軽量IR）。数式変換済みテキストを含める | `output/json.rs` |
| P2-8 | テーブル: colspan/rowspan モデル導入、マルチヘッダ保持、罫線なし検出の条件緩和 | `table/*` |

### Phase 3 — Skills 再パッケージング（小～中）

| 対応 | 内容 | 対象 |
|------|------|------|
| P3-1 | **command と skill を分離**。slash想定の本文は `.claude/commands/*.md` へ移し `$1`/`$ARGUMENTS` 化。または SKILL.md をモデル起動用に書き直し（`argument-hint`・`/pdf`・`$PDF_PATH` を除去し自然言語からパス/オプションを導く指示に） | `.claude/skills/*`, 新 `.claude/commands/*` |
| P3-2 | phantom `--format` を除去、実サブコマンド（`toc`/`markdown`/新設 `json`/`chunks`）に整合 | SKILL/prompt 本文 |
| P3-3 | 配布パッケージ化（`plugin.json` + `.claude-plugin/marketplace.json`、または `~/.claude/skills/` へのインストール手順）でリポジトリ外でも起動可能に | 新規 |
| P3-4 | `python3` 統一・`allowed-tools` 整合・`pip install pdflay` の実在確認（未公開なら `maturin develop` を主手順に） | SKILL frontmatter, README |
| P3-5 | **仕様書（01_SPECIFICATION.md）を実装に同期**。未実装CLIの記述を「実装済み」と「将来」に区分 | `docs/arch/*` |

### Phase 4 — 抽出堅牢化（要調査・大）

ユーザーの実PDFでの経験的確認が前提（特に日本語）。

| 対応 | 内容 | 対象 |
|------|------|------|
| P4-1 | `pdf_oxide 0.3.8` を **実際の日本語A4 PDF** で検証。CID/Identity-H・ToUnicode・縦書き・スキャンの挙動を確認 | `pdf_reader.rs` |
| P4-2 | スキャン/画像onlyページに OCR 連携 or 部分回復パスを追加（別プロセス/フィーチャフラグ） | `extract/` |
| P4-3 | ベクター図・inline画像・Form XObject の抽出、PNG以外の保存形式、SMask/CMYK対応 | `image_extractor.rs` |
| P4-4 | caption 正規表現拡張（`FIG.`/`Scheme`/`Chart`/`Figure S1`/「図1」「表1」等） | `caption_detector.rs` |

### 横断 — 検証基盤

- **ゴールデンテスト用の実PDFフィクスチャ**を用意（A4日本語1段、IEEE2段、arXiv、スキャン）。現状 `tests/fixtures/sample.pdf` が無く主要テストが `#[ignore]`（`pipeline.rs:333`）。
- メトリクス: テキスト網羅率、読み順正確性、セクション検出F1、キャプション-画像マッチ率。仕様書 §3.2 の目標値（見出し>95%等）を CI で継続測定。

---

## 4. 優先順位の推奨

**すぐ効く順（quick wins）:** P0-1（ページ寸法）→ P0-3（数式ON）→ P0-4（相対パス）→ P0-5（drop厳格化）→ P0-2（全域割当）。
この5つで①③と数式の体感が大きく改善する。破壊的変更が小さく、既存テストへの影響も限定的。

**次に構造改善:** Phase 1（セクション＝④の核）→ Phase 2（LLM出力一本化＝本ツールの存在意義）。

**並行可能:** Phase 3（Skills）は Phase 2 の CLI 新設（P2-6）に依存するが、SKILL.md の形式修正（P3-1/P3-2）は独立して先行可能。

**要調査で後回し可:** Phase 4（実PDF依存、特にOCR/CJKフォント）。ただし日本語PDFで①が P0-1 後も残るなら P4-1 を前倒し。

---

## 5. 想定される破壊的変更・互換性

- `math_config` デフォルト変更（P0-3）で既存出力が変わる → メジャー前（rc段階）なので許容。フラグで旧挙動も残せる。
- 画像リンクパスの変更（P0-4）で既存の `.md` 参照が変化 → `--image-base` 明示時は従来通りにするフォールバックを用意。
- CLIサブコマンド追加（P2-6）は後方互換（既存 `toc`/`markdown` は不変）。
- `LlmTextConfig`/`ChunkConfig` へのフィールド追加は `#[serde(default)]` で後方互換を維持。

---

## 付録A: 調査で確認した「壊れていない」点（スコープ確定用）

- Python API 自体は仕様と整合し、ドキュメントされたメソッドは全て存在する（`to_chunks`/`to_llm_text`/`select_sections`/`to_json` 等）。②の主因は**パッケージングと本文の形式**であって Python バインディングの欠落ではない。
- ヘッダー前の前文（preamble）はヘッダー無しセクションとして保持される（`section_builder.rs:44-71`）。ただし①のページ寸法問題で上端帯が落ちる分は別問題。
- `block_grouper` を通過したブロックは必ずいずれかのセクションに入る（脱落は上流フィルタと下流レンダラのblock-typeフィルタが主因）。
- Markdown/JSON の HTML/YAML インジェクション対策（サニタイズ）は実装済みでテストも充実。

## 付録B: 主要マジックナンバー（設定化候補）

| 値 | 意味 | 場所 |
|----|------|------|
| 612×792 | ページ寸法（**バグ**、撤廃対象） | `pdf_reader.rs:83-84` |
| min_score=4 / max_chars=120 / max_lines=3 | ヘッダー判定 | `config.rs:189` |
| 1.1 / 1.15 / 1.8 | size_ratio 各種閾値 | `header_detector.rs` |
| 0.85 | running header フォント比 | `block_classifier.rs:237` |
| 50pt | caption-image 最大距離 | `config.rs:43` |
| 4 / 1.5 | トークン推定 char/token | `chunker.rs:310-314` |
| 0.20 | カラムピーク閾値 | `column_detector.rs` |
