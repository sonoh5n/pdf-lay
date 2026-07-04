# Phase 3 — Skills 再パッケージング & 仕様書同期

**対象読者:** 本フェーズを実装するコーディングエージェント（Codex 等）
**前提:** 本書は `docs/refactor/00_REVIEW_POLICY.md`（以下「レビュー方針」）に従属する。
本書とレビュー方針が矛盾する場合はレビュー方針を優先する。
**上位提案:** `docs/refactor/REFACTORING_PROPOSAL.md` の §1.5（K1〜K6）と §1.6（仕様乖離）。

---

## 前文 — なぜ現状の Skills は「呼べない」のか

### Claude Code には別物の2機構がある

このフェーズを実装する前に、Claude Code の**2つの別々の機構**を混同しないこと。両者はファイルの置き場所・フロントマターのスキーマ・引数の渡り方・起動方法がすべて異なる。

| 観点 | (1) Slash Command | (2) Agent Skill |
|------|-------------------|-----------------|
| 置き場所 | `.claude/commands/<name>.md` | `.claude/skills/<name>/SKILL.md` |
| 起動方法 | ユーザーが明示的に `/name ...` と打つ | **モデルが `description` を読んで自律的に選ぶ**（ユーザーは `/name` と打たない） |
| 引数 | `$1`..`$9` / `$ARGUMENTS` に**ランタイムが文字列置換**する | **置換されない**。引数は自然言語としてモデルの文脈に届く |
| フロントマター | `description`, `argument-hint`, `allowed-tools` 等 | `name`, `description`（必須）＋任意 `allowed-tools` |
| `argument-hint` | 有効（補完UIに出る） | **無効**（存在しないキー） |

### 現状の SKILL.md は「slash-command の中身を skill の場所に誤配置」したもの

現物（`.claude/skills/pdf/SKILL.md` 他3つ）は、フロントマターに `argument-hint` を持ち、本文の `## Usage` で `/pdf paper.pdf --format toc` のように**スラッシュ起動を案内**し、コマンド行に `$PDF_PATH` / `$MAX_TOKENS` / `$OVERLAP` / `$STRATEGY` という**プレースホルダ**を書いている。これは (1) Slash Command 用の書き方である。ところが置き場所は `.claude/skills/` すなわち (2) Agent Skill の場所であり、次の二重故障を起こす:

1. **Agent Skill として:** `argument-hint` は無視され、`/pdf` は起動トリガにならず（`.claude/commands/pdf.md` が存在しないため `/pdf` 自体が未定義。本リポジトリに `.claude/commands/` は無い＝確認済み）、そして `$PDF_PATH` 等は**誰も置換しない生文字列**のままシェルに渡る。モデルがそのまま実行すれば `pdf-lay toc "$PDF_PATH"` は空パスまたは未定義変数エラーになる。
2. **Slash Command として:** そもそも `.claude/commands/*.md` が無いので `/pdf` は存在しない。

対照的に `.codex/prompts/*.md`（Codex 用スラッシュプロンプト）は**正しく `$1` を使っている**。つまり `.claude` 版はこの Codex 版を誤って複製し、`$1` を `$PDF_PATH` に置換してしまった、という履歴が読み取れる。`.codex/prompts/*.md` を「$1 の参照実装」として扱う。

### さらに CLI とプレースホルダが実体と乖離している

- フロントマター `argument-hint` と本文 Usage が宣伝する `--format markdown|json|toc` は**実在しないフラグ**。実 CLI（`crates/pdf-lay-cli/src/main.rs`）のサブコマンドは **`toc` と `markdown` の2つだけ**で、出力形式はフラグではなくサブコマンドで切り替える。字義通り `pdf-lay markdown x.pdf --format json` を実行すると clap がエラーを返す。
- `allowed-tools: Bash(python *)` だが README のインストール手順は `python3`。`python` エイリアスが無い環境で許可が滑る。
- スキルはこのリポジトリを cwd にした時しかロードされない（配布パッケージング無し）。「Devin/Codex/Claude で再利用できる」という `.devin/skills.md` 冒頭の主張と矛盾する。
- `pip install pdflay` を案内しているが、**PyPI に `pdflay` は存在しない**（`https://pypi.org/pypi/pdflay/json` が 404 ＝確認済み）。唯一動くのは `maturin develop`。

### このフェーズのゴール

「**すべての skill / command が案内するすべての起動コマンドが、実在する CLI サブコマンドまたは実在する Python メソッドに1対1で対応し、かつリポジトリ外でもロードできる**」状態にする。加えて仕様書 `docs/arch/01_SPECIFICATION.md` の「約束したが未実装」の CLI/API を実体に同期させ、混乱の発生源を断つ。

---

## タスク一覧

| ID | タイトル | 重大度 | 依存 |
|----|---------|--------|------|
| P3-1 | command と skill の分離（true skill への書き直し） | HIGH | なし（先行可） |
| P3-2 | phantom `--format` の除去 / 実サブコマンド整合 | MED | 一部 P2-6（`json`/`chunks`/`llm-text`） |
| P3-3 | 配布パッケージ化（plugin + marketplace / global install） | MED | P3-1（整形済み skill が前提） |
| P3-4 | インタプリタ / インストール整合（python3・maturin） | MED | P3-1（frontmatter を触るため）|
| P3-5 | 仕様書 `01_SPECIFICATION.md` を実装に同期 | MED | 参照のみ（P2-6 の最終名を反映） |

**実施順の推奨:** P3-1 → P3-4 →（P2-6 マージ後に）P3-2 → P3-3 → P3-5。
P3-1 と P3-4 は同じ frontmatter/本文を触るので、実装上は近接して行うと衝突が少ない（ただしレビュー方針「1タスク=1PR」は厳守。P3-1 の PR がマージされてから P3-4 に着手する）。

> **重要な依存の注意:** P3-2 と P3-5 は Phase 2 の **P2-6（CLI に `json` / `chunks`(JSONL) / `llm-text` サブコマンド追加、`PyChunk` に `tables` getter 追加）** に依存する部分がある。P2-6 が未マージの間は、これらの新サブコマンドを**案内文に含めてはならない**（また新たな phantom を生む）。P2-6 未マージ時点で P3-2 を進める場合は、実在する `toc`/`markdown` のみに整合させ、新サブコマンドの追記は P2-6 マージ後の追補 PR に回す（PR 説明にその旨を明記）。

---

### P3-1: command と skill の分離（true Agent Skill への書き直し）

**目的:** 誤配置された slash-command 本文を、`.claude/skills/*/SKILL.md` として正しくモデル起動される Agent Skill に書き直し、`argument-hint`・`/pdf` 起動案内・`$VAR` プレースホルダを排除する。

**重大度 / 依存:** HIGH / なし（先行可能）。ただし frontmatter を触るため P3-4 と同一ファイルを編集する。実装順は P3-1 を先にマージする。

**対象ファイル:**
- `.claude/skills/pdf/SKILL.md`（現 1–89 行）
- `.claude/skills/pdf-qa/SKILL.md`（現 1–94 行）
- `.claude/skills/pdf-summary/SKILL.md`（現 1–98 行）
- `.claude/skills/pdf-to-llm/SKILL.md`（現 1–102 行）
- 参照（変更しない）: `.codex/prompts/pdf.md` 他3つ（`$1` の参照実装）

**現状（問題）:**

