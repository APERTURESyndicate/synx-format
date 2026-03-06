<p align="center">
  <a href="https://aperturesyndicate.com/branding/aperturesyndicate.png" target="_blank">
    <img src="https://aperturesyndicate.com/branding/aperturesyndicate.png" alt="APERTURESyndicate" width="400" />
  </a>
</p>

> **🔗 [ロゴを表示 →](https://aperturesyndicate.com/branding/aperturesyndicate.png)**

<h1 align="center">SYNX v3.0 — 完全ガイド</h1>

<p align="center">
  <strong>JSONより優れている。YAMLより安い。AIと人間のために作られた。</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-3.0.0-5a6eff?style=for-the-badge" />
  <img src="https://img.shields.io/badge/license-MIT-blue?style=for-the-badge" />
  <img src="https://img.shields.io/badge/format-SYNX-blueviolet?style=for-the-badge" />
  <img src="https://img.shields.io/badge/written_in-Rust-orange?style=for-the-badge" />
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/@aperturesyndicate/synx">npm</a> ·
  <a href="https://pypi.org/project/synx-format/">PyPI</a> ·
  <a href="https://crates.io/crates/synx-core">crates.io</a> ·
  <a href="https://marketplace.visualstudio.com/items?itemName=APERTURESyndicate.synx-vscode">VS Code</a> ·
  <a href="https://github.com/kaiserrberg/synx-format">GitHub</a>
</p>

---

## 目次

- [設計思想](#-設計思想)
- [デモンストレーション](#-デモンストレーション)
- [仕組み](#-仕組み)
- [パフォーマンスとベンチマーク](#-パフォーマンスとベンチマーク)
- [インストール](#-インストール)
- [構文リファレンス](#-構文リファレンス)
  - [基本構文](#基本構文)
  - [ネスト](#ネスト)
  - [リスト](#リスト)
  - [型変換](#型変換)
  - [複数行テキスト](#複数行テキスト)
  - [コメント](#コメント)
- [アクティブモード (`!active`)](#-アクティブモード-active)
- [マーカー完全リファレンス](#-マーカー完全リファレンス)
  - [:env — 環境変数](#env--環境変数)
  - [:default — デフォルト値](#default--デフォルト値)
  - [:calc — 算術式](#calc--算術式)
  - [:random — ランダム選択](#random--ランダム選択)
  - [:alias — 別キーの参照](#alias--別キーの参照)
  - [:secret — 隠し値](#secret--隠し値)
  - [:template — 文字列補間](#template--文字列補間)
  - [:include — 外部ファイルの読み込み](#include--外部ファイルの読み込み)
  - [:unique — リストの重複排除](#unique--リストの重複排除)
  - [:split — 文字列を配列に](#split--文字列を配列に)
  - [:join — 配列を文字列に](#join--配列を文字列に)
  - [:geo — 地域ベースの選択](#geo--地域ベースの選択)
  - [:clamp — 数値クランプ](#clamp--数値クランプ)
  - [:round — 四捨五入](#round--四捨五入)
  - [:map — ルックアップテーブル](#map--ルックアップテーブル)
  - [:format — 数値フォーマット](#format--数値フォーマット)
  - [:fallback — ファイルパスのフォールバック](#fallback--ファイルパスのフォールバック)
  - [:once — 生成して永続化](#once--生成して永続化)
  - [:version — セマンティックバージョン比較](#version--セマンティックバージョン比較)
  - [:watch — 外部ファイルの読み取り](#watch--外部ファイルの読み取り)
- [制約](#-制約)
- [マーカーチェーン](#-マーカーチェーン)
- [コード例](#-コード例)
- [エディタサポート](#-エディタサポート)
- [アーキテクチャ](#-アーキテクチャ)
- [リンク](#-リンク)

---

## 💡 設計思想

設定はあらゆるアプリケーションの基盤です。しかし、業界標準のフォーマット — **JSON** と **YAML** — はこのために設計されたものではありませんでした：

| 問題 | JSON | YAML | SYNX |
|---|:---:|:---:|:---:|
| 文字列とキーに引用符が必要 | ✓ | ✗ | ✗ |
| 末尾カンマでパースエラー | ✗ | — | ✓ |
| スペース依存のインデント | — | ✗ (危険) | ✓ (安全、2スペース) |
| コメントサポート | ✗ | ✓ | ✓ |
| 環境変数 | ✗ | ✗ | ✓ ネイティブ |
| 計算値 | ✗ | ✗ | ✓ ネイティブ |
| AIトークンコスト (110キー) | ~3300文字 | ~2500文字 | **~2000文字** |
| 可読性 | 低 | 中 | **高** |

SYNXは3つの原則に基づいて構築されています：

1. **最小限の構文** — キー、スペース、値。引用符なし、カンマなし、波括弧なし、コロンなし。
2. **本質的にアクティブ** — 設定はデータだけでなく、ロジックです。環境変数、数学演算、参照、ランダム選択、バリデーション — すべてフォーマットに組み込まれています。
3. **トークン効率** — LLMを通じて設定を送信する際、すべての文字が重要です。SYNXはJSONと比較して30〜40%のトークンを節約します。

> **SYNXはJSONの代替ではありません。SYNXはJSONがあるべきだった姿です。**

---

## 🎬 デモンストレーション

### データ入力 — クリーンでシンプル

**キー**、**スペース**、**値**だけ。引用符なし、カンマなし、波括弧なし：

<p align="center">
  <a href="https://aperturesyndicate.com/branding/gifs/synx/synx.gif" target="_blank">
    <img src="https://aperturesyndicate.com/branding/gifs/synx/synx.gif" alt="静的SYNXの記述" width="720" />
  </a>
</p>

> **📺 [デモを見る →](https://aperturesyndicate.com/branding/gifs/synx/synx.gif)**

### `!active` モード — ロジック付き設定

最初の行に `!active` を追加すると、設定が動き出します — 関数がフォーマットに直接組み込まれています：

<p align="center">
  <a href="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" target="_blank">
    <img src="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" alt="マーカー付きアクティブSYNXの記述" width="720" />
  </a>
</p>

> **📺 [デモを見る →](https://aperturesyndicate.com/branding/gifs/synx/synx2.gif)**

---

## ⚙ 仕組み

SYNXパイプラインは**2つのステージ**に分かれています — この分離がパフォーマンスの鍵です：

```
┌──────────────┐         ┌─────────────┐         ┌──────────────┐
│  .synxファイル│ ──────► │  パーサー    │ ──────► │    出力      │
│  (テキスト)   │         │  (常に実行)  │         │  (JS)        │
└──────────────┘         └──────┬──────┘         └──────────────┘
                                │
                         !active あり？
                                │
                           ┌────▼────┐
                           │ エンジン │
                           │(マーカー │
                           │ を実行)  │
                           └─────────┘
```

### ステージ1 — パーサー

**パーサー**はテキストを読み取り、キーバリューツリーを構築します。キーバリューペア、ネスト（2スペースインデント）、リスト、型変換、コメント、複数行テキストを処理します。

パーサーはマーカー（`:env`、`:calc` など）を各キーに付加された**メタデータ**として記録しますが、**実行はしません**。つまり、**新しいマーカーを追加してもパースは遅くなりません**。

### ステージ2 — エンジン（`!active` の場合のみ）

ファイルが `!active` で始まる場合、**エンジン**がパース済みツリーを走査し、各マーカーを解決します。

**`!active` のないファイルはエンジンに触れることはありません。**

---

## 📊 パフォーマンスとベンチマーク

すべてのベンチマークは実データで、標準的な110キーSYNX設定（2.5 KB）で実行：

### Rust（criterion、直接呼び出し）

| ベンチマーク | 時間 |
|---|---|
| `Synx::parse`（110キー） | **~39 µs** |
| `parse_to_json`（110キー） | **~42 µs** |
| `Synx::parse`（4キー） | **~1.2 µs** |

### Node.js（50,000回反復）

| パーサー | µs/回 | vs JSON | vs YAML |
|---|---:|---:|---:|
| `JSON.parse`（3.3 KB） | 6.08 µs | 1× | — |
| **`synx-js` 純TS** | **39.20 µs** | 6.4× | **YAMLの2.1倍高速** |
| `js-yaml`（2.5 KB） | 82.85 µs | 13.6× | 1× |

### Python（10,000回反復）

| パーサー | µs/回 | vs YAML |
|---|---:|---:|
| `json.loads`（3.3 KB） | 13.04 µs | — |
| **`synx_native.parse`** | **55.44 µs** | **YAMLの67倍高速** |
| `yaml.safe_load`（2.5 KB） | 3,698 µs | 1× |

> Pythonでは、SYNXはYAMLの**67倍**高速にパースします。

---

## 📦 インストール

### Node.js / ブラウザ

```bash
npm install @aperturesyndicate/synx
```

### Python

```bash
pip install synx-format
```

### Rust

```bash
cargo add synx-core
```

### VS Code拡張機能

拡張機能パネルで **"SYNX"** を検索するか：

```bash
code --install-extension APERTURESyndicate.synx-vscode
```

---

## 📝 構文リファレンス

### 基本構文

基本ルール：**キー** `(スペース)` **値**。

```synx
name John
age 25
phrase プログラミング大好き！
empty_value
```

> 数値、ブーリアン（`true`/`false`）、`null` は自動検出されます。それ以外はすべて文字列です。

---

### ネスト

インデントで階層を作成 — 各レベル**2スペース**：

```synx
server
  host 0.0.0.0
  port 8080
  ssl
    enabled true
```

---

### リスト

`- ` で始まる行は配列を作成：

```synx
fruits
  - Apple
  - Banana
  - Cherry
```

---

### 型変換

キー名の後に `(型)` を使用して型を強制：

```synx
zip_code(string) 90210
id(int) 007
ratio(float) 3
enabled(bool) 1
```

利用可能な型：`int`、`float`、`bool`、`string`。

---

### 複数行テキスト

`|` 演算子を使用：

```synx
description |
  これは長い説明文で、
  複数行にまたがっています。
```

---

### コメント

```synx
# ハッシュコメント
// スラッシュコメント
name John  # インラインコメント
```

---

## 🔥 アクティブモード (`!active`)

**最初の行**に `!active` を置くと、マーカーと制約がアンロックされます。

```synx
!active

port:env PORT
boss_hp:calc base_hp * 5
```

---

## 🧩 マーカー完全リファレンス

SYNX v3.0は**20のマーカー**を提供します。各マーカーは `:マーカー` 構文でキーに付加される関数です。

### `:env` — 環境変数

```synx
!active
port:env PORT
port:env:default:8080 PORT
```

### `:default` — デフォルト値

```synx
!active
theme:default dark
```

### `:calc` — 算術式

```synx
!active
base_price 100
tax:calc base_price * 0.2
total:calc base_price + tax
```

演算子：`+` `-` `*` `/` `%` `(` `)`

### `:random` — ランダム選択

```synx
!active
loot:random 70 20 10
  - common
  - rare
  - legendary
```

### `:alias` — 別キーの参照

```synx
!active
admin_email alex@example.com
billing:alias admin_email
```

### `:secret` — 隠し値

```synx
!active
api_key:secret sk-1234567890
```

### `:template` — 文字列補間

```synx
!active
name John
greeting:template こんにちは、{name}さん！
```

### `:include` — 外部ファイルの読み込み

```synx
!active
database:include ./db.synx
```

### `:unique` — リストの重複排除

```synx
!active
tags:unique
  - action
  - rpg
  - action
```

結果：`["action", "rpg"]`

### `:split` — 文字列を配列に

```synx
!active
colors:split red, green, blue
words:split:space hello world foo
```

区切りキーワード：`space`、`pipe`、`dash`、`dot`、`semi`、`tab`

### `:join` — 配列を文字列に

```synx
!active
path:join:slash
  - home
  - user
  - docs
```

結果：`"home/user/docs"`

### `:geo` — 地域ベースの選択

```synx
!active
currency:geo
  - US USD
  - JP JPY
  - EU EUR
```

### `:clamp` — 数値クランプ

```synx
!active
volume:clamp:0:100 150
```

結果：`100`

### `:round` — 四捨五入

```synx
!active
price:round:2 19.999
profit:calc:round:2 revenue * 0.337
```

### `:map` — ルックアップテーブル

```synx
!active
status_code 1
status:map:status_code
  - 0 オフライン
  - 1 オンライン
  - 2 離席中
```

結果：`"オンライン"`

### `:format` — 数値フォーマット

```synx
!active
price:format:%.2f 1234.5
id:format:%06d 42
```

結果：`"1234.50"`、`"000042"`

### `:fallback` — ファイルパスのフォールバック

```synx
!active
icon:fallback:./default.png ./custom.png
```

### `:once` — 生成して永続化

```synx
!active
session_id:once uuid
app_seed:once random
build_time:once timestamp
```

生成タイプ：`uuid`（デフォルト）、`random`、`timestamp`

### `:version` — セマンティックバージョン比較

```synx
!active
runtime:version:>=:18.0 20.11.0
```

結果：`true`。演算子：`>=` `<=` `>` `<` `==` `!=`

### `:watch` — 外部ファイルの読み取り

```synx
!active
app_name:watch:name ./package.json
config:watch ./data.txt
```

---

## 🔒 制約

制約はパース時に値を検証します。キー名の後の `[角括弧]` 内に定義します。

| 制約 | 構文 | 説明 |
|---|---|---|
| `required` | `key[required]` | 値が必須 |
| `readonly` | `key[readonly]` | 読み取り専用 |
| `min:N` | `key[min:3]` | 最小長/値 |
| `max:N` | `key[max:100]` | 最大長/値 |
| `type:T` | `key[type:int]` | 型を強制 |
| `pattern:R` | `key[pattern:^\d+$]` | 正規表現で検証 |
| `enum:A\|B` | `key[enum:light\|dark]` | 許可される値 |

```synx
!active
app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
```

---

## 🔗 マーカーチェーン

```synx
!active
port:env:default:8080 PORT
profit:calc:round:2 revenue * margin
```

---

## 💻 コード例

### JavaScript / TypeScript

```typescript
import { Synx } from '@aperturesyndicate/synx';

const config = Synx.parse(`
  app_name TotalWario
  server
    host 0.0.0.0
    port 8080
`);

console.log(config.server.port);  // 8080
```

### Python

```python
import synx_native

config = synx_native.parse("""
app_name TotalWario
server
  host 0.0.0.0
  port 8080
""")

print(config["server"]["port"])  # 8080
```

### Rust

```rust
use synx_core::Synx;

let config = Synx::parse("
    app_name TotalWario
    version 3.0.0
");
```

---

## 🛠 エディタサポート

### Visual Studio Code

完全な言語サポート：シンタックスハイライト、IntelliSense（20マーカー）、リアルタイム診断（15項目チェック）、定義へジャンプ、フォーマット、カラープレビュー、`:calc` インラインヒント、ライブJSONプレビュー。

### Visual Studio 2022

MEF拡張機能：シンタックスハイライト、IntelliSense、エラーマーキング、コード折りたたみ、変換コマンド。

---

## 🏗 アーキテクチャ

```
synx-format/
├── crates/synx-core/          # Rustコア — パーサー + エンジン
├── bindings/
│   ├── node/                  # NAPI-RS → npmネイティブモジュール
│   └── python/                # PyO3 → PyPIネイティブモジュール
├── packages/
│   ├── synx-js/               # 純TypeScriptパーサー + エンジン
│   ├── synx-vscode/           # VS Code拡張機能
│   └── synx-visualstudio/     # Visual Studio 2022拡張機能
├── publish-npm.bat
├── publish-pypi.bat
└── publish-crates.bat
```

---

## 🔗 リンク

| リソース | URL |
|---|---|
| **GitHub** | [github.com/kaiserrberg/synx-format](https://github.com/kaiserrberg/synx-format) |
| **npm** | [npmjs.com/package/@aperturesyndicate/synx](https://www.npmjs.com/package/@aperturesyndicate/synx) |
| **PyPI** | [pypi.org/project/synx-format](https://pypi.org/project/synx-format/) |
| **crates.io** | [crates.io/crates/synx-core](https://crates.io/crates/synx-core) |

---

<p align="center">
  <img src="https://aperturesyndicate.com/branding/logos/asp_128.png" width="96" height="96" />
</p>

<p align="center">
  MIT — © <a href="https://github.com/kaiserrberg">APERTURESyndicate</a>
</p>
