<p align="center">
  <a href="https://media.aperturesyndicate.com/asother/as/branding/png/aperturesyndicate.png" target="_blank">
    <img src="https://media.aperturesyndicate.com/asother/as/branding/png/aperturesyndicate.png" alt="APERTURESyndicate" width="400" />
  </a>
</p>

> **🔗 [ロゴを表示 →](https://media.aperturesyndicate.com/asother/as/branding/png/aperturesyndicate.png)**

<h1 align="center">SYNX v3.6 — 完全ガイド</h1>

<p align="center">
  <strong>JSONより優れている。YAMLより安い。AIと人間のために作られた。</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-3.6.0-5a6eff?style=for-the-badge" />
  <img src="https://img.shields.io/badge/license-MIT-blue?style=for-the-badge" />
  <img src="https://img.shields.io/badge/format-SYNX%20v3.6-blueviolet?style=for-the-badge" />
  <img src="https://img.shields.io/badge/written_in-Rust-orange?style=for-the-badge" />
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/@aperturesyndicate/synx-format">npm</a> ·
  <a href="https://pypi.org/project/synx-format/">PyPI</a> ·
  <a href="https://crates.io/crates/synx-core">crates.io</a> ·
  <a href="https://marketplace.visualstudio.com/items?itemName=APERTURESyndicate.synx-vscode">VS Code</a> ·
  <a href="https://github.com/APERTURESyndicate/synx-format">GitHub</a>
</p>

---

## 目次

- [設計思想](#-設計思想)
- [デモンストレーション](#-デモンストレーション)
- [仕組み](#-仕組み)
- [セキュリティモデル (v3.5.0+)](#-セキュリティモデル-v350)
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
- [ロックモード (`!lock`)](#-ロックモード-lock)
- [Includeディレクティブ (`!include`)](#-includeディレクティブ-include)
- [正規フォーマット (`format`)](#-正規フォーマット-format)
- [マーカー完全リファレンス](#-マーカー完全リファレンス)
  - [:env — 環境変数](#env--環境変数)
  - [:default — デフォルト値](#default--デフォルト値)
  - [:calc — 算術式](#calc--算術式)
  - [:random — ランダム選択](#random--ランダム選択)
  - [:alias — 別キーの参照](#alias--別キーの参照)
  - [:ref — チェーン付き参照](#ref--チェーン付き参照)
  - [:inherit — ブロック継承](#inherit--ブロック継承)
  - [:i18n — 多言語値](#i18n--多言語値)
  - [:secret — 隠し値](#secret--隠し値)
  - [auto-{} — 文字列補間](#auto---文字列補間)
  - [:include / :import — 外部ファイルの読み込み](#include--import--外部ファイルの読み込み)
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
  - [:spam — アクセス頻度制限](#spam--アクセス頻度制限)
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

## セキュリティモデル (v3.5.0+)

SYNXはマーカー機能を維持したまま、ファイル操作と式評価に実行時ガードを追加しています。

- **ファイル系マーカーのPath Jail**: `:include`, `:import`, `:watch`, `:fallback` は `basePath` 内でのみ解決されます。絶対パスと `../` によるベース外への移動は拒否されます。
- **ネスト深度ガード**: include/watch の再帰はデフォルトで `16` 階層まで（設定可能）。
  Rustオプション: `max_include_depth`
  JSオプション: `maxIncludeDepth`
- **ファイルサイズガード**: `10 MB` を超えるファイルは読み込み前に拒否されます。
- **`:calc` 式長ガード**: `4096` 文字を超える式は拒否されます。
- **エンジン動作**: パーサーはメタデータのみ記録し、マーカー処理は `!active` のときだけ実行されます。

セキュリティ注記:
- SYNXは設定データから任意コードを実行しません（YAML系オブジェクトコンストラクタなし、`eval` なし）。

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
npm install @aperturesyndicate/synx-format
```

### Python

```bash
pip install synx-format
```

### Rust

```bash
cargo add synx-core
```

### C# / .NET 8（NuGet）

```bash
dotnet add package APERTURESyndicate.Synx
```

パッケージ: [nuget.org/packages/APERTURESyndicate.Synx](https://www.nuget.org/packages/APERTURESyndicate.Synx)。nuget.org では ID `Synx.Core` は他者が使用中です。公開前は [`parsers/dotnet/README.md`](../../parsers/dotnet/README.md) を参照。

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

> **引用符付きの値**はリテラル文字列として保持されます：`"null"`、`"true"`、`"42"` は文字列のままです。

パーサーの型推論（明示的な `(type)` がない場合）:

1. 完全一致 `true`/`false` -> Bool
2. 完全一致 `null` -> Null
3. 整数パターン -> Int
4. 小数パターン -> Float
5. それ以外 -> String

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

#### ランダム値生成

`(random)` を使用して解析時にランダム値を生成：

```synx
pin(random) null
flag(random:bool) null
chance(random:float) null
dice(random:int) null
```

```json
{
  "pin": 1847362951,
  "flag": true,
  "chance": 0.7342,
  "dice": 982451653
}
```

利用可能な型：`(random)`（整数）、`(random:int)`、`(random:float)`、`(random:bool)`。

> 値は解析時に生成されます — 毎回異なる値が生成されます。

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

###
これはブロックコメントです。
### の間のすべてが無視されます。
###
```

VSCode拡張機能では、コメント内のフォーマットが対応しています：
- `*イタリック*` — 緑
- `**太字**` — 紫
- `***太字+イタリック***` — 金
- `` `コード` `` — オレンジ（背景付き）

---

## 🔥 アクティブモード (`!active`)

**最初の行**に `!active` を置くと、マーカーと制約がアンロックされます。

```synx
!active

port:env PORT
boss_hp:calc base_hp * 5
```

---

## 🔐 ロックモード (`!lock`)

`!lock` を追加すると、外部コードから `Synx.set()`、`Synx.add()`、`Synx.remove()` で設定値を変更できなくなります。SYNX 内部マーカーは通常通り動作します。

```synx
!active
!lock

max_players 100
greeting:random
  - こんにちは！
  - ようこそ！
```

```typescript
const config = Synx.loadSync('./config.synx');

Synx.set(config, 'max_players', 200);
// ❌ エラー: "SYNX: Cannot set "max_players" — config is locked (!lock)"

console.log(config.max_players); // ✅ 100（読み取りは常に許可）
```

`Synx.isLocked(config)` でロック状態を確認できます。

---

## 📎 Includeディレクティブ (`!include`)

`!include` ディレクティブは、別の `.synx` ファイルのキーを `{key:alias}` 補間用にインポートします。`:include` マーカー（ファイルを子ブロックとして埋め込む）とは異なり、`!include` はトップレベルのキーを文字列補間に利用可能にします。

```synx
!active
!include ./db.synx
!include ./cache.synx redis

db_url postgresql://{host:db}:{port:db}/{name:db}
cache_url redis://{host:redis}:{port:redis}
```

| ディレクティブ | エイリアス | アクセス |
|---|---|---|
| `!include ./db.synx` | `db`（自動） | `{host:db}` |
| `!include ./cache.synx redis` | `redis`（明示的） | `{host:redis}` |
| `!include ./config.synx`（唯一のinclude） | — | `{host:include}` |

---

## 🧹 正規フォーマット (`format`)

`Synx.format()` は任意の `.synx` 文字列を唯一の正規化された形式に書き直します。

**機能：**
- **すべてのキーをアルファベット順にソート** — すべてのネストレベルで
- **インデントを正規化** — レベルごとに正確に2スペース
- **コメントを削除** — 正規フォーマットはデータのみを含む
- **トップレベルブロック間に1行の空行**（オブジェクトとリスト）
- **ディレクティブを保持** (`!active`, `!lock`) はファイルの先頭に維持
- **リスト要素の順序を保持** — 名前付きキーのみがソートされる

### Gitにとっての重要性

正規フォーマットがなければ、2人の開発者が同じ設定を異なる形で書く可能性があります：

```synx
# 開発者 A                   # 開発者 B
server                       server
    port 8080                  host 0.0.0.0
    host 0.0.0.0               port 8080
```

`git diff` はブロック全体が変更されたと表示します — データは同一であるにもかかわらず。

`Synx.format()` 後、どちらも同じ出力になります：

```synx
server
  host 0.0.0.0
  port 8080
```

ひとつの正規形式。diffのノイズはゼロ。

### 使用方法

**JavaScript / TypeScript：**

```typescript
import { Synx } from '@aperturesyndicate/synx-format';
import * as fs from 'fs';

const raw = fs.readFileSync('config.synx', 'utf-8');
fs.writeFileSync('config.synx', Synx.format(raw));
```

**Rust：**

```rust
use synx_core::Synx;

let raw = std::fs::read_to_string("config.synx").unwrap();
std::fs::write("config.synx", Synx::format(&raw)).unwrap();
```

---

## 🧩 マーカー完全リファレンス

SYNX v3.6は**24のマーカー**を提供します。各マーカーは `:マーカー` 構文でキーに付加される関数です。

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

入れ子キーの dot-path をサポートします:

```synx
!active
stats
  base_hp 100
  multiplier 3

total_hp:calc stats.base_hp * stats.multiplier
```

### `:random` — ランダム選択

```synx
!active
loot:random 70 20 10
  - common
  - rare
  - legendary
```

### `:alias` — 別キーの参照

別のキーの解決済み値をコピーします。ソースを一度変更すれば、すべてのエイリアスが更新されます。

```synx
!active
admin_email alex@example.com
billing:alias admin_email
complaints:alias admin_email
```

`:alias` はソースを先に解決するため、他のマーカーを持つキーも参照できます：

```synx
!active
base_port:env:default:3000 PORT
api_port:alias base_port
```

> **`:alias` vs `:ref`:** 両方とも値をコピーしますが、`:alias` は終端操作です。マーカーをチェーンする場合は `:ref` を使用します（例: `:ref:calc:*2`）。

### `:ref` — チェーン付き参照

`:alias`と同様ですが、解決された値を後続のマーカーに渡します。

```synx
!active

base_rate 50
quick_rate:ref base_rate
double_rate:ref:calc:*2 base_rate
```

**省略形構文:** `:ref:calc:*2` は参照を解決し、演算子を適用します。サポート: `+`, `-`, `*`, `/`, `%`。

**例 — 難易度スケーリング:**

```synx
!active

base_hp 100
easy_hp:ref:calc:*0.5 base_hp
hard_hp:ref:calc:*2 base_hp
```

> **`:ref` と `:alias` の使い分け:** 値をさらに処理する場合は `:ref`。単純なコピーには `:alias`。

---

### `:inherit` — ブロック継承

親ブロックの全フィールドを子ブロックにマージします。子の値が優先されます。`_` プレフィックスはプライベートブロック—出力から除外されます。

```synx
!active

_base_resource
  weight 10
  stackable true

steel:inherit:_base_resource
  weight 25
  material metal
```

複数親継承をサポートします。適用順序は左から右、子ブロックが最終的に上書きします。

```synx
!active
_movable
  speed 10
_damageable
  hp 100

tank:inherit:_movable:_damageable
  hp 150
```

**多段階継承:**

```synx
!active

_entity
  visible true
  layer world

_enemy:inherit:_entity
  hostile true
  ai patrol

goblin:inherit:_enemy
  hp 30
  damage 5
```

継承チェーンが機能します: `_entity` → `_enemy` → `goblin`。プライベートブロックは出力から除外されます。

---

### `:i18n` — 多言語値

ネストされた言語キーからローカライズされた値を選択します。オプションで `lang` を渡します。フォールバック: `en` → 最初の利用可能な値。

```synx
!active

title:i18n
  en Hello World
  ja こんにちは世界
  ru Привет мир
```

```javascript
const config = Synx.parse(text, { lang: 'ja' });
// config.title → "こんにちは世界"
```

複数形は `:i18n:COUNT_FIELD` でサポートされます:

```synx
!active
count 5

label:i18n:count
  en
    one {count} item
    other {count} items
```

---

### `:secret` — 隠し値

```synx
!active
api_key:secret sk-1234567890
```

### Auto-`{}` — 文字列補間

`!active` モードでは、`{key}` を含む文字列値は自動的に補間されます—マーカー不要。

```synx
!active
name John
greeting こんにちは、{name}さん！

server
  host api.example.com
  port 443
api_url https://{server.host}:{server.port}/v1
```

**`!include` によるクロスファイル補間:**

```synx
!active
!include ./db.synx

conn_string postgresql://{host:db}:{port:db}/{name:db}
```

構文: `{key}` ローカル、`{key:alias}` インクルードファイル、`{key:include}` 唯一のインクルードファイル。

> **レガシー:** `:template` マーカーは引き続き機能しますが、もはや不要です。

### `:include / :import` — 外部ファイルの読み込み

```synx
!active
database:import ./db.synx
```

`:import` は `:include` のエイリアスです（挙動は同じ）。

| 仕組み | 使用場所 | 動作 |
|---|---|---|
| `!include ./file.synx [alias]` | ファイル先頭ディレクティブ | `{key:alias}` 補間用の値を公開 |
| `key:include ./file.synx` / `key:import ./file.synx` | キー上のマーカー | ファイルを子オブジェクトとして埋め込み |

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

区切りキーワード：`space`、`pipe`、`dash`、`dot`、`semi`、`tab`、`slash`

### `:join` — 配列を文字列に

区切りキーワード: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`, `slash`。デフォルト: カンマ。

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

### `:spam` — アクセス頻度制限

指定した時間窓の中で、対象キー/ファイルへの解決回数を制限します。

構文: `:spam:MAX_CALLS[:WINDOW_SEC]`。
`WINDOW_SEC` を省略した場合は `1` が使われます。

```synx
!active
secret_token abc
access:spam:3:10 secret_token
burst_access:spam:5 secret_token
```

上限を超えると `SPAM_ERR: ...` が返されます。

---

### `:prompt` — LLMプロンプトブロック

> v3.6.0で追加。

サブツリーをLLM用のラベル付きSYNXコードフェンスにフォーマットします。`:prompt:ラベル` のようにラベルを指定します。

```synx
!active
app_name "MyCoolApp"
version "2.1.0"

prompt_block:prompt:AppConfig
  app_name "MyCoolApp"
  version "2.1.0"
```

解決後の結果：

```json
{
  "app_name": "MyCoolApp",
  "version": "2.1.0",
  "prompt_block": "AppConfig (SYNX):\n```synx\napp_name \"MyCoolApp\"\nversion \"2.1.0\"\n```"
}
```

AIパイプラインに最適 — 追加のシリアライズコード不要で構造化コンテキストをLLMに渡せます。

---

### `:vision` — 画像生成インテント

> v3.6.0で追加。

キーを画像生成インテントとしてマークするメタデータマーカーです。エンジンはパース時にこのマーカーをパススルーし、アプリケーション層で処理できるよう保持します。

```synx
!active
banner:vision "sunset landscape, 16:9, photorealistic"
```

---

### `:audio` — 音声生成インテント

> v3.6.0で追加。

キーを音声生成インテントとしてマークするメタデータマーカーです。`:vision` と同様にパススルーされます。

```synx
!active
greeting:audio "Welcome to our application"
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

### ✅ マーカー互換性

よく機能する組み合わせ:

- `env:default`
- `calc:round`
- `split:unique`
- `split:join`（中間配列を経由）

重要な制限:

- `!active` が必要です。ない場合、マーカーは評価されません。
- 一部のマーカーは型依存です: `split` は文字列、`join` は配列、`round`/`clamp` は数値を想定します。
- マーカー引数はチェーン内の右側から読み取られます（例: `clamp:min:max`, `round:n`, `map:key`）。
- 前のマーカーで型が変わると、後続マーカーが適用されない場合があります。

---

## � CLIツール

> v3.1.3で追加。

npmでグローバルインストール：

```bash
npm install -g @aperturesyndicate/synx-format
```

### `synx convert` — 他のフォーマットへエクスポート

```bash
# SYNX → JSON
synx convert config.synx --format json

# SYNX → YAML（Helm、Ansible、K8s向け）
synx convert config.synx --format yaml > values.yaml

# SYNX → TOML
synx convert config.synx --format toml

# SYNX → .env（Docker Compose向け）
synx convert config.synx --format env > .env

# strictモード（マーカーエラーで即座に失敗）
synx convert config.synx --format json --strict
```

### `synx validate` — CI/CDバリデーション

```bash
synx validate config.synx --strict
# 成功時は終了コード0、INCLUDE_ERR / WATCH_ERR / CALC_ERR / CONSTRAINT_ERRで1
```

### `synx watch` — ライブリロード

```bash
# 変更のたびにJSONを出力
synx watch config.synx --format json

# 変更のたびにコマンドを実行（例：Nginxのリロード）
synx watch config.synx --exec "nginx -s reload"
```

### `synx schema` — 制約からJSON Schemaを抽出

```bash
synx schema config.synx
# [required, min:N, max:N, type:T, enum:A|B, pattern:R]に基づきJSON Schemaを出力
```

---

## 📤 エクスポートフォーマット（JS/TS API）

> v3.1.3で追加。

パース済みSYNXオブジェクトをJSON、YAML、TOML、または.envに変換：

```typescript
import Synx from '@aperturesyndicate/synx-format';

const config = Synx.loadSync('config.synx');

// JSON
const json = Synx.toJSON(config);          // 整形済み
const compact = Synx.toJSON(config, false); // コンパクト

// YAML
const yaml = Synx.toYAML(config);

// TOML
const toml = Synx.toTOML(config);

// .env（KEY=VALUEフォーマット）
const env = Synx.toEnv(config);            // APP_NAME=TotalWario
const prefixed = Synx.toEnv(config, 'APP'); // APP_APP_NAME=TotalWario
```

---

## 📋 スキーマエクスポート

> v3.1.3で追加。

SYNXの制約をJSON Schemaオブジェクトとして抽出：

```typescript
const schema = Synx.schema(`
!active
app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
`);
```

結果：

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {
    "app_name": { "minimum": 3, "maximum": 30, "required": true },
    "volume": { "type": "integer", "minimum": 0, "maximum": 100 },
    "theme": { "enum": ["light", "dark", "auto"] }
  },
  "required": ["app_name"]
}
```

---

## � Synx.diff()

> v3.6.0で追加。

2つのパース済みSYNXオブジェクトを比較し、構造化された差分を返します：

```typescript
const old = Synx.parse('config_v1.synx');
const next = Synx.parse('config_v2.synx');
const diff = Synx.diff(old, next);
// diff.added    — 新しいオブジェクトにのみ存在するキー
// diff.removed  — 古いオブジェクトにのみ存在するキー
// diff.changed  — 両方に存在するが値が異なるキー（{ from, to }）
// diff.unchanged — 値が同一のキー一覧
```

設定のマイグレーション検証、監査ログ、CI差分チェックに活用できます。

---

## �👁 ファイルウォッチャー

> v3.1.3で追加。

`.synx`ファイルを監視し、変更のたびに更新された設定を取得：

```typescript
const handle = Synx.watch('config.synx', (config, error) => {
  if (error) {
    console.error('設定のリロードに失敗:', error.message);
    return;
  }
  console.log('設定が更新されました:', config.server.port);
}, { strict: true });

// 監視を停止
handle.close();
```

---

## 🐳 デプロイガイド

> v3.1.3で追加。

### Docker + Docker Compose

SYNXはすべてのサービス設定の**単一の信頼できるソース**として機能します。独自の設定フォーマットが必要なサービス（Nginx、Redisなど）は、起動時に生成された設定を受け取ります。

**パターン：**

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   config.synx   │────▶│  起動スクリプト  │────▶│  nginx.conf     │
│  （1ファイル）    │     │  またはCLI conv. │     │  .env           │
│  :env :default  │     │                 │     │  redis.conf     │
│  :template      │     │                 │     │  アプリ設定      │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

**ステップ1 — 設定を記述：**

```synx
!active

app
  name my-service
  port:env:default:3000 APP_PORT
  host:env:default:0.0.0.0 APP_HOST

database
  host:env:default:postgres DB_HOST
  port:env:default:5432 DB_PORT
  name:env:default:mydb DB_NAME
  user:env:default:app DB_USER
  password:env DB_PASSWORD

redis
  host:env:default:redis REDIS_HOST
  port:env:default:6379 REDIS_PORT
  url:template redis://{redis.host}:{redis.port}/0
```

**ステップ2 — Docker Compose用の.envを生成：**

```bash
synx convert config.synx --format env > .env
```

**ステップ3 — docker-compose.ymlで使用：**

```yaml
services:
  web:
    image: node:20-alpine
    env_file: .env
    ports:
      - "${APP_PORT}:${APP_PORT}"

  redis:
    image: redis:7-alpine

  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_DB: ${DB_NAME}
      POSTGRES_USER: ${DB_USER}
      POSTGRES_PASSWORD: ${DB_PASSWORD}
```

### Nginx設定の生成

テンプレート+起動スクリプトを使用して、SYNXから`nginx.conf`を生成：

```javascript
const Synx = require('@aperturesyndicate/synx-format');
const fs = require('fs');

const config = Synx.loadSync('/config/app.synx', {
  env: process.env,
  strict: true,
});

const nginxConf = `
server {
  listen ${config.nginx.listen};
  location / {
    proxy_pass http://${config.nginx.upstream_host}:${config.nginx.upstream_port};
  }
}`;

fs.writeFileSync('/etc/nginx/conf.d/default.conf', nginxConf);
```

### Redis接続

```synx
!active

redis
  host:env:default:localhost REDIS_HOST
  port:env:default:6379 REDIS_PORT
  db:default 0
  ttl:default 3600
  password:env REDIS_PASSWORD
  url:template redis://{redis.host}:{redis.port}/{redis.db}
```

```javascript
const config = Synx.loadSync('config.synx', { env: process.env, strict: true });
const redis = new Redis(config.redis.url);
```

### PostgreSQL接続

```synx
!active

db
  host:env:default:localhost DATABASE_HOST
  port:env:default:5432 DATABASE_PORT
  name:env:default:mydb DATABASE_NAME
  user:env:default:app DATABASE_USER
  password:env DATABASE_PASSWORD
  url:template postgresql://{db.user}:{db.password}@{db.host}:{db.port}/{db.name}
  pool_min:default 5
  pool_max:default 20
```

```javascript
const config = Synx.loadSync('config.synx', { env: process.env, strict: true });
const pool = new Pool({ connectionString: config.db.url });
```

### Kubernetes Secrets

K8sはシークレットを`/run/secrets/`にファイルとしてマウントします。`:watch`で読み取り：

```synx
!active

db_password:watch /run/secrets/db-password
api_key:watch /run/secrets/api-key
```

Docker Secretsも同様です — `/run/secrets/`にマウントされます。

### HashiCorp Vault

Vault Agentを使用してシークレットをファイルに書き込み、`:watch`で読み取り：

```synx
!active

db_creds:watch:password /vault/secrets/database
api_key:watch:key /vault/secrets/api-key
```

またはVault Agentの`env_template`を使用して環境変数で注入：

```synx
!active

db_password:env VAULT_DB_PASSWORD
api_key:env VAULT_API_KEY
```

### Helm / Kubernetes

SYNXをYAMLに変換してHelmのvaluesとして使用：

```bash
synx convert config.synx --format yaml > helm/values.yaml
helm upgrade my-release ./chart -f helm/values.yaml
```

### Terraform

TerraformはJSON変数ファイルを受け付けます：

```bash
synx convert config.synx --format json > terraform.tfvars.json
terraform apply -var-file=terraform.tfvars.json
```

### CI/CDパイプラインバリデーション

CIパイプラインに追加して、デプロイ前に設定エラーを検出：

```yaml
# GitHub Actionsの例
- name: SYNX設定のバリデーション
  run: npx @aperturesyndicate/synx-format validate config.synx --strict
```

---

## �💻 コード例

### JavaScript / TypeScript

```typescript
import { Synx } from '@aperturesyndicate/synx-format';

const config = Synx.parse(`
  app_name TotalWario
  server
    host 0.0.0.0
    port 8080
`);

console.log(config.server.port);  // 8080
```

**ランタイム操作 (set / add / remove)：**

```typescript
import { Synx } from '@aperturesyndicate/synx-format';

const config = Synx.loadSync('./game.synx');

// 値を設定
Synx.set(config, 'max_players', 100);
Synx.set(config, 'server.host', 'localhost');

// 値を取得
const port = Synx.get(config, 'server.port'); // 8080

// リストに追加
Synx.add(config, 'maps', 'Arena of Doom');

// リストから削除
Synx.remove(config, 'maps', 'Arena of Doom');

// キーを完全に削除
Synx.remove(config, 'deprecated_key');

// ロック状態を確認
if (!Synx.isLocked(config)) {
  Synx.set(config, 'motd', 'ようこそ!');
}
```

> **注意:** `.synx` ファイルに `!lock` がある場合、すべての `set`/`add`/`remove` 呼び出しはエラーをスローします。

**アクセスメソッド (JS/TS API):**

- `Synx.get(obj, keyPath)` — ドットパスで値を取得。
- `Synx.set(obj, keyPath, value)` — ドットパスで値を設定。
- `Synx.add(obj, keyPath, item)` — 配列に要素を追加。
- `Synx.remove(obj, keyPath, item?)` — 配列要素の削除、またはキー全体の削除。
- `Synx.isLocked(obj)` — `!lock` でロックされているか確認。

### Python

現在 `synx_native` が公開しているのは `parse`, `parse_active`, `parse_to_json` のみです。

Python での `get`/`set`/`add`/`remove` 相当:

```python
def get_path(obj, key_path, default=None):
  cur = obj
  for part in key_path.split('.'):
    if not isinstance(cur, dict) or part not in cur:
      return default
    cur = cur[part]
  return cur

def set_path(obj, key_path, value):
  parts = key_path.split('.')
  cur = obj
  for part in parts[:-1]:
    if part not in cur or not isinstance(cur[part], dict):
      cur[part] = {}
    cur = cur[part]
  cur[parts[-1]] = value

def add_path(obj, key_path, item):
  arr = get_path(obj, key_path)
  if not isinstance(arr, list):
    set_path(obj, key_path, [] if arr is None else [arr])
    arr = get_path(obj, key_path)
  arr.append(item)

def remove_path(obj, key_path, item=None):
  parts = key_path.split('.')
  cur = obj
  for part in parts[:-1]:
    if not isinstance(cur, dict) or part not in cur:
      return
    cur = cur[part]
  last = parts[-1]
  if item is None:
    if isinstance(cur, dict):
      cur.pop(last, None)
    return
  if isinstance(cur, dict) and isinstance(cur.get(last), list):
    try:
      cur[last].remove(item)
    except ValueError:
      pass
```

```python
import synx_native

config = synx_native.parse("""
app_name TotalWario
server
  host 0.0.0.0
  port 8080
""")

print(config["server"]["port"])  # 8080

# Python access helper usage
set_path(config, "server.port", 9090)
add_path(config, "maps", "Arena of Doom")
remove_path(config, "maps", "Arena of Doom")
print(get_path(config, "server.port"))  # 9090
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

完全な言語サポート：シンタックスハイライト、IntelliSense（24マーカー）、リアルタイム診断（15項目チェック）、定義へジャンプ、フォーマット、カラープレビュー、`:calc` インラインヒント、ライブJSONプレビュー。

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

## C++ SDK（`bindings/cpp`）— v3.6.0

**C++17** ヘッダー `include/synx/synx.hpp` は、Rust **`synx-core`** と同じ **`synx-c`** ネイティブ DLL/SO にバインドします。文法、`!active`、`!tool`、`.synxb`、正規 JSON、`diff` は Rust と同一です。

```bash
cargo build -p synx-c --release
```

インクルード: `bindings/cpp/include`、`bindings/c-header/include`。詳細は [`bindings/cpp/README.md`](../../bindings/cpp/README.md)。

## Go（`bindings/go`、cgo）— v3.6.0

**cgo** で **`synx-c`** を呼び出し、Rust と同じエンジン。`CGO_ENABLED=1`、`cargo build -p synx-c --release`。**Windows** は `CGO_LDFLAGS` / `PATH` で DLL — [`bindings/go/README.md`](../../bindings/go/README.md)。

## Swift（`bindings/swift`、SwiftPM）— v3.6.0

SwiftPM の **`Synx`** が **`synx-c`** を **C 相互運用（`CSynx`）** で呼び出し、Rust と同一エンジン（Swift 単独の文法実装ではない）。`cargo build -p synx-c --release` の後、`-Xlinker -L … -lsynx_c` でリンク。macOS は `DYLD_LIBRARY_PATH` が必要な場合あり。[`bindings/swift/README.md`](../../bindings/swift/README.md)。

## Kotlin/JVM（`bindings/kotlin`、JNA）— v3.6.0

**JNA** で **`synx-c`** を呼ぶ **`SynxEngine`** — Rust と同一エンジン。`SYNX_LIB_DIR`、`./gradlew test`、**JDK 17+**。[`bindings/kotlin/README.md`](../../bindings/kotlin/README.md)。

## Mojo（`bindings/mojo`）— v3.6.0

**Mojo から CPython の `synx_native`**（本リポと同じ PyO3）。Rust と同一エンジン。`pip install synx-format` — [`bindings/mojo/README.md`](../../bindings/mojo/README.md)。

---

## 🔗 リンク

| リソース | URL |
|---|---|
| **GitHub** | [github.com/APERTURESyndicate/synx-format](https://github.com/APERTURESyndicate/synx-format) |
| **npm** | [npmjs.com/package/@aperturesyndicate/synx-format](https://www.npmjs.com/package/@aperturesyndicate/synx-format) |
| **PyPI** | [pypi.org/project/synx-format](https://pypi.org/project/synx-format/) |
| **crates.io** | [crates.io/crates/synx-core](https://crates.io/crates/synx-core) |

---

<p align="center">
  <img src="https://media.aperturesyndicate.com/asother/as/branding/png/asp_128.png" width="96" height="96" />
</p>

<p align="center">
  MIT — © <a href="https://github.com/APERTURESyndicate">APERTURESyndicate</a>
</p>