`.claude/skills/pdf/SKILL.md:1-6`（frontmatter）:
```yaml
---
name: pdf
description: Analyze academic paper PDF to extract structured content (sections, figures, tables) using pdf-lay
argument-hint: "<pdf-path> [--format markdown|json|toc] [--section NAME]"
allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python *), Read
---
```
- `argument-hint`（4行目）は Agent Skill では**無効なキー**。
- `--format ...`（4行目）は**実在しないフラグ**。
- `allowed-tools`（5行目）の `Bash(python *)` は `python3` 環境で滑る（→ P3-4 で修正）。

本文 `.claude/skills/pdf/SKILL.md:13-20`:
```
## Usage
    /pdf paper.pdf                          # Full Markdown output (default)
    /pdf paper.pdf --format toc             # Table of contents with token estimates
    /pdf paper.pdf --format json            # Full JSON output
    /pdf paper.pdf --section "Introduction" # Extract specific section only
```
- `/pdf` 起動案内（Agent Skill は `/name` で呼ばない）。
- `--format toc|json` は phantom。

本文コマンド `.claude/skills/pdf/SKILL.md:47-66`:
```
pdf-lay toc "$PDF_PATH"
pdf-lay markdown "$PDF_PATH" --no-page-numbers
pdf-lay markdown "$PDF_PATH" --section "Introduction" --no-page-numbers
...
doc = pdflay.analyze("$PDF_PATH", extract_images=False)
```
- `$PDF_PATH` は**どのランタイムも置換しない**生文字列（K2）。他3ファイルも同様（`pdf-qa` は `$PDF_PATH`、`pdf-summary` は `$PDF_PATH`、`pdf-to-llm` は `$PDF_PATH` / `$MAX_TOKENS` / `$OVERLAP` / `$STRATEGY`）。

**変更後の期待動作:**

4ファイルを**真の Agent Skill** に書き直す（Option B を採用。理由は下記）。すなわち:
- frontmatter は `name` + `description`（＋任意 `allowed-tools`）のみ。`argument-hint` を削除。
- 本文から `/pdf ...` の Usage を削除し、代わりに「ユーザーの自然言語からPDFパスと望む出力を読み取る」指示にする。
- 本文のコマンド例から `$PDF_PATH` 等のプレースホルダを排除し、`<PDF_PATH>`（山括弧の説明用トークン。モデルが実パスに差し替える旨を明記）または「（ユーザーが与えた実際のパスに置換して実行する）」という**指示文**に変える。シェル変数に見える `$...` を一切書かない（モデルが誤ってそのまま貼るのを防ぐ）。
- `description` は起動トリガを兼ねるため、「いつこのスキルを使うか」を1〜2文で明記する（Agent Skill はモデルが description だけを見て選ぶため）。

**採用方針の決定（Option A vs Option B）:**

- **Option A**（slash body を `.claude/commands/*.md` へ移し `$1`/`$ARGUMENTS` 化）
- **Option B**（SKILL.md を true skill に書き直す）← **本タスクはこちらを採用**

**採用理由:** ディレクトリが `.claude/skills/` であり、リポジトリの売り（`.devin/skills.md` 冒頭）が「Devin/Codex/Claude が**自然言語で**PDFを扱う」ことなので、モデル自律起動＝Agent Skill が本来の意図に合致する。`$1` 置換に依存する slash-command 版は既に `.codex/prompts/*.md` として存在するため、`.claude/commands/` を新設する必要は薄い。
**任意（本タスクの必須スコープ外）:** slash 起動も欲しい利用者向けに `.claude/commands/pdf.md` 等を `.codex/prompts/*.md` の本文流用で追加してもよいが、**本 PR には含めない**（1タスク=1PR）。必要なら別タスク化する。

> **スキーマの前提（レビュー方針 §7 に基づく明示）:** Agent Skill の `SKILL.md` frontmatter は `name`（必須）・`description`（必須）を持ち、`allowed-tools` を任意で受ける、と仮定する。`allowed-tools` が Agent Skill で無視される実装であっても**害はない**（余分なキーは単に効かないだけ）ため保守的に残す。もしレビューで「Agent Skill は `allowed-tools` を許容しない／エラーになる」と判明した場合は、`allowed-tools` 行を削除する方針に切り替える（PR 説明の「レビュアーへの質問」に選択肢を明記）。

**実装手順:**

1. 4ファイルすべてで frontmatter から `argument-hint` 行を削除する。`allowed-tools` は残すが `Bash(python *)` → `Bash(python3 *)` に変更（P3-4 と重複するが、P3-1 が先行する場合はここで直し、P3-4 側は差分が無ければスキップ）。`name` は現状維持（`pdf` / `pdf-qa` / `pdf-summary` / `pdf-to-llm`）。
2. 各 `description` を「トリガを含む1〜2文」に整える（下記の完全版を参照）。
3. 本文から `## Usage` の `/name ...` ブロックを削除し、`## When to use` と `## How to derive arguments from the request`（自然言語→パス/オプション抽出）に置き換える。
4. すべてのコマンド例で `$PDF_PATH` / `$MAX_TOKENS` / `$OVERLAP` / `$STRATEGY` を撤廃し、`<PDF_PATH>` 等の**説明用プレースホルダ**＋「実際のパスに差し替えて実行する」指示に変える。Python 例も `pdflay.analyze("<PDF_PATH>", ...)` の形にし「文字列を実パスへ置換」と明記。
5. phantom `--format` に依存する記述を除去（詳細は P3-2 と重複。P3-1 では `--format` を含む Usage 行を消すところまで行い、実サブコマンド整合の網羅チェックは P3-2 で完了させる）。
6. 4ファイルの `## Section Selection` / `## Token Estimates` 等の末尾ドキュメントは事実であれば残す（`--section` 部分一致は実装済み・実在。トークン概算 ~4/~1.5 も現行 `chunker.rs` の値と一致するので保持可）。

**完全な before/after（`.claude/skills/pdf/SKILL.md` 全文）:**

Before（現行、要旨。1–89行）:
```markdown
---
name: pdf
description: Analyze academic paper PDF to extract structured content (sections, figures, tables) using pdf-lay
argument-hint: "<pdf-path> [--format markdown|json|toc] [--section NAME]"
allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python *), Read
---

# PDF Structure Analysis
...
## Usage
    /pdf paper.pdf                          # Full Markdown output (default)
    /pdf paper.pdf --format toc             # Table of contents with token estimates
    /pdf paper.pdf --format json            # Full JSON output
    /pdf paper.pdf --section "Introduction" # Extract specific section only
...
pdf-lay toc "$PDF_PATH"
pdf-lay markdown "$PDF_PATH" --no-page-numbers
...
doc = pdflay.analyze("$PDF_PATH", extract_images=False)
print(doc.to_json())
```

After（このタスクで書くべき全文。P3-4 の `python3` も織り込み済み）:
```markdown
---
name: pdf
description: Extract structured content from an academic-paper PDF using the pdf-lay CLI (or its pdflay Python bindings). Use this when the user gives a path to a PDF and asks for its structure, a table of contents, a specific named section, or a Markdown conversion. The model derives the PDF path and the desired output from the request in natural language — there is no slash command and no shell-variable substitution.
allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python3 *), Read
---

# PDF Structure Analysis

Analyze an academic-paper PDF with pdf-lay and return structured content
(table of contents, full Markdown, a selected section, or JSON).

## When to use

Invoke when the user supplies a PDF file path and wants any of:
- the section hierarchy / table of contents,
- the whole document as Markdown,
- one or more specific sections,
- a JSON dump of the document structure.

## How to derive arguments from the request

Read these from the user's natural-language message (nothing is substituted for you):
- **PDF path**: the file the user names. Substitute it literally into the commands below
  in place of `<PDF_PATH>`.
- **Desired output**: pick the matching command below. There is NO `--format` flag —
  the output kind is chosen by the pdf-lay *subcommand* (`toc` vs `markdown`) or by the
  Python API. Do not pass `--format`.
- **Section filter**: if the user names sections, pass one `--section <NAME>` per section
  (the flag is repeatable; matching is case-insensitive partial match).

## Detect availability

Try in order; use the first that works (substitute the real path, do not run `$VAR`):
- `pdf-lay --version`  (CLI on PATH)
- `cargo run -p pdf-lay-cli -- --version`  (from a checkout of the pdf-lay repo)
- `python3 -c "import pdflay; print('ok')"`  (Python bindings)

If none work, tell the user to install pdf-lay:
> - CLI from source: `cargo install --path crates/pdf-lay-cli`
> - Python: `python3 -m venv .venv && source .venv/bin/activate && \
>   maturin develop -m crates/pdflay-python/Cargo.toml`
>   (there is no `pip install pdflay` — the package is not published on PyPI)

## Run analysis

Table of contents:
    pdf-lay toc <PDF_PATH>

Full Markdown (default):
    pdf-lay markdown <PDF_PATH> --no-page-numbers

Selected section(s):
    pdf-lay markdown <PDF_PATH> --section "Introduction" --no-page-numbers
    pdf-lay markdown <PDF_PATH> --section "Results" --section "Discussion" --no-page-numbers

JSON (via Python bindings; the CLI has no json subcommand yet):
    python3 -c "import pdflay; print(pdflay.analyze('<PDF_PATH>', extract_images=False).to_json())"

## Present results

- TOC: show the table (level, header, page range, ~tokens, fig/tab counts).
- Markdown: show the output; if it exceeds ~300 lines, show the TOC first and ask which
  section to expand.
- JSON: save to a file and report the path, or summarize.
- Always surface warnings pdf-lay prints to stderr.

## Section name matching

pdf-lay matches section names case-insensitively by partial match:
- "intro"  → "Introduction", "1. Introduction", "I. INTRODUCTION"
- "method" → "Methods", "Methodology"
- "result" → "Results", "Results and Discussion"
```

（`JSON (via Python)` の行は P2-6 で `pdf-lay json` が実装されたら CLI 版に差し替える。それまでは Python 経由が唯一の実経路。）

**他3ファイルの frontmatter + 本文構造（このタスクで書くべき指針）:**

- **`pdf-qa/SKILL.md`**
  - frontmatter:
    ```yaml
    name: pdf-qa
    description: Answer a question about an academic-paper PDF by extracting the relevant sections with pdf-lay and grounding the answer in the paper. Use when the user gives a PDF path plus a question about the paper's method, results, background, limitations, etc. Derive the PDF path and the question from the request; no slash command, no variable substitution.
    allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python3 *), Read
    ```
  - 本文: `## Usage`（`/pdf-qa ...`）を削除。`## When to use` ＋「質問タイプ→対象セクション」表（現行の表は保持）＋実コマンド（`pdf-lay toc <PDF_PATH>` / `pdf-lay markdown <PDF_PATH> --section "SECTION" --no-page-numbers`）＋Python 複数セクション例（`pdflay.analyze("<PDF_PATH>", extract_images=False).select_sections([...]).to_llm_text(...)`）。`$PDF_PATH` を全廃。回答フォーマット（Answer / Evidence）は保持。

- **`pdf-summary/SKILL.md`**
  - frontmatter:
    ```yaml
    name: pdf-summary
    description: Summarize an academic-paper PDF by extracting its key sections with pdf-lay and composing a structured summary. Use when the user gives a PDF path and wants a summary; honor any requested language (ja/en) and depth (brief/standard/detailed) expressed in natural language. No slash command, no variable substitution.
    allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python3 *), Read
    ```
  - 本文: `## Usage`（`/pdf-summary ...`）削除。`--lang` / `--depth` は**フラグではなく自然言語の希望**として扱う旨を明記（「ユーザーが日本語を望めば日本語で出す」等）。`$PDF_PATH` を全廃し `<PDF_PATH>` に。抽出コマンド（`pdf-lay toc` ＋ `pdf-lay markdown <PDF_PATH> --section "Abstract" --no-page-numbers` …）と Python 一括（`select_sections([...]).to_llm_text()`）を保持。要約フォーマット（Purpose/Approach/Key Findings/Significance）は保持。

- **`pdf-to-llm/SKILL.md`**
  - frontmatter:
    ```yaml
    name: pdf-to-llm
    description: Convert an academic-paper PDF into LLM-ready text chunks (for RAG or context-window input) using pdf-lay. Use when the user gives a PDF path and wants chunked or LLM-optimized text; read max-tokens / overlap / strategy from the request as natural language. Prefer the pdflay Python bindings for chunking. No slash command, no variable substitution.
    allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python3 *), Read
    ```
  - 本文: `## Usage`（`/pdf-to-llm ...`）削除。`$MAX_TOKENS` / `$OVERLAP` / `$STRATEGY` を**すべて撤廃**し、「ユーザーが 2000 と言えば `max_tokens=2000` を渡す」という指示に変える。Python 例は実シグネチャに合わせて数値リテラルを使う:
    ```python
    import pdflay
    doc = pdflay.analyze("<PDF_PATH>", extract_images=False)
    chunks = doc.to_chunks(max_tokens=4000, overlap=200, strategy="section")
    sel = doc.select_sections(["Methods", "Results"])
    chunks = sel.to_chunks(max_tokens=4000, overlap=200)   # NOTE: selector.to_chunks has no `strategy` arg
    text = sel.to_llm_text(include_figures=True, include_tables=True)
    ```
    - **正確性の注意（実 API と一致させること）:** `PyPaperDocument.to_chunks` は `strategy` 引数を持つが、`PySectionSelector.to_chunks` は **`max_tokens` と `overlap` のみ**（`strategy` 無し。`crates/pdflay-python/src/lib.rs:365` の `#[pyo3(signature = (max_tokens = 4000, overlap = 200))]`）。案内文でセレクタ側に `strategy=` を渡さないこと。
    - CLI フォールバックは現状 `pdf-lay markdown <PDF_PATH> --section ... --no-page-numbers`（手動チャンク）。P2-6 マージ後は `pdf-lay chunks <PDF_PATH> --max-tokens 4000 --overlap 200 --strategy section`（JSONL）に差し替える（P3-2 で対応）。

**成果物（新規/変更ファイル）:**
- 変更: `.claude/skills/pdf/SKILL.md`, `.claude/skills/pdf-qa/SKILL.md`, `.claude/skills/pdf-summary/SKILL.md`, `.claude/skills/pdf-to-llm/SKILL.md`
- 新規: なし（`.claude/commands/*` は本タスク非スコープ）

**受け入れ基準（Given/When/Then）:**
- Given 4つの `SKILL.md`、When frontmatter を検査、Then `argument-hint` キーが1つも存在せず、`name` と `description` が全ファイルに存在する。
- Given 4つの `SKILL.md` 本文、When `$PDF_PATH` / `$MAX_TOKENS` / `$OVERLAP` / `$STRATEGY` を全文検索、Then 0件。
- Given 4つの `SKILL.md` 本文、When `/pdf` `/pdf-qa` `/pdf-summary` `/pdf-to-llm` の**スラッシュ起動案内**を検索、Then Usage としての起動案内が0件（説明文中でスキル名に触れるのは可だが `/name paper.pdf` 形式のコマンド行は無い）。
- Given 各 `description`、When 読む、Then 「いつ使うか（トリガ）」が1文以上含まれる。
- Given `pdf-to-llm/SKILL.md` の Python 例、When `sel.to_chunks(` を確認、Then `strategy=` 引数が渡されていない（実 API と一致）。

**検証:**
- `grep -RnE '\$PDF_PATH|\$MAX_TOKENS|\$OVERLAP|\$STRATEGY|argument-hint|--format' .claude/skills/` が空になること。
- Claude Code でリポジトリを開き、スキル一覧にロードされること（`/help` あるいはスキル選択UIで `pdf` `pdf-qa` `pdf-summary` `pdf-to-llm` の4つが description 付きで見えること）。
- 本文中のすべての `pdf-lay ...` 行を、実際に `pdf-lay <subcommand> --help` と照合し、フラグ・サブコマンドが実在することを確認（`toc`/`markdown` のみ、`--format` 無し）。
- Python 例をコピーして `python3` で構文が通ること（`maturin develop` 済み環境で `import pdflay` → 各メソッド呼び出しが AttributeError にならないこと）。

**非スコープ:** `.claude/commands/*` の新設、CLI へのサブコマンド追加（P2-6）、`.codex/prompts/*` と `.devin/skills.md` の内容変更（後者は P3-2/P3-4 で扱う）。

---

### P3-2: phantom `--format` の除去 / 実サブコマンドへの整合

**目的:** 実在しない `--format`（および将来サブコマンドの誤記）を全案内文から除去し、実 CLI（`toc`/`markdown` ＋ P2-6 の `json`/`chunks`/`llm-text`）に1対1で整合させる。

**重大度 / 依存:** MED / P2-6（`json`/`chunks`/`llm-text` サブコマンドと `PyChunk.tables` getter を参照する箇所）。P2-6 未マージ時は `toc`/`markdown` のみで整合させ、新サブコマンド追記は追補 PR に回す。

**対象ファイル:**
- `.claude/skills/pdf/SKILL.md:4,15-19,77-81`
- `.codex/prompts/pdf.md:3,13-16`
- `.claude/skills/pdf-to-llm/SKILL.md:34-38,55-59`（CLI フォールバック）
- `.codex/prompts/pdf-to-llm.md:35-39`（CLI フォールバック）
- `.devin/skills.md:79-83`（CLI フォールバック）
- （P3-1 実施後は `.claude/skills/pdf/SKILL.md` の `--format` は既に消えている。P3-2 では**残り全箇所**を潰す）

**現状（問題）— phantom を含む全箇所:**

| ファイル:行 | 現物 | 欠陥 |
|-------------|------|------|
| `.claude/skills/pdf/SKILL.md:4` | `argument-hint: "... [--format markdown\|json\|toc] ..."` | `--format` は実在しない。`argument-hint` 自体も無効（P3-1で削除） |
| `.claude/skills/pdf/SKILL.md:16-18` | `/pdf paper.pdf --format toc` / `--format json` | phantom フラグ＋slash案内 |
| `.claude/skills/pdf/SKILL.md:77-81` | 出力表 `toc`=CLI / `markdown`=CLI / `json`=Python | `json` が Python 限定な事実自体は正しいが、`--format` を前提にした文脈に紐づく |
| `.codex/prompts/pdf.md:3` | `argument-hint: "... [--format markdown\|json\|toc] ..."` | phantom（Codex prompt では `argument-hint` は有効だが、値の `--format` は実在しない） |
| `.codex/prompts/pdf.md:13-16` | `/pdf paper.pdf --format toc` / `--format json` | phantom フラグ |
| `.claude/skills/pdf-to-llm/SKILL.md:55-59` | CLI fallback が `markdown --section` のみで「手で分割」 | P2-6 後は `chunks`/`llm-text` が実在するのに未反映 |

実 CLI サブコマンド（`crates/pdf-lay-cli/src/main.rs:164-178`）= `Toc`, `Markdown` の2つのみ。`markdown` の実フラグ = `--section`（NAME・繰り返し可）, `--heading-offset N`, `--no-page-numbers`, `--image-base PATH`, `-o/--output FILE`, `--image-dir DIR`, `--no-extract-images`。**`--format` は存在しない。**

**変更後の期待動作:** すべての案内文で出力形式は**サブコマンド**で表す。
- TOC → `pdf-lay toc <PDF>`
- Markdown（全体/セクション）→ `pdf-lay markdown <PDF> [--section NAME]... [--no-page-numbers]`
- JSON → （現状）Python `analyze(...).to_json()`／（P2-6後）`pdf-lay json <PDF>`
- チャンク → （現状）Python `to_chunks(...)`／（P2-6後）`pdf-lay chunks <PDF> --max-tokens N --overlap N --strategy section`（JSONL）
- LLMテキスト → （現状）Python `to_llm_text(...)`／（P2-6後）`pdf-lay llm-text <PDF> --section NAME ...`

**実装手順:**
1. `.codex/prompts/pdf.md` の frontmatter `argument-hint`（3行目）から `--format markdown|json|toc` を削除。`argument-hint: "<pdf-path> [--section NAME]"` に変更（Codex prompt は `argument-hint` 有効・`$1` 有効なので slash 形式のまま残してよい。ただし phantom フラグは消す）。
2. `.codex/prompts/pdf.md:13-16` の Usage から `--format toc` / `--format json` の行を削除し、代わりに「TOC は `pdf-lay toc "$1"`、JSON は Python 経由」と本文（既に 28-31 行に正しい実コマンドがある）に一本化。
3. P3-1 で書き直した `.claude/skills/pdf/SKILL.md` に `--format` 残存が無いことを確認（P3-1 完了後なら既に無い。単独で P3-2 を先行する場合はここで除去）。
4. `.claude/skills/pdf/SKILL.md` の出力表（現 77-81 行相当）を「Subcommand / Source / Content」に整え、`toc`→CLI、`markdown`→CLI、`json`→Python（P2-6後は CLI）と明記。
5. **（P2-6 マージ後のみ）** `pdf-to-llm` の CLI フォールバック（`.claude/skills/pdf-to-llm/SKILL.md` と `.codex/prompts/pdf-to-llm.md` と `.devin/skills.md`）を `pdf-lay chunks <PDF> --max-tokens N --overlap N --strategy section`（JSONL 出力）に更新。`PyChunk.tables` getter が追加されたら、チャンク提示項目にテーブル数を含めてよい。**この手順5は P2-6 未マージなら実施せず、PR 説明に「P2-6 待ち」と記す。**
6. 実装後、**すべての案内された CLI 行**を機械的に抽出し `pdf-lay --help` / `pdf-lay <sub> --help` と突き合わせる（検証節参照）。

**成果物（新規/変更ファイル）:**
- 変更: `.codex/prompts/pdf.md`, `.claude/skills/pdf/SKILL.md`（P3-1未了時のみ）, （P2-6後）`.claude/skills/pdf-to-llm/SKILL.md`, `.codex/prompts/pdf-to-llm.md`, `.devin/skills.md`

**受け入れ基準（Given/When/Then）:**
- Given 全 skill / prompt / devin ファイル、When `--format` を全文検索、Then 0件。
- Given 全案内文中の `pdf-lay <something>` 呼び出し、When `pdf-lay --help` と照合、Then すべてのサブコマンド・フラグが実在する（P2-6 未マージ時は `toc`/`markdown` のみ登場）。
- Given `.codex/prompts/pdf.md`、When frontmatter を見る、Then `argument-hint` に phantom フラグが無い。
- Given P2-6 マージ済みで手順5実施後、When `pdf-lay chunks --help` を確認、Then 案内文の `--max-tokens`/`--overlap`/`--strategy` が実フラグ名と一致する。

**検証:**
- `grep -Rn -- '--format' .claude/ .codex/ .devin/` が空。
- 案内文の CLI 行の抽出＆照合（例）:
  ```bash
  grep -RhoE 'pdf-lay [a-z-]+[^`]*' .claude/skills .codex/prompts .devin/skills.md | sort -u
  # 出力に現れるサブコマンドが `pdf-lay --help` の Commands 節に全て存在することを目視/スクリプトで確認
  cargo run -p pdf-lay-cli -- --help
  cargo run -p pdf-lay-cli -- markdown --help   # --format が無いことを確認
  ```
- 実 PDF で1つ実行（`pdf-lay toc tests/fixtures/<sample>.pdf`）し、エラーにならないこと。

**非スコープ:** CLI 実装（P2-6）、frontmatter の `argument-hint`/`python3` 以外の整形（P3-1/P3-4）、仕様書の同期（P3-5）。

---

### P3-3: 配布パッケージ化（plugin + marketplace、または global install 手順）

**目的:** skills をこのリポジトリを cwd にしていない環境でもロードできるようにし、「再利用可能なスキル」の主張（`.devin/skills.md:1-3`）を実体化する。

**重大度 / 依存:** MED / P3-1（整形済みの正しい skill が前提。壊れた skill を配布しない）。

**対象ファイル:**
- 新規 `.claude-plugin/plugin.json`
- 新規 `.claude-plugin/marketplace.json`
- 変更 `README.md`（インストール手順に「スキルの導入」節を追加）
- 参照 `.claude/skills/*`（配布対象。移動はしない — 下記前提参照）

**現状（問題）:** リポジトリに `plugin.json` も `.claude-plugin/` も存在しない（確認済み）。skills は `.claude/skills/` にあるため、Claude Code が**このディレクトリを cwd/プロジェクトとして開いた時にだけ**ロードされる。別プロジェクトで論文PDFを扱う際には `pdf` スキルが出てこない（K5）。

**変更後の期待動作:** 次の2経路のいずれかで、任意の cwd から4スキルが利用可能になる。
- **経路A（推奨・配布）:** このリポジトリを Claude Code の **plugin marketplace** として登録 → `pdf-lay` プラグインをインストールすると4スキルが常時ロードされる。
- **経路B（フォールバック・手動）:** `.claude/skills/*` を `~/.claude/skills/` にコピーする手順を README に明記（グローバルスキルとして常時ロード）。

> **スキーマの前提（レビュー方針 §7 に基づく明示）:** Claude Code のプラグイン/マーケットプレイス機構について、以下を仮定する。**実装前に現行 Claude Code のプラグイン仕様（`/plugin` コマンドのヘルプ、または公式ドキュメント）で各キー名を必ず確認し、差異があれば PR 説明の「レビュアーへの質問」に記載してから合わせること。推測で確定しない。**
> - プラグインのマニフェストは **`.claude-plugin/plugin.json`**（プラグインルート直下の `.claude-plugin/` 内）。
> - マーケットプレイス定義は **`.claude-plugin/marketplace.json`**（マーケットプレイスとして公開する git リポジトリのルートの `.claude-plugin/` 内）。
> - プラグインは配下の `skills/`（および `commands/`, `agents/`, `hooks/`）を自動検出する。**現行の skills は `.claude/skills/` にあるため**、プラグインからの検出のために (i) `skills/` へ移動する、または (ii) plugin.json でスキルディレクトリを明示できるなら `.claude/skills` を指す、のいずれかが要る。移動はリポジトリ内での cwd ローカル・ロード（P3-1 の検証で使う経路）を壊すため、**まず「plugin.json でパスを指定できるか」を確認し、できなければ `skills/` への移動＋`.claude/skills` からのシンボリックリンク等で両立させる**。この判断は確認結果に依存するため、確定は実装時に行い PR に明記する。

**実装手順:**

1. `.claude-plugin/plugin.json` を作成（スケルトン。キー名は上記前提のとおり実仕様で確認のうえ確定）:
   ```json
   {
     "name": "pdf-lay",
     "description": "Academic-paper PDF analysis skills (structure, Q&A, summary, LLM chunking) powered by the pdf-lay CLI and pdflay Python bindings.",
     "version": "0.1.0",
     "author": { "name": "sonoh5n" },
     "homepage": "https://github.com/sonoh5n/pdf-lay",
     "keywords": ["pdf", "academic", "rag", "llm", "chunking"]
   }
   ```
2. プラグインがスキルを検出できる配置にする（前提の (i)/(ii) の確認結果に従う）。既定案: `skills/pdf/SKILL.md` 等をプラグインの検出パスに置く。リポジトリ内 cwd ローカル・ロードを維持するため `.claude/skills/*` を残す必要があるなら、二重管理を避けてどちらかを正とし他方をリンク/コピーする方針を README とコミットに明記。
3. `.claude-plugin/marketplace.json` を作成（このリポジトリを1プラグインだけ持つマーケットプレイスとして公開）:
   ```json
   {
     "name": "pdf-lay-marketplace",
     "owner": { "name": "sonoh5n" },
     "plugins": [
       {
         "name": "pdf-lay",
         "source": "./",
         "description": "PDF analysis skills for academic papers (pdf, pdf-qa, pdf-summary, pdf-to-llm)."
       }
     ]
   }
   ```
   （`source` の表現＝ローカル相対パス or `{ "source": "github", "repo": "sonoh5n/pdf-lay" }` 形式か、は実仕様で確認して確定する。）
4. `README.md` に「### Claude Code スキルの導入」節を追加し、**経路A**（`/plugin marketplace add sonoh5n/pdf-lay` → `/plugin install pdf-lay@pdf-lay-marketplace`）と、**経路B**（手動グローバル導入）を併記:
   ```bash
   # 経路B（フォールバック）: グローバルにスキルをインストール
   mkdir -p ~/.claude/skills
   cp -R .claude/skills/pdf ~/.claude/skills/
   cp -R .claude/skills/pdf-qa ~/.claude/skills/
   cp -R .claude/skills/pdf-summary ~/.claude/skills/
   cp -R .claude/skills/pdf-to-llm ~/.claude/skills/
   # 以後、任意の cwd で pdf / pdf-qa / pdf-summary / pdf-to-llm が利用可能
   ```
5. 経路Aは `pdf-lay` CLI か `pdflay`（Python）が別途 PATH/環境に必要。README のスキル導入節に「スキルはあくまで pdf-lay 本体のラッパー。先に CLI か Python バインディングを導入すること」と依存を明記（`allowed-tools` が CLI/`python3` を叩くため）。

**成果物（新規/変更ファイル）:**
- 新規: `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json`（＋前提 (i) を採る場合 `skills/` 配下のスキル、または `.claude/skills` からのリンク）
- 変更: `README.md`

**受け入れ基準（Given/When/Then）:**
- Given 別ディレクトリ（pdf-lay リポジトリ外）で Claude Code を起動、When 経路A または経路B で導入、Then `pdf` `pdf-qa` `pdf-summary` `pdf-to-llm` の4スキルがスキル一覧に現れる。
- Given `.claude-plugin/plugin.json` と `marketplace.json`、When JSON パーサで読む、Then 妥当な JSON で必須キー（少なくとも `name`）が揃っている。
- Given README のスキル導入節、When 手順どおり実行、Then コピー/インストールが成功し、依存（CLI か Python バインディング）の必要性が明記されている。

**検証:**
- `python3 -c "import json,sys; json.load(open('.claude-plugin/plugin.json')); json.load(open('.claude-plugin/marketplace.json')); print('json ok')"`
- 実機確認: pdf-lay リポジトリ外の一時ディレクトリで Claude Code を開き、経路B（`~/.claude/skills` へコピー）でスキルが列挙されること。可能なら経路A（`/plugin marketplace add` → `/plugin install`）も試す。
- `/plugin`（または現行の相当コマンド）のヘルプでキー名・ディレクトリ規約を確認し、スケルトンと差異が無いこと。

**非スコープ:** skills 本文の書き換え（P3-1）、CLI/Python の配布（既存の `scripts/install.sh` と `maturin` は現状維持）、CI での自動公開。

---

### P3-4: インタプリタ / インストール整合（python3 統一・maturin 主手順化）

**目的:** `python`→`python3` に統一し、`allowed-tools` を実インタプリタに合わせ、存在しない `pip install pdflay` を撤廃して `maturin develop` を主インストール経路にする。

**重大度 / 依存:** MED / P3-1（同一 frontmatter を編集。P3-1 が先行マージ。P3-1 で `Bash(python *)`→`Bash(python3 *)` を既に直していれば、本タスクは本文と README とドキュメントの `python`/`pip install` を潰す差分に集中する）。

**対象ファイル:**
- `.claude/skills/pdf/SKILL.md:5,36,42,62-64`（`allowed-tools`・`python`・`pip install pdflay`）
- `.claude/skills/pdf-qa/SKILL.md:5,54`
- `.claude/skills/pdf-summary/SKILL.md:5,46`
- `.claude/skills/pdf-to-llm/SKILL.md:5,28,31`
- `.codex/prompts/pdf.md:24,31`（`python -c "import pdflay"`）
- `.devin/skills.md:10`（`pip install pdflay`）, `:37,55,110`（`python` 例）
- `README.md`（既に `python3`／`maturin` 主体だが、`pip install pdflay` への言及が無いことを確認し、Python 節に「PyPI 未公開」を1文明記）
- 参照: `AGENTS.md:78-80`（`maturin develop` / `python -c ...`。AGENTS は本フェーズの主対象外だが `python`→`python3` の整合として任意で直してよい。ただし別スコープになるので原則触らない）

**現状（問題）:**

- `allowed-tools: ... Bash(python *), Read`（4つの SKILL.md の5行目）— `python` しか許可していない。`python3` のみの環境で `python3 -c ...` を弾く（K6）。
- `.claude/skills/pdf/SKILL.md:42`:
  `> - Python: \`pip install pdflay\` or \`cd crates/pdflay-python && maturin develop\``
  — **`pip install pdflay` は PyPI に存在しない**（`pypi.org/pypi/pdflay/json` が 404 ＝確認済み）。
- `.devin/skills.md:10`:
  `- **Python**: \`import pdflay\` (install via \`pip install pdflay\` or \`maturin develop\`)` — 同上。
- 本文の Python 実行が `python`（`.codex/prompts/pdf.md:24` の `python -c "import pdflay"`、`.claude/skills/pdf/SKILL.md:36` の `python -c "import pdflay; print('ok')"`）。README は `python3`（`README.md:112-114`）で不一致。
- README のインストールは既に `maturin develop -m crates/pdflay-python/Cargo.toml` を主にしているが（`README.md:109-121`）、「`pip install pdflay` は使えない」旨の明示が無いため、SKILL/devin の誤案内と衝突する。

**変更後の期待動作:**
- すべての skill/prompt/devin で `python` → `python3`。
- `allowed-tools` は `Bash(python3 *)`（`Bash(python *)` を置換。両対応にしたければ `Bash(python3 *), Bash(python *)` の順で併記可だが、`python3` を第一とする）。
- `pip install pdflay` を撤廃し、Python 導入は `python3 -m venv .venv && source .venv/bin/activate && maturin develop -m crates/pdflay-python/Cargo.toml`（README と一致）を主手順にする。将来 PyPI 公開されたら `pip install pdflay` を戻す旨をコメントで残してもよいが、現状は書かない。

**実装手順:**
1. 4つの `SKILL.md` の `allowed-tools` を `Bash(pdf-lay *), Bash(cargo run *), Bash(python3 *), Read` に統一（P3-1 で済んでいれば差分なし）。
2. 4つの `SKILL.md` 本文の `python -c ...` / `python "..."` をすべて `python3 ...` に置換。
3. `.claude/skills/pdf/SKILL.md:42` のインストール案内から `pip install pdflay or` を削除し、
   `> - Python: \`python3 -m venv .venv && source .venv/bin/activate && maturin develop -m crates/pdflay-python/Cargo.toml\`` に置換。他の SKILL.md に同種の案内があれば同様に。
4. `.codex/prompts/pdf.md:24,31` の `python` → `python3`。
5. `.devin/skills.md:10` の `pip install pdflay or ` を削除し `maturin develop -m crates/pdflay-python/Cargo.toml` に統一。`:37,55,110` 等の `python` コードフェンス言語指定は `python` のままでよい（コードブロックの言語ラベルであり実行インタプリタではない）が、**実行コマンドとしての `python -c`** があれば `python3 -c` にする。
6. `README.md` の「### Python Binding」節末尾に1文追加:
   `> Note: \`pdflay\` はまだ PyPI に公開されていません（\`pip install pdflay\` は使えません）。上記の \`maturin develop\` を使ってください。`

**成果物（新規/変更ファイル）:**
- 変更: `.claude/skills/pdf/SKILL.md`, `.claude/skills/pdf-qa/SKILL.md`, `.claude/skills/pdf-summary/SKILL.md`, `.claude/skills/pdf-to-llm/SKILL.md`, `.codex/prompts/pdf.md`, `.devin/skills.md`, `README.md`

**受け入れ基準（Given/When/Then）:**
- Given 全 skill/prompt/devin、When `pip install pdflay` を全文検索、Then 0件。
- Given 全 skill/prompt/devin の**実行コマンド**、When `python ` （末尾スペース）を検索、Then `python3` 以外の裸の `python` 実行が0件（コードフェンスの ```python ラベルは除く）。
- Given 4つの `SKILL.md` の `allowed-tools`、When 確認、Then すべて `Bash(python3 *)` を含む。
- Given `README.md` Python 節、When 読む、Then PyPI 未公開の注意書きが1文以上ある。

**検証:**
- `grep -Rn 'pip install pdflay' .claude/ .codex/ .devin/ README.md` が空。
- `grep -RnE 'python -c|python "' .claude/ .codex/ .devin/` が空（`python3 -c` のみ残る）。
- `python3 -m venv /tmp/venv && . /tmp/venv/bin/activate && maturin develop -m crates/pdflay-python/Cargo.toml && python3 -c "import pdflay; print('ok')"` が通ること。
- `allowed-tools` に列挙した `python3` で本文の Python 例が実行できること。

**非スコープ:** P3-1 の本文構造変更、`AGENTS.md` の整備（原則触らない）、PyPI 公開作業そのもの。

---

### P3-5: 仕様書 `01_SPECIFICATION.md` を実装に同期

**目的:** 仕様書が約束するが未実装の CLI/API を「実装済み」と「計画中（未実装）」に区分し、虚偽の記述を修正して「仕様書＝実装契約」を回復する。

**重大度 / 依存:** MED / 参照のみ（P2-6 の最終サブコマンド名が確定したら「計画中→実装済み」に移す）。docs タスクだが、どの行のどの主張が**現時点で偽**かを厳密に列挙する。

**対象ファイル:**
- `docs/arch/01_SPECIFICATION.md`（下表の各行）

**現状（問題）— 現時点で偽の主張の網羅リスト:**

| 箇所（行） | 記述 | 実装の現実 | 区分 |
|-----------|------|-----------|------|
| `01_SPECIFICATION.md:593` | `pdf-lay markdown ... --sections "RESULTS,EXPERIMENTS"`（**カンマ区切りの複数指定**） | 実 CLI は `--section NAME`（**単数・繰り返し可**、カンマ分割なし。`main.rs:257-268`）。`--sections` というフラグは無い | 偽（要修正） |
| `01_SPECIFICATION.md:596,1091` | `pdf-lay markdown ... --section-index 3,4` | `--section-index` フラグは未実装 | 未実装（計画中に区分） |
| `01_SPECIFICATION.md:599,1094` | `pdf-lay llm-text paper.pdf --sections "RESULTS" --include-tables --include-figures` | `llm-text` サブコマンド未実装（P2-6 で追加予定） | 未実装（P2-6 後に実装済みへ） |
| `01_SPECIFICATION.md:1071` | `pdf-lay analyze paper.pdf -o output/` | `analyze` サブコマンド未実装 | 未実装（計画中） |
| `01_SPECIFICATION.md:1101` | `pdf-lay json paper.pdf -o paper.json` | `json` サブコマンド未実装（P2-6 で追加予定） | 未実装（P2-6 後に実装済みへ） |
| `01_SPECIFICATION.md:1104` | `pdf-lay analyze papers/*.pdf -o output/ --parallel 4`（バッチ） | バッチ処理・`--parallel` 未実装 | 未実装（計画中） |
| `01_SPECIFICATION.md:1107-1109` | `pdf-lay markdown ... --math-format latex\|unicode\|plain` | `--math-format` 未実装（Phase 0 P0-3 で導入予定） | 未実装（Phase 0 後に実装済みへ） |
| `01_SPECIFICATION.md:1112` | `pdf-lay debug-layout paper.pdf -o debug/ --page 2` | `debug-layout` 未実装（`AGENTS.md:468,475` も同コマンドを前提にしているが実在しない） | 未実装（計画中） |
| `01_SPECIFICATION.md:991-1001` | `doc.metadata.title` / `doc.metadata.authors` / `section.text` | 実 Python API に `metadata` 属性は無い。`doc.title` / `doc.authors`（`lib.rs:47-56`）が正。`section.text` は実在（`lib.rs:537`） | 偽（要修正） |
| `01_SPECIFICATION.md:1007,1009,482,1044` | `entry.page_range[0]` / `entry.page_range[1]` | 実 `PySectionEntry` に `page_range` は無い。`page_start` / `page_end`（`lib.rs:440-448`）が正 | 偽（要修正） |
| `01_SPECIFICATION.md:1047-1052` | `pdflay.Config(...)` を作って `analyze("paper.pdf", config=config)` | `pdflay.Config` は未公開クラス。`analyze` は `config` 引数を取らない（`analyze(path, image_dir, extract_images, detect_tables)`、`lib.rs:757-764`） | 偽 / 未実装（計画中） |
| `01_SPECIFICATION.md:1055-1057` | `pdflay.extract_spans` / `reconstruct_lines` / `detect_layout`（段階的 Python API） | いずれも Python 未公開（モジュールは `analyze` と各 Py クラスのみ、`lib.rs:789-797`） | 未実装（計画中） |
| `01_SPECIFICATION.md:1060-1064` | `pdflay.analyze_batch([...], parallel=True)` | 未実装 | 未実装（計画中） |
| `01_SPECIFICATION.md:916-922` | 段階的 Rust API（`extract_text_spans`/`reconstruct_lines`/`detect_layout`/`group_blocks`/`detect_sections`/`extract_images`/`match_figures`） | Rust 側の実在は要確認（`pdf-lay` 再エクスポート）。本タスクでは検証で確認し、未公開なら「計画中」に区分 | 要確認 |
| `01_SPECIFICATION.md:944-946` | `to_markdown`/`to_json`/`to_chunks` の自由関数 | 実際は `MarkdownGenerator`/`JsonGenerator`/`Chunker` 経由（`main.rs:13`, `lib.rs:13`）。自由関数の実在は要確認 | 要確認 |
| `01_SPECIFICATION.md:1050,1107` | 数式 `--math-format` / `math_representation="latex"` を「利用可能」と読める | 実装は全経路 `math_config: None` 固定（`main.rs:415`, `lib.rs:104`）で数式変換は現状デッド（Phase 0 P0-3 対象） | 未実装（Phase 0 後に実装済みへ） |

> 注: 上表の CLI 出力例（`toc` の整形 `p.1-2 ~1200 tokens fig:…`）自体は実 `toc` 出力（`main.rs:384-394`）と概ね一致するので、TOC 例は保持してよい。問題は**サブコマンド/フラグ/Python 属性名**の偽り。

**変更後の期待動作:** 仕様書の §4.3 CLI・§4.2 Python API・§2.14 に「実装状況」を明示し、未実装項目には「（計画中 / Phase X）」を付す。加えて仕様書冒頭付近（§1.3 の後、または §2.1 の前）に**「実装済み vs 計画中」対応表**を新設する。

**実装手順:**
1. 仕様書に新規セクション **`### 1.5 実装状況（Implemented vs Planned）`** を追加し、次の表を挿入する（P2-6 等の進捗に応じて「状態」を更新する運用にする）:

   ```markdown
   ### 1.5 実装状況（Implemented vs Planned）

   本表は「仕様が約束する表面」と「現時点の実装」の対応。✅=実装済み / 🅿=計画中（未実装）。

   | 面 | 項目 | 状態 | 実体 / 追加予定タスク |
   |----|------|------|----------------------|
   | CLI | `pdf-lay toc <PDF>` | ✅ | `main.rs` Toc |
   | CLI | `pdf-lay markdown <PDF>` | ✅ | `main.rs` Markdown |
   | CLI | `markdown --section NAME`（繰り返し可） | ✅ | 単数・repeatable。`--sections "A,B"` は**無い** |
   | CLI | `markdown --heading-offset / --no-page-numbers / --image-base / -o / --image-dir` | ✅ | `main.rs` MarkdownArgs |
   | CLI | `markdown --section-index` | 🅿 | 未実装 |
   | CLI | `pdf-lay json <PDF>` | 🅿 | Phase 2 P2-6 |
   | CLI | `pdf-lay chunks <PDF>`（JSONL） | 🅿 | Phase 2 P2-6 |
   | CLI | `pdf-lay llm-text <PDF>` | 🅿 | Phase 2 P2-6 |
   | CLI | `markdown --math-format latex\|unicode\|plain` | 🅿 | Phase 0 P0-3 |
   | CLI | `pdf-lay analyze` / `debug-layout` / バッチ `--parallel` | 🅿 | 未計画 or Phase 4 |
   | Python | `pdflay.analyze(path, image_dir, extract_images, detect_tables)` | ✅ | `lib.rs:757` |
   | Python | `doc.title / doc.authors / doc.doi / doc.pages / doc.sections / doc.figures` | ✅ | `lib.rs:35-81`（`doc.metadata.*` は**無い**）|
   | Python | `doc.to_markdown / to_json / to_chunks / toc / select_sections* ` | ✅ | `lib.rs` PyPaperDocument |
   | Python | `PySectionEntry.page_start / page_end` | ✅ | `page_range[..]` は**無い** |
   | Python | `selector.to_markdown / to_json / to_llm_text / to_chunks / total_estimated_tokens` | ✅ | `lib.rs` PySectionSelector |
   | Python | `PyChunk.tables` getter | 🅿 | Phase 2 P2-6（現状 `figures` のみ）|
   | Python | `pdflay.Config(...)` / `analyze(..., config=)` | 🅿 | 未実装 |
   | Python | `extract_spans / reconstruct_lines / detect_layout / analyze_batch` | 🅿 | 未実装 |
   | 出力 | 数式変換（`--math-format` / `math_representation`） | 🅿 | 全経路 `math_config: None` 固定。Phase 0 P0-3 |
   ```

2. §4.3 CLI（1067–1113 行）の未実装コマンド行に `# 🅿 計画中（Phase X）` の注記を付す。`--sections "A,B"`（593,1088 行）は `--section NAME`（繰り返し）に**書き換える**（これは偽記述の訂正であり注記では不十分）。
3. §4.2 Python API（982–1065 行）の `doc.metadata.title`→`doc.title`、`doc.metadata.authors`→`doc.authors`、`entry.page_range[0]`→`entry.page_start` 等、**属性名の偽りを実名に訂正**。`pdflay.Config` / `extract_spans` / `analyze_batch` の各ブロックには `# 🅿 計画中（未実装）` を付す（または「将来 API」節へ隔離）。
4. §2.14（444–609 行）内の `entry.page_range[...]`（482 行）と CLI 例（593–599 行）も同様に訂正/注記。
5. P2-6・P0-3 がマージされた時点で、対応表の 🅿 を ✅ に更新し、CLI/Python 例から注記を外す追補コミットを行う（本 PR では現時点の真偽に忠実にする）。
6. Rust 段階的 API（916–946 行）の実在は検証で確認し、未公開なら 🅿 に、実在するなら ✅ のまま残す（推測で断定しない）。

**成果物（新規/変更ファイル）:**
- 変更: `docs/arch/01_SPECIFICATION.md`（新規 §1.5 表＋§2.14/§4.2/§4.3 の訂正・注記）

**受け入れ基準（Given/When/Then）:**
- Given 仕様書、When §1.5 を探す、Then「実装済み vs 計画中」表が存在し、上表の全項目を網羅する。
- Given 仕様書 §4.2、When `doc.metadata` / `entry.page_range` を検索、Then 実 API に無い属性が「計画中」注記なしで**断定形のまま**残っていない（訂正済み or 明示注記）。
- Given 仕様書 §4.3、When `--sections "` を検索、Then カンマ区切り複数指定の偽記述が `--section`（繰り返し）に訂正されている。
- Given 未実装 CLI（`analyze`/`json`/`llm-text`/`chunks`/`debug-layout`/`--math-format`/`--section-index`/バッチ）、When 各行を見る、Then すべて 🅿 注記が付く。
- Given P2-6/P0-3 未マージ、When `json`/`llm-text`/`chunks`/`--math-format` の状態、Then 🅿 のまま（先取りで ✅ にしない）。

**検証:**
- 仕様書中の全 `pdf-lay ...` 行を抽出し、✅ とされたものだけが `pdf-lay --help` に存在すること、🅿 のものは存在しないことをクロスチェック:
  ```bash
  grep -nE 'pdf-lay [a-z-]+' docs/arch/01_SPECIFICATION.md
  cargo run -p pdf-lay-cli -- --help          # 実在サブコマンド一覧
  ```
- 仕様書中の全 Python 属性/メソッドを `crates/pdflay-python/src/lib.rs` の `#[getter]` / `#[pymethods]` / `#[pyfunction]` 定義と突き合わせ、✅ 記述がすべて実在すること（`doc.title`,`doc.authors`,`page_start`,`page_end` 等）。
- `maturin develop` 済み環境で、✅ とした Python 例が実際に AttributeError を出さないこと（例: `python3 -c "import pdflay; d=pdflay.analyze('tests/fixtures/<sample>.pdf'); print(d.title, d.authors); print(d.toc()[0].page_start)"`）。

**非スコープ:** CLI/Python の**実装**（P2-6/P0-3 が担当。本タスクは docs のみ）、`AGENTS.md` の `debug-layout` 記述訂正（別スコープ。ただし §1.5 表で「未実装」と分かるので相互参照を1行入れてよい）、`02_DESIGN.md` の同期。

---

## フェーズ完了の定義（Definition of Done — Phase 3 全体）

Phase 3 は、レビュー方針 §4 の各タスク DoD に加えて、**次をすべて満たしたときに完了**とする:

1. **すべての起動が実体に対応する:** `.claude/skills/*`, `.codex/prompts/*`, `.devin/skills.md` が案内する**すべての `pdf-lay ...` コマンドと `pdflay` Python 呼び出し**が、`pdf-lay --help` に存在する実サブコマンド/フラグ、または `crates/pdflay-python/src/lib.rs` に存在する実メソッド/属性に**1対1で対応**する。phantom（`--format`、`--sections "A,B"` カンマ形式、`pip install pdflay`、`$PDF_PATH` 等の未置換プレースホルダ）が**全ファイルで0件**。
2. **skill が正しく起動する:** 4つの `SKILL.md` が true Agent Skill（`name`+`description`、`argument-hint` なし、`/name` 起動案内なし、`$VAR` なし）であり、Claude Code のスキル一覧に description 付きでロードされる。
3. **リポジトリ外でロードできる:** plugin+marketplace（経路A）または `~/.claude/skills` グローバル導入（経路B）により、pdf-lay リポジトリを cwd にしない環境でも4スキルが利用可能。
4. **インタプリタ整合:** 実行コマンドは `python3` に統一され、`allowed-tools` が `Bash(python3 *)` を含み、Python 導入手順は実在する `maturin develop` を主経路にしている。
5. **仕様書が契約になっている:** `docs/arch/01_SPECIFICATION.md` に「実装済み vs 計画中」表が存在し、現時点で偽の属性名/フラグ/サブコマンドが訂正または 🅿 注記されている。✅ とされた項目は実 CLI/実 Python API と照合済み。
6. **回帰なし:** レビュー方針 §5 の標準検証（`cargo fmt --check` / `clippy -D warnings` / `cargo test --workspace`）が緑（本フェーズは主に docs/skill 変更だが、CLI/Python に触れていないことを確認）。

> 最終確認スクリプト（全ファイル横断の phantom 検出。緑＝すべて空）:
> ```bash
> grep -RnE '\$PDF_PATH|\$MAX_TOKENS|\$OVERLAP|\$STRATEGY' .claude .codex .devin
> grep -RnE '--format|--sections "' .claude .codex .devin docs/arch/01_SPECIFICATION.md
> grep -Rn 'pip install pdflay' .claude .codex .devin README.md
> grep -RnE 'argument-hint' .claude/skills
> ```
