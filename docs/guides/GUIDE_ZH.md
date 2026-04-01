<p align="center">
  <a href="https://media.aperturesyndicate.com/asother/as/branding/png/aperturesyndicate.png" target="_blank">
    <img src="https://media.aperturesyndicate.com/asother/as/branding/png/aperturesyndicate.png" alt="APERTURESyndicate" width="400" />
  </a>
</p>

> **🔗 [查看徽标 →](https://media.aperturesyndicate.com/asother/as/branding/png/aperturesyndicate.png)**

<h1 align="center">SYNX v3.6 — 完整指南</h1>

<p align="center">
  <strong>比 JSON 更好。比 YAML 更便宜。为 AI 和人类而生。</strong>
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

## 目录

- [设计哲学](#-设计哲学)
- [实际演示](#-实际演示)
- [工作原理](#-工作原理)
- [安全模型 (v3.5.0+)](#-安全模型-v350)
- [性能与基准测试](#-性能与基准测试)
- [安装](#-安装)
- [语法参考](#-语法参考)
  - [基本语法](#基本语法)
  - [嵌套](#嵌套)
  - [列表](#列表)
  - [类型转换](#类型转换)
  - [多行文本](#多行文本)
  - [注释](#注释)
- [活动模式 (`!active`)](#-活动模式-active)
- [锁定模式 (`!lock`)](#-锁定模式-lock)
- [Include指令 (`!include`)](#-include指令-include)
- [规范格式 (`format`)](#-规范格式-format)
- [标记完整参考](#-标记完整参考)
  - [:env — 环境变量](#env--环境变量)
  - [:default — 默认值](#default--默认值)
  - [:calc — 算术表达式](#calc--算术表达式)
  - [:random — 随机选择](#random--随机选择)
  - [:alias — 引用另一个键](#alias--引用另一个键)
  - [:ref — 链式引用](#ref--链式引用)
  - [:inherit — 块继承](#inherit--块继承)
  - [:i18n — 多语言值](#i18n--多语言值)
  - [:secret — 隐藏值](#secret--隐藏值)
  - [auto-{} — 字符串插值](#auto---字符串插值)
  - [:include / :import — 导入外部文件](#include--import--导入外部文件)
  - [:unique — 列表去重](#unique--列表去重)
  - [:split — 字符串转数组](#split--字符串转数组)
  - [:join — 数组转字符串](#join--数组转字符串)
  - [:geo — 基于地区选择](#geo--基于地区选择)
  - [:clamp — 数值钳制](#clamp--数值钳制)
  - [:round — 四舍五入](#round--四舍五入)
  - [:map — 查找表](#map--查找表)
  - [:format — 数字格式化](#format--数字格式化)
  - [:fallback — 文件路径回退](#fallback--文件路径回退)
  - [:once — 生成并持久化](#once--生成并持久化)
  - [:version — 语义版本比较](#version--语义版本比较)
  - [:watch — 读取外部文件](#watch--读取外部文件)
  - [:spam — 访问频率限制](#spam--访问频率限制)
- [约束](#-约束)
- [标记链](#-标记链)
- [代码示例](#-代码示例)
- [编辑器支持](#-编辑器支持)
- [架构](#-架构)
- [链接](#-链接)

---

## 💡 设计哲学

配置是每个应用的基础。然而行业标准格式 — **JSON** 和 **YAML** — 从未为此而设计：

| 问题 | JSON | YAML | SYNX |
|---|:---:|:---:|:---:|
| 字符串和键需要引号 | ✓ | ✗ | ✗ |
| 尾部逗号导致解析失败 | ✗ | — | ✓ |
| 空格敏感的缩进 | — | ✗ (危险) | ✓ (安全，2空格) |
| 注释支持 | ✗ | ✓ | ✓ |
| 环境变量 | ✗ | ✗ | ✓ 内置 |
| 计算值 | ✗ | ✗ | ✓ 内置 |
| AI 令牌成本 (110个键) | ~3300 字符 | ~2500 字符 | **~2000 字符** |
| 可读性 | 低 | 中 | **高** |

SYNX 基于三个原则构建：

1. **最小语法** — 键、空格、值。没有引号，没有逗号，没有花括号，没有冒号。
2. **天生活跃** — 配置不仅仅是数据，它是逻辑。环境变量、数学运算、引用、随机和验证 — 全部内置于格式本身。
3. **令牌高效** — 通过 LLM 发送配置时，每个字符都很重要。SYNX 比 JSON 节省 30–40% 的令牌。

> **SYNX 不是 JSON 的替代品。它是 JSON 本应成为的样子。**

---

## 🎬 实际演示

### 数据编写 — 简洁明了

只需 **键**、**空格**、**值**。没有引号，没有逗号，没有花括号：

<p align="center">
  <a href="https://aperturesyndicate.com/branding/gifs/synx/synx.gif" target="_blank">
    <img src="https://aperturesyndicate.com/branding/gifs/synx/synx.gif" alt="编写静态 SYNX" width="720" />
  </a>
</p>

> **📺 [观看演示 →](https://aperturesyndicate.com/branding/gifs/synx/synx.gif)**

### `!active` 模式 — 带逻辑的配置

在第一行添加 `!active`，你的配置就活了过来 — 函数直接内置在格式中：

<p align="center">
  <a href="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" target="_blank">
    <img src="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" alt="使用标记编写活动 SYNX" width="720" />
  </a>
</p>

> **📺 [观看演示 →](https://aperturesyndicate.com/branding/gifs/synx/synx2.gif)**

---

## ⚙ 工作原理

SYNX 管道分为 **两个阶段** — 这种分离是性能的关键：

```
┌─────────────┐         ┌─────────────┐         ┌──────────────┐
│  .synx 文件  │ ──────► │   解析器    │ ──────► │    输出      │
│  (文本)      │         │  (始终执行) │         │  (JS 对象)   │
└─────────────┘         └──────┬──────┘         └──────────────┘
                               │
                          有 !active？
                               │
                          ┌────▼────┐
                          │  引擎   │
                          │ (执行   │
                          │  标记)  │
                          └─────────┘
```

### 第一阶段 — 解析器

**解析器**读取原始文本并构建键值树。它处理键值对、嵌套（2空格缩进）、列表、类型转换、注释和多行文本。

解析器将标记（`:env`、`:calc` 等）记录为附加到每个键的**元数据**，但**不执行它们**。这意味着**添加新标记不会减慢解析速度**。

### 第二阶段 — 引擎（仅在 `!active` 时）

如果文件以 `!active` 开头，**引擎**遍历已解析的树并解析每个标记。

**没有 `!active` 的文件永远不会接触引擎。**

---

## 安全模型 (v3.5.0+)

SYNX 在保留全部标记功能的同时，为文件操作和表达式处理增加了运行时安全保护。

- **文件标记 Path Jail**: `:include`, `:import`, `:watch`, `:fallback` 只能在 `basePath` 内解析。绝对路径和通过 `../` 跳出基目录都会被拒绝。
- **嵌套深度限制**: include/watch 递归默认限制为 `16` 层（可配置）。
  Rust 选项: `max_include_depth`
  JS 选项: `maxIncludeDepth`
- **文件大小限制**: 大于 `10 MB` 的文件在读取前会被拒绝。
- **`:calc` 表达式长度限制**: 超过 `4096` 字符的表达式会被拒绝。
- **引擎行为**: 解析器只记录元数据；标记处理器仅在 `!active` 模式执行。

安全说明:
- SYNX 不会从配置数据执行任意代码（无 YAML 风格对象构造器、无 `eval`）。

---

## 📊 性能与基准测试

所有基准测试均为真实数据，运行于标准的 110 键 SYNX 配置（2.5 KB）：

### Rust（criterion，直接调用）

| 基准测试 | 时间 |
|---|---|
| `Synx::parse`（110 键） | **~39 µs** |
| `parse_to_json`（110 键） | **~42 µs** |
| `Synx::parse`（4 键） | **~1.2 µs** |

### Node.js（50,000 次迭代）

| 解析器 | µs/次 | vs JSON | vs YAML |
|---|---:|---:|---:|
| `JSON.parse`（3.3 KB） | 6.08 µs | 1× | — |
| **`synx-js` 纯 TS** | **39.20 µs** | 6.4× | **比 YAML 快 2.1×** |
| `js-yaml`（2.5 KB） | 82.85 µs | 13.6× | 1× |

### Python（10,000 次迭代）

| 解析器 | µs/次 | vs YAML |
|---|---:|---:|
| `json.loads`（3.3 KB） | 13.04 µs | — |
| **`synx_native.parse`** | **55.44 µs** | **比 YAML 快 67×** |
| `yaml.safe_load`（2.5 KB） | 3,698 µs | 1× |

> 在 Python 中，SYNX 的解析速度是 YAML 的 **67 倍**。

---

## 📦 安装

### Node.js / 浏览器

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

NuGet 包：[nuget.org/packages/APERTURESyndicate.Synx](https://www.nuget.org/packages/APERTURESyndicate.Synx)。`Synx.Core` 在 nuget.org 上已被其他包占用。正式发布前请参阅 [`parsers/dotnet/README.md`](../../parsers/dotnet/README.md)。

### VS Code 扩展

在扩展面板中搜索 **"SYNX"**，或：

```bash
code --install-extension APERTURESyndicate.synx-vscode
```

---

## 📝 语法参考

### 基本语法

基本规则：**键** `（空格）` **值**。

```synx
name John
age 25
phrase 我爱编程！
empty_value
```

> 数字、布尔值（`true`/`false`）和 `null` 会自动检测。其他所有内容都是字符串。

> **带引号的值**会被保留为字符串字面量：`"null"`、`"true"`、`"42"` 都保持为字符串。

解析器类型推断规则（未显式写 `(type)` 时）:

1. 完全匹配 `true`/`false` -> Bool
2. 完全匹配 `null` -> Null
3. 整数模式 -> Int
4. 小数模式 -> Float
5. 其他情况 -> String

---

### 嵌套

缩进创建层次结构 — 每级 **2 个空格**：

```synx
server
  host 0.0.0.0
  port 8080
  ssl
    enabled true
```

---

### 列表

以 `- ` 开头的行创建数组：

```synx
fruits
  - Apple
  - Banana
  - Cherry
```

---

### 类型转换

在键名后使用 `(类型)` 强制指定类型：

```synx
zip_code(string) 90210
id(int) 007
ratio(float) 3
enabled(bool) 1
```

可用类型：`int`、`float`、`bool`、`string`。

#### 随机值生成

使用 `(random)` 在解析时生成随机值：

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

可用类型：`(random)`（整数）、`(random:int)`、`(random:float)`、`(random:bool)`。

> 每次解析都会生成新的随机值。

---

### 多行文本

使用 `|` 运算符：

```synx
description |
  这是一段长描述，
  跨越多行。
```

---

### 注释

```synx
# 井号注释
// 斜杠注释
name John  # 行内注释

###
这是块注释。
### 之间的所有内容都会被忽略。
###
```

在 VSCode 扩展中，注释支持格式化：
- `*斜体*` — 绿色
- `**粗体**` — 紫色
- `***粗体+斜体***` — 金色
- `` `代码` `` — 橙色带背景

---

## 🔥 活动模式 (`!active`)

将 `!active` 放在**第一行**以解锁标记和约束。

```synx
!active

port:env PORT
boss_hp:calc base_hp * 5
```

---

## 🔐 锁定模式 (`!lock`)

添加 `!lock` 可以禁止外部代码通过 `Synx.set()`、`Synx.add()`、`Synx.remove()` 修改配置值。内部 SYNX 标记正常工作。

```synx
!active
!lock

max_players 100
greeting:random
  - 你好！
  - 欢迎！
```

```typescript
const config = Synx.loadSync('./config.synx');

Synx.set(config, 'max_players', 200);
// ❌ 报错: "SYNX: Cannot set "max_players" — config is locked (!lock)"

console.log(config.max_players); // ✅ 100（读取始终允许）
```

使用 `Synx.isLocked(config)` 检查是否锁定。

---

## 📎 Include指令 (`!include`)

`!include` 指令导入另一个 `.synx` 文件的键用于 `{key:alias}` 插值。与 `:include` 标记（将文件作为子块嵌入）不同，`!include` 使顶层键可用于字符串插值。

```synx
!active
!include ./db.synx
!include ./cache.synx redis

db_url postgresql://{host:db}:{port:db}/{name:db}
cache_url redis://{host:redis}:{port:redis}
```

| 指令 | 别名 | 访问 |
|---|---|---|
| `!include ./db.synx` | `db`（自动） | `{host:db}` |
| `!include ./cache.synx redis` | `redis`（显式） | `{host:redis}` |
| `!include ./config.synx`（唯一include） | — | `{host:include}` |

---

## 🧹 规范格式 (`format`)

`Synx.format()` 将任意 `.synx` 字符串重写为唯一的规范化形式。

**功能：**
- **所有键按字母顺序排序** — 在每个嵌套层级
- **缩进标准化** — 每级恰好 2 个空格
- **删除注释** — 规范格式仅包含数据
- **顶级块之间一个空行**（对象和列表）
- **保留指令** (`!active`, `!lock`) 在文件顶部
- **列表元素顺序保留** — 只有命名键被排序

### 对 Git 的意义

没有规范格式时，两位程序员写出的相同配置可能不同：

```synx
# 程序员 A                   # 程序员 B
server                       server
    port 8080                  host 0.0.0.0
    host 0.0.0.0               port 8080
```

`git diff` 会显示整个块都已更改 — 尽管数据完全相同。

使用 `Synx.format()` 后，两者都生成：

```synx
server
  host 0.0.0.0
  port 8080
```

一种规范形式。diff 中零噪音。

### 用法

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

## 🧩 标记完整参考

SYNX v3.6 提供 **24 个标记**。每个标记都是通过 `:标记` 语法附加到键的函数。

### `:env` — 环境变量

```synx
!active
port:env PORT
port:env:default:8080 PORT
```

### `:default` — 默认值

```synx
!active
theme:default dark
```

### `:calc` — 算术表达式

```synx
!active
base_price 100
tax:calc base_price * 0.2
total:calc base_price + tax
```

运算符：`+` `-` `*` `/` `%` `(` `)`

支持 dot-path 访问嵌套键:

```synx
!active
stats
  base_hp 100
  multiplier 3

total_hp:calc stats.base_hp * stats.multiplier
```

### `:random` — 随机选择

```synx
!active
loot:random 70 20 10
  - common
  - rare
  - legendary
```

### `:alias` — 引用另一个键

复制另一个键的解析值。修改源一次——所有别名自动更新。

```synx
!active
admin_email alex@example.com
billing:alias admin_email
complaints:alias admin_email
```

`:alias` 先解析源，因此可以引用带有其他标记的键：

```synx
!active
base_port:env:default:3000 PORT
api_port:alias base_port
```

> **`:alias` vs `:ref`:** 两者都复制值，但 `:alias` 是终端操作。需要链式标记时使用 `:ref`（例如 `:ref:calc:*2`）。

### `:ref` — 链式引用

类似 `:alias`，但将解析后的值传递给后续标记。

```synx
!active

base_rate 50
quick_rate:ref base_rate
double_rate:ref:calc:*2 base_rate
```

**简写语法:** `:ref:calc:*2` 解析引用并应用运算符。支持: `+`, `-`, `*`, `/`, `%`。

**示例——难度缩放:**

```synx
!active

base_hp 100
easy_hp:ref:calc:*0.5 base_hp
hard_hp:ref:calc:*2 base_hp
```

> **何时用 `:ref`，何时用 `:alias`:** 需要进一步处理值时用 `:ref`。简单复制用 `:alias`。

---

### `:inherit` — 块继承

将父块的所有字段合并到子块中。子块的值优先。`_` 前缀表示私有块——从输出中排除。

```synx
!active

_base_resource
  weight 10
  stackable true

steel:inherit:_base_resource
  weight 25
  material metal
```

支持多父继承。应用顺序为从左到右，子块最终覆盖所有父块字段。

```synx
!active
_movable
  speed 10
_damageable
  hp 100

tank:inherit:_movable:_damageable
  hp 150
```

**多级继承:**

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

继承链生效: `_entity` → `_enemy` → `goblin`。私有块从输出中排除。

---

### `:i18n` — 多语言值

从嵌套的语言键中选择本地化的值。在选项中传递 `lang`。回退: `en` → 第一个可用值。

```synx
!active

title:i18n
  en Hello World
  zh 你好世界
  ru Привет мир
```

```javascript
const config = Synx.parse(text, { lang: 'zh' });
// config.title → "你好世界"
```

支持复数规则：使用 `:i18n:COUNT_FIELD`。

```synx
!active
count 5

label:i18n:count
  en
    one {count} item
    other {count} items
```

---

### `:secret` — 隐藏值

```synx
!active
api_key:secret sk-1234567890
```

### Auto-`{}` — 字符串插值

在 `!active` 模式下，任何包含 `{key}` 的字符串值都会自动插值——无需标记。

```synx
!active
name John
greeting 你好，{name}！

server
  host api.example.com
  port 443
api_url https://{server.host}:{server.port}/v1
```

**使用 `!include` 跨文件插值:**

```synx
!active
!include ./db.synx

conn_string postgresql://{host:db}:{port:db}/{name:db}
```

语法: `{key}` 本地键，`{key:alias}` 包含文件，`{key:include}` 唯一包含文件。

> **兼容性:** `:template` 标记仍然有效，但不再需要。

### `:include / :import` — 导入外部文件

```synx
!active
database:import ./db.synx
```

`:import` 是 `:include` 的别名（行为一致）。

| 机制 | 使用位置 | 作用 |
|---|---|---|
| `!include ./file.synx [alias]` | 文件级指令 | 提供 `{key:alias}` 插值数据 |
| `key:include ./file.synx` / `key:import ./file.synx` | 键级标记 | 将文件嵌入为子对象 |

### `:unique` — 列表去重

```synx
!active
tags:unique
  - action
  - rpg
  - action
```

结果：`["action", "rpg"]`

### `:split` — 字符串转数组

```synx
!active
colors:split red, green, blue
words:split:space hello world foo
```

分隔符关键词：`space`、`pipe`、`dash`、`dot`、`semi`、`tab`、`slash`

### `:join` — 数组转字符串

分隔符关键词：`space`、`pipe`、`dash`、`dot`、`semi`、`tab`、`slash`。默认：逗号。

```synx
!active
path:join:slash
  - home
  - user
  - docs
```

结果：`"home/user/docs"`

### `:geo` — 基于地区选择

```synx
!active
currency:geo
  - US USD
  - CN CNY
  - JP JPY
```

### `:clamp` — 数值钳制

```synx
!active
volume:clamp:0:100 150
```

结果：`100`

### `:round` — 四舍五入

```synx
!active
price:round:2 19.999
profit:calc:round:2 revenue * 0.337
```

### `:map` — 查找表

```synx
!active
status_code 1
status:map:status_code
  - 0 离线
  - 1 在线
  - 2 离开
```

结果：`"在线"`

### `:format` — 数字格式化

```synx
!active
price:format:%.2f 1234.5
id:format:%06d 42
```

结果：`"1234.50"`、`"000042"`

### `:fallback` — 文件路径回退

```synx
!active
icon:fallback:./default.png ./custom.png
```

### `:once` — 生成并持久化

```synx
!active
session_id:once uuid
app_seed:once random
build_time:once timestamp
```

生成类型：`uuid`（默认）、`random`、`timestamp`

### `:version` — 语义版本比较

```synx
!active
runtime:version:>=:18.0 20.11.0
```

结果：`true`。运算符：`>=` `<=` `>` `<` `==` `!=`

### `:watch` — 读取外部文件

```synx
!active
app_name:watch:name ./package.json
config:watch ./data.txt
```

### `:spam` — 访问频率限制

用于限制在时间窗口内对目标键/文件的解析次数。

语法: `:spam:MAX_CALLS[:WINDOW_SEC]`。
如果省略 `WINDOW_SEC`，默认值为 `1`。

```synx
!active
secret_token abc
access:spam:3:10 secret_token
burst_access:spam:5 secret_token
```

超过限制时，引擎会返回 `SPAM_ERR: ...`。

---

### `:prompt` — LLM 提示块

> 在 v3.6.0 中添加。

将子树格式化为带标签的 SYNX 代码围栏，供 LLM 使用。通过 `:prompt:标签` 指定标签。

```synx
!active
app_name "MyCoolApp"
version "2.1.0"

prompt_block:prompt:AppConfig
  app_name "MyCoolApp"
  version "2.1.0"
```

解析结果：

```json
{
  "app_name": "MyCoolApp",
  "version": "2.1.0",
  "prompt_block": "AppConfig (SYNX):\n```synx\napp_name \"MyCoolApp\"\nversion \"2.1.0\"\n```"
}
```

非常适合 AI 管道 — 无需额外序列化代码即可将结构化上下文传递给 LLM。

---

### `:vision` — 图像生成意图

> 在 v3.6.0 中添加。

元数据标记，将键标记为图像生成意图。引擎在解析时会透传此标记，保留供应用层处理。

```synx
!active
banner:vision "sunset landscape, 16:9, photorealistic"
```

---

### `:audio` — 音频生成意图

> 在 v3.6.0 中添加。

元数据标记，将键标记为音频生成意图。与 `:vision` 相同，该标记会被透传。

```synx
!active
greeting:audio "Welcome to our application"
```

---

## 🔒 约束

约束在解析时验证值。定义在键名后的 `[方括号]` 中。

| 约束 | 语法 | 描述 |
|---|---|---|
| `required` | `key[required]` | 必须有值 |
| `readonly` | `key[readonly]` | 只读 |
| `min:N` | `key[min:3]` | 最小长度/值 |
| `max:N` | `key[max:100]` | 最大长度/值 |
| `type:T` | `key[type:int]` | 强制类型 |
| `pattern:R` | `key[pattern:^\d+$]` | 正则验证 |
| `enum:A\|B` | `key[enum:light\|dark]` | 允许的值 |

```synx
!active
app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
```

---

## 🔗 标记链

```synx
!active
port:env:default:8080 PORT
profit:calc:round:2 revenue * margin
```

### ✅ 标记兼容性

常见且稳定的组合:

- `env:default`
- `calc:round`
- `split:unique`
- `split:join`（通过中间数组）

重要限制:

- 需要 `!active`，否则标记不会被解析。
- 部分标记依赖类型: `split` 需要字符串，`join` 需要数组，`round`/`clamp` 需要数字。
- 标记参数从链的右侧读取（例如 `clamp:min:max`、`round:n`、`map:key`）。
- 如果前一个标记改变了类型，后一个标记可能无法生效。

---

## � CLI 工具

> 在 v3.1.3 中添加。

通过 npm 全局安装：

```bash
npm install -g @aperturesyndicate/synx-format
```

### `synx convert` — 导出为其他格式

```bash
# SYNX → JSON
synx convert config.synx --format json

# SYNX → YAML（用于 Helm、Ansible、K8s）
synx convert config.synx --format yaml > values.yaml

# SYNX → TOML
synx convert config.synx --format toml

# SYNX → .env（用于 Docker Compose）
synx convert config.synx --format env > .env

# 严格模式（遇到任何标记错误即失败）
synx convert config.synx --format json --strict
```

### `synx validate` — CI/CD 验证

```bash
synx validate config.synx --strict
# 成功返回退出码 0，遇到 INCLUDE_ERR / WATCH_ERR / CALC_ERR / CONSTRAINT_ERR 返回 1
```

### `synx watch` — 实时重载

```bash
# 每次更改时打印 JSON
synx watch config.synx --format json

# 每次更改时执行命令（例如重载 Nginx）
synx watch config.synx --exec "nginx -s reload"
```

### `synx schema` — 从约束提取 JSON Schema

```bash
synx schema config.synx
# 基于 [required, min:N, max:N, type:T, enum:A|B, pattern:R] 输出 JSON Schema
```

---

## 📤 导出格式（JS/TS API）

> 在 v3.1.3 中添加。

将已解析的 SYNX 对象转换为 JSON、YAML、TOML 或 .env：

```typescript
import Synx from '@aperturesyndicate/synx-format';

const config = Synx.loadSync('config.synx');

// JSON
const json = Synx.toJSON(config);          // 格式化
const compact = Synx.toJSON(config, false); // 紧凑

// YAML
const yaml = Synx.toYAML(config);

// TOML
const toml = Synx.toTOML(config);

// .env（KEY=VALUE 格式）
const env = Synx.toEnv(config);            // APP_NAME=TotalWario
const prefixed = Synx.toEnv(config, 'APP'); // APP_APP_NAME=TotalWario
```

---

## 📋 Schema 导出

> 在 v3.1.3 中添加。

将 SYNX 约束提取为 JSON Schema 对象：

```typescript
const schema = Synx.schema(`
!active
app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
`);
```

结果：

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

> 在 v3.6.0 中添加。

比较两个已解析的 SYNX 对象，返回结构化差异：

```typescript
const old = Synx.parse('config_v1.synx');
const next = Synx.parse('config_v2.synx');
const diff = Synx.diff(old, next);
// diff.added    — 仅存在于新对象中的键
// diff.removed  — 仅存在于旧对象中的键
// diff.changed  — 两者都存在但值不同的键（{ from, to }）
// diff.unchanged — 值相同的键列表
```

适用于配置迁移验证、审计日志、CI 差异检查。

---

## �👁 文件监视器

> 在 v3.1.3 中添加。

监视 `.synx` 文件，每次更改时获取更新的配置：

```typescript
const handle = Synx.watch('config.synx', (config, error) => {
  if (error) {
    console.error('配置重载失败:', error.message);
    return;
  }
  console.log('配置已更新:', config.server.port);
}, { strict: true });

// 停止监视
handle.close();
```

---

## 🐳 部署指南

> 在 v3.1.3 中添加。

### Docker + Docker Compose

SYNX 作为所有服务配置的**唯一真实来源**。需要自己配置格式的服务（Nginx、Redis 等）在启动时接收生成的配置。

**模式：**

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   config.synx   │────▶│  启动脚本       │────▶│  nginx.conf     │
│ （单一文件）      │     │  或 CLI convert  │     │  .env           │
│  :env :default  │     │                 │     │  redis.conf     │
│  :template      │     │                 │     │  应用设置        │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

**步骤 1 — 编写配置：**

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

**步骤 2 — 为 Docker Compose 生成 .env：**

```bash
synx convert config.synx --format env > .env
```

**步骤 3 — 在 docker-compose.yml 中使用：**

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

### Nginx 配置生成

使用模板+启动脚本从 SYNX 生成 `nginx.conf`：

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

### Redis 连接

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

### PostgreSQL 连接

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

K8s 将密钥挂载为 `/run/secrets/` 下的文件。使用 `:watch` 读取：

```synx
!active

db_password:watch /run/secrets/db-password
api_key:watch /run/secrets/api-key
```

Docker Secrets 工作方式相同 — 挂载在 `/run/secrets/`。

### HashiCorp Vault

使用 Vault Agent 将密钥写入文件，然后用 `:watch` 读取：

```synx
!active

db_creds:watch:password /vault/secrets/database
api_key:watch:key /vault/secrets/api-key
```

或使用 Vault Agent 的 `env_template` 通过环境变量注入：

```synx
!active

db_password:env VAULT_DB_PASSWORD
api_key:env VAULT_API_KEY
```

### Helm / Kubernetes

将 SYNX 转换为 YAML 用作 Helm values：

```bash
synx convert config.synx --format yaml > helm/values.yaml
helm upgrade my-release ./chart -f helm/values.yaml
```

### Terraform

Terraform 接受 JSON 变量文件：

```bash
synx convert config.synx --format json > terraform.tfvars.json
terraform apply -var-file=terraform.tfvars.json
```

### CI/CD 管道验证

添加到 CI 管道中，在部署前检测配置错误：

```yaml
# GitHub Actions 示例
- name: 验证 SYNX 配置
  run: npx @aperturesyndicate/synx-format validate config.synx --strict
```

---

## �💻 代码示例

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

**运行时管理 (set / add / remove)：**

```typescript
import { Synx } from '@aperturesyndicate/synx-format';

const config = Synx.loadSync('./game.synx');

// 设置值
Synx.set(config, 'max_players', 100);
Synx.set(config, 'server.host', 'localhost');

// 获取值
const port = Synx.get(config, 'server.port'); // 8080

// 添加到列表
Synx.add(config, 'maps', 'Arena of Doom');

// 从列表删除
Synx.remove(config, 'maps', 'Arena of Doom');

// 删除整个键
Synx.remove(config, 'deprecated_key');

// 检查锁定状态
if (!Synx.isLocked(config)) {
  Synx.set(config, 'motd', '欢迎!');
}
```

> **注意：** 如果 `.synx` 文件包含 `!lock`，所有 `set`/`add`/`remove` 调用将抛出错误。

**访问方法 (JS/TS API)：**

- `Synx.get(obj, keyPath)` — 按点路径读取值。
- `Synx.set(obj, keyPath, value)` — 按点路径设置值。
- `Synx.add(obj, keyPath, item)` — 向数组追加元素。
- `Synx.remove(obj, keyPath, item?)` — 删除数组元素或删除整个键。
- `Synx.isLocked(obj)` — 检查配置是否被 `!lock` 锁定。

### Python

当前 `synx_native` 仅导出：`parse`、`parse_active`、`parse_to_json`。

Python 中可用以下方式实现 `get`/`set`/`add`/`remove` 等价操作：

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

## 🛠 编辑器支持

### Visual Studio Code

完整语言支持：语法高亮、IntelliSense（24个标记）、实时诊断（15项检查）、跳转到定义、格式化、颜色预览、`:calc` 内联提示、实时 JSON 预览。

### Visual Studio 2022

MEF 扩展：语法高亮、IntelliSense、错误标记、代码折叠、转换命令。

---

## 🏗 架构

```
synx-format/
├── crates/synx-core/          # Rust 核心 — 解析器 + 引擎
├── bindings/
│   ├── node/                  # NAPI-RS → npm 原生模块
│   └── python/                # PyO3 → PyPI 原生模块
├── packages/
│   ├── synx-js/               # 纯 TypeScript 解析器 + 引擎
│   ├── synx-vscode/           # VS Code 扩展
│   └── synx-visualstudio/     # Visual Studio 2022 扩展
├── publish-npm.bat
├── publish-pypi.bat
└── publish-crates.bat
```

---

### C# / .NET

**安装:**

```bash
dotnet add package APERTURESyndicate.Synx
```

> NuGet ID 为 `APERTURESyndicate.Synx`（不是 `Synx.Core` — 该名称已被占用）。参见 [nuget.org/packages/APERTURESyndicate.Synx](https://nuget.org/packages/APERTURESyndicate.Synx)。

这是一个**托管 .NET 8 实现** — 无需原生 DLL。解析器为纯 C#，通过一致性测试套件与 Rust 参考实现保持一致。

**SynxOptions:**

| 属性 | 类型 | 效果 |
|----------|------|--------|
| `Env` | `Dictionary<string, string>` | 为 `:env` 标记注入模拟环境 |
| `Region` | `string` | `:geo` 的值 |
| `Lang` | `string` | `:i18n` 的语言 |
| `BasePath` | `string` | `:include` 的基础目录 |
| `MaxIncludeDepth` | `int` | 限制包含嵌套深度 |

**SynxFormat API:**

| 方法 | 行为 |
|--------|-----------|
| `Parse(text)` | Static parse → `Dictionary<string, SynxValue>` |
| `ParseActive(text, options?)` | Parse + run engine (markers, constraints) |
| `ParseFull(text)` | Returns `SynxParseResult` with mode/tool flags, before resolve |
| `ParseFullActive(text, options?)` | Parse + resolve, returns full result |
| `ParseTool(text)` | `!tool` reshape |
| `ToJson(value)` / `ToJson(map)` | Canonical JSON string |
| `Stringify(value)` | SynxValue tree → SYNX text |
| `Serialize<T>(obj)` | Typed object → SYNX text |
| `Deserialize<T>(text, jsonOptions?)` | Parse then deserialize directly into `T` |
| `DeserializeActive<T>(text, synxOptions?, jsonOptions?)` | Parse + engine, then deserialize into `T` |
| `Deserialize(text, type)` | Runtime-type deserialization |
| `DeserializeActive(text, type, options?)` | Active variant with runtime type |
| `DeserializeAsync<T>(stream)` | Async stream-based deserialization |
| `DeserializeActiveAsync<T>(stream, options?)` | Async active variant |
| `SerializeAsync<T>(stream, obj)` | Async stream-based serialization |
| `LoadFileAsync<T>(path)` | One-line file → typed object |
| `SaveFileAsync(path, obj)` | One-line typed object → file |
| `FromJson(json)` | JSON string → SYNX text |
| `Format(text)` | Canonical SYNX (sorted keys, 2-space indent) |
| `Diff(a, b)` | Structural diff → list of changes |
| `DiffJson(textA, textB)` | Diff → JSON string |
| `Compile(text, resolved?)` | SYNX → `.synxb` binary |
| `Decompile(data)` | `.synxb` → SYNX text |
| `IsSynxb(data)` | Check magic header |

**Hello World:**

```csharp
using Synx;

// 1. Static parse
var text = """
name    Alice
age     30
active  true
roles
  - admin
  - editor
""";

var data = SynxFormat.Parse(text);

// SynxValue helpers — like Rust's value.as_str() / value["key"]
string? name = data["name"].AsString();  // "Alice"
long? age    = data["age"].AsInt();      // 30
bool? active = data["active"].AsBool();  // true

// ToString() unwraps primitives for display
Console.WriteLine(data["name"]);    // Alice
Console.WriteLine(data["age"]);     // 30

// JSON export (for interop)
Console.WriteLine(SynxFormat.ToJson(data));

// 2. Active mode with injected env (perfect for unit tests)
var config = SynxFormat.ParseActive("""
!active
port:env:default:8080        PORT
host:env:default:localhost   HOST
debug:env:default:false      DEBUG
app_name[required]           MyApp
""", new SynxOptions {
    Env = new Dictionary<string, string>(StringComparer.Ordinal) {
        ["PORT"] = "3000",
        ["HOST"] = "0.0.0.0"
    }
});

Console.WriteLine(SynxFormat.ToJson(config));
// {"app_name":"MyApp","debug":false,"host":"0.0.0.0","port":3000}
```

**类型化反序列化 — 直接转换为 POCO:**

```csharp
using Synx;
using System.Text.Json;

public record AppSettingsData(int RetryCount, int RetryDelayMinutes);

// Static parse
var settings = SynxFormat.Deserialize<AppSettingsData>("""
    RetryCount 3
    RetryDelayMinutes 5
    """);

// Active mode (resolves :env, :calc, constraints…)
var config = SynxFormat.DeserializeActive<AppSettingsData>(
    File.ReadAllText("config.synx"),
    synxOptions: new SynxOptions { BasePath = "/data" },
    jsonOptions: new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
```

这替换了手动模式:
```csharp
// before
JsonSerializer.Deserialize<AppSettingsData>(SynxFormat.ToJson(SynxFormat.Parse(text)))
// after
SynxFormat.Deserialize<AppSettingsData>(text)
```

**生产配置加载器:**

```csharp
using Synx;
using System.IO;

public static class AppConfig
{
    public static Dictionary<string, SynxValue> Load(string path)
    {
        var text = File.ReadAllText(path);
        return SynxFormat.ParseActive(text, new SynxOptions {
            BasePath = Path.GetDirectoryName(Path.GetFullPath(path))
        });
    }

    public static string? Get(Dictionary<string, SynxValue> config, string key)
    {
        return config.TryGetValue(key, out var v) ? v.AsString() : null;
    }

    public static long? GetInt(Dictionary<string, SynxValue> config, string key)
    {
        return config.TryGetValue(key, out var v) ? v.AsInt() : null;
    }
}
```

**Format — 规范化重格式:**

按字母顺序排列键，规范化为2空格缩进，去除注释。与 `synx format` CLI 输出相同。

```csharp
var messy = "age 30\n  name   Alice\n# comment";
var canonical = SynxFormat.Format(messy);
// age 30
// name Alice
```

**Diff — 结构比较:**

```csharp
var a = SynxFormat.Parse("name Alice\nage 30");
var b = SynxFormat.Parse("name Bob\nage 30\nemail bob@test.com");

var changes = SynxFormat.Diff(a, b);
foreach (var op in changes)
    Console.WriteLine(op);  // Changed: name Alice → Bob, Added: email

// Get diff as JSON
var json = SynxFormat.DiffJson("x 1\ny 2", "x 1\ny 3\nz new");
```

**编译 / 反编译 — 二进制 `.synxb`:**

```csharp
// Compile to binary
byte[] binary = SynxFormat.Compile("name Alice\nage 30");
File.WriteAllBytes("config.synxb", binary);

// Compile with resolved values (after ParseActive)
byte[] resolved = SynxFormat.Compile(activeText, resolved: true);

// Decompile back to SYNX text
string text = SynxFormat.Decompile(File.ReadAllBytes("config.synxb"));

// Check if a file is .synxb binary
bool isBinary = SynxFormat.IsSynxb(File.ReadAllBytes("config.synxb"));  // true
```

---

### C++ (C++17)

**设置:**

1. 从 monorepo 构建 `synx-c`：`cargo build --release -p synx-c`
2. 将 `bindings/c-header/include/synx.h` 和 `bindings/cpp/include/synx/synx.hpp` 复制到您的 include 路径
3. 链接 `libsynx_c`（`.so` / `.dylib` / `.dll`）

**CMakeLists.txt:**

```cmake
cmake_minimum_required(VERSION 3.15)
project(MyApp)
set(CMAKE_CXX_STANDARD 17)

add_executable(myapp main.cpp)
target_include_directories(myapp PRIVATE ${SYNX_INCLUDE_DIR})
target_link_directories(myapp PRIVATE ${SYNX_LIB_DIR})
target_link_libraries(myapp PRIVATE synx_c)
```

**API（仅头文件，`synx/synx.hpp`）:**

所有函数返回 `std::optional<std::string>` — 错误时返回 `nullopt`。`compile` 返回 `std::optional<std::vector<unsigned char>>`。

```cpp
#include "synx/synx.hpp"
#include <iostream>
#include <fstream>
#include <sstream>

int main() {
    // Read file
    std::ifstream f("config.synx");
    std::string text((std::istreambuf_iterator<char>(f)),
                      std::istreambuf_iterator<char>());

    // 1. Static parse → JSON
    auto json = synx::parse(text);
    if (!json) {
        std::cerr << "Parse failed\n";
        return 1;
    }
    std::cout << *json << "\n";

    // 2. Active mode
    auto active = synx::parse_active(text);
    if (active) std::cout << "Active: " << *active << "\n";

    // 3. Tool call document
    std::string tool_doc = "!tool\nweb_search\n  query hello world\n  lang en\n";
    auto tool = synx::parse_tool(tool_doc);
    if (tool) std::cout << "Tool: " << *tool << "\n";

    // 4. Diff two configs
    std::string v1 = "name Alice\nrole admin\n";
    std::string v2 = "name Alice\nrole editor\n";
    auto d = synx::diff(v1, v2);
    if (d) std::cout << "Diff: " << *d << "\n";

    // 5. Binary format
    auto bin = synx::compile(text, false);
    if (bin) {
        std::cout << "Compiled " << bin->size() << " bytes\n";
        auto restored = synx::decompile(*bin);
        if (restored) std::cout << "Restored: " << *restored << "\n";
    }

    return 0;
}
```

**完整函数参考:**

| 函数 | C++ 签名 | 说明 |
|----------|--------------|-------|
| `synx::parse` | `optional<string>(string)` | Static parse → JSON |
| `synx::parse_active` | `optional<string>(string)` | Active parse → JSON |
| `synx::stringify` | `optional<string>(string json)` | JSON → SYNX |
| `synx::format` | `optional<string>(string)` | Canonical SYNX |
| `synx::parse_tool` | `optional<string>(string)` | `!tool` reshape |
| `synx::diff` | `optional<string>(string, string)` | Diff JSON |
| `synx::compile` | `optional<vector<uint8_t>>(string, bool)` | → `.synxb` |
| `synx::decompile` | `optional<string>(vector<uint8_t>)` | `.synxb` → text |
| `synx::is_synxb` | `bool(vector<uint8_t>)` | Magic check |

> **内存：** C++ 头文件自动管理所有内存。无需手动调用 `synx_free` — optional 包装器在析构函数中处理。

---

### Go

**设置:**

绑定使用 cgo 并链接 `libsynx_c`。

```bash
go get github.com/APERTURESyndicate/synx-format/go
```

> **需要 cgo。** Go 绑定使用 cgo，需要 synx-core 共享库。有关特定平台的构建说明，请参阅模块 README。

**API:**

```go
import (
    "encoding/json"
    "fmt"
    "log"

    "github.com/APERTURESyndicate/synx-format/go/synx"
)

// Parse returns canonical JSON as a string
jsonStr, err := synx.Parse("name Alice\nage 30")
if err != nil {
    log.Fatal(err)
}

// Unmarshal into a Go map for field access
var data map[string]any
json.Unmarshal([]byte(jsonStr), &data)
fmt.Println(data["name"])  // Alice
fmt.Println(data["age"])   // 30

// Or unmarshal into a typed struct
type Config struct {
    Name   string  `json:"name"`
    Age    float64 `json:"age"`
}
var cfg Config
json.Unmarshal([]byte(jsonStr), &cfg)

// Stringify: JSON string → SYNX text
text, err := synx.Stringify(jsonStr)

// Format: canonical SYNX reformat
formatted, err := synx.Format("name   Alice\n  age 30")

// Diff: structural comparison → JSON
diff, err := synx.Diff("x 1\ny 2", "x 1\ny 3")

// Compile / Decompile: binary .synxb
bytes, err := synx.Compile("name Alice", false)
text, err = synx.Decompile(bytes)
```

| 函数 | 返回值 | 说明 |
|----------|---------|-------|
| `Parse(text string)` | `(string, error)` | JSON output |
| `ParseActive(text string)` | `(string, error)` | Active mode, real process env |
| `Stringify(json string)` | `(string, error)` | JSON → SYNX |
| `Format(text string)` | `(string, error)` | Canonical SYNX |
| `ParseTool(text string)` | `(string, error)` | Tool call reshape |
| `Diff(a, b string)` | `(string, error)` | Diff JSON |
| `Compile(text string, resolved bool)` | `([]byte, error)` | Binary format |
| `Decompile(data []byte)` | `(string, error)` | Binary → text |
| `IsSynxb(data []byte)` | `bool` | Magic check |

---

### Swift

**设置:**

通过 SynxEngine 的 Swift Package Manager 绑定（FFI 到 synx-core）。

```swift
// Package.swift dependency
.package(url: "https://github.com/APERTURESyndicate/synx-format", from: "3.6.0")
```

**API:**

```swift
import SynxEngine
import Foundation

// parse() returns a JSON string
let json = try SynxEngine.parse("name Alice\nage 30")

// Decode into native types via JSONSerialization or Codable
if let data = json.data(using: .utf8),
   let obj = try JSONSerialization.jsonObject(with: data) as? [String: Any] {
    print(obj["name"] as? String ?? "")  // Alice
    print(obj["age"] as? Int ?? 0)       // 30
}

// Or use Codable for typed access
struct Config: Codable { let name: String; let age: Int }
let config = try JSONDecoder().decode(Config.self, from: json.data(using: .utf8)!)

// stringify: JSON → SYNX text
let text = try SynxEngine.stringify(json)

// format, diff, compile, decompile also available
let formatted = try SynxEngine.format("name   Alice")
let changes = try SynxEngine.diff("x 1", "x 2")
```

---

### Kotlin / JVM

通过 synx-core 共享库的 JNA 绑定。适用于任何 JVM 语言。

```kotlin
// build.gradle.kts
implementation("com.aperturesyndicate:synx-engine:3.6.0")
```

**API:**

```kotlin
import com.aperturesyndicate.synx.SynxEngine
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

// parse() returns a canonical JSON string
val json = SynxEngine.parse("name Alice\nage 30")

// Decode with kotlinx.serialization, Gson, or Jackson
val obj = Json.parseToJsonElement(json).jsonObject
println(obj["name"]?.jsonPrimitive?.content)  // Alice
println(obj["age"]?.jsonPrimitive?.content)    // 30

// Or deserialize into a data class
@kotlinx.serialization.Serializable
data class Config(val name: String, val age: Int)
val config = Json.decodeFromString<Config>(json)

// stringify: JSON → SYNX text
val text = SynxEngine.stringify(json)

// format, diff, compile, decompile also available
val formatted = SynxEngine.format("name   Alice")
val changes = SynxEngine.diff("x 1", "x 2")
```

---

### WebAssembly

WASM 绑定是 npm 包 `@aperturesyndicate/synx-format` 的基础。它使用 wasm-bindgen 将 synx-core 编译为 WASM，并提供 JavaScript/TypeScript 胶水代码。

**直接 WASM 使用:**

```javascript
import init, { parse, stringify } from './synx_bg.wasm.js';

await init();  // load WASM module

const result = parse("name Alice\nage 30");
console.log(JSON.parse(result));
```

WASM 构建兼容 Cloudflare Workers、Deno Deploy 和其他支持 WASM 的边缘运行时。直接使用 npm 包 — 它将 WASM 二进制文件作为资产提供。

---

### Mojo

CPython 互操作绑定。内部使用 Python `synx_native` 扩展。

```mojo
from synx.interop import parse_json, parse_active_json, stringify_json
from synx.interop import format_synx, diff_json, compile_hex, decompile_hex

fn main() raises:
    # Parse → JSON string
    let json = parse_json("name Alice\nage 30")
    print(json)

    # Parse with !active engine
    let config = parse_active_json("!active\nport:env:default:8080 PORT")

    # Stringify: JSON → SYNX text
    let text = stringify_json(json)
    print(text)

    # Format: canonical SYNX reformat
    let formatted = format_synx("name   Alice\n  age 30")

    # Diff: structural comparison → JSON
    let changes = diff_json("x 1\ny 2", "x 1\ny 3")

    # Compile / decompile (hex-encoded binary)
    let hex = compile_hex("name Alice\nage 30", False)
    let back = decompile_hex(hex)
```

---


---

## 工具与编辑器

### VS Code扩展

**安装：**

```bash
code --install-extension APERTURESyndicate.synx-vscode
```

或在扩展面板中搜索**SYNX**。

**功能：**

- `.synx`文件的语法高亮
- 实时诊断（Tab键、不规则缩进、重复键、未知标记）
- 标记、约束和指令的自动补全
- 文档符号大纲
- 保存时格式化
- 标记的悬停文档
- 通过`:watch`实时重载

### synx-lsp — 语言服务器

```bash
cargo install --path crates/synx-lsp
```

服务器通过**stdio**使用标准LSP协议通信。以`synx-lsp`启动，无需参数。

| 功能 | 描述 |
|------------|-------------|
| 诊断 | Tab键、不规则缩进、重复键、未知标记/约束 |
| 自动补全 | 标记（`:env`、`:calc`、…）、约束、指令 |
| 文档符号 | 带嵌套的完整文档大纲 |

### Neovim

**LSP配置:**

```lua
-- init.lua
vim.lsp.start({
  name = 'synx-lsp',
  cmd = { 'synx-lsp' },
  root_dir = vim.fn.getcwd(),
  filetypes = { 'synx' },
})
```

**Tree-sitter:**

```lua
require('nvim-treesitter.configs').setup {
  ensure_installed = { 'synx' },
  highlight = { enable = true }
}
```

### 其他编辑器

**Helix:**

```toml
# ~/.config/helix/languages.toml
[[language]]
name = "synx"
scope = "source.synx"
file-types = ["synx"]
language-servers = ["synx-lsp"]

[language-server.synx-lsp]
command = "synx-lsp"
```

**Zed:** Settings → Language Servers → Add custom server：command `synx-lsp`，languages `SYNX`。

**Emacs (eglot):**

```elisp
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs
               '(synx-mode . ("synx-lsp"))))
```

**JetBrains:** Settings → Languages & Frameworks → Language Server → Add：command `synx-lsp`，file pattern `*.synx`。

**Sublime Text:**

```json
// LSP.sublime-settings
{
  "clients": {
    "synx-lsp": {
      "command": ["synx-lsp"],
      "selector": "source.synx",
      "enabled": true
    }
  }
}
```

**Visual Studio 2022+:** 从`integrations/visualstudio/`通过Extensions → Manage Extensions安装VSIX。

### MCP服务器

`synx-mcp`服务器将SYNX操作作为MCP工具暴露给任何MCP兼容客户端（Claude Desktop、Claude Code等）。

**可用工具：**

| 工具 | 描述 |
|------|-------------|
| `validate` | 检查`.synx`文件的语法和约束 |
| `parse` | 将SYNX字符串或文件解析为JSON |
| `format` | 规范化格式化SYNX文档 |
| `synx_read_path` | 读取文件（受`SYNX_MCP_ROOT`限制） |
| `synx_write_path` | 原子写入（temp + rename） |
| `synx_apply_patch` | 替换文件中的子字符串 |

**Claude Desktop配置:**

```json
{
  "mcpServers": {
    "synx": {
      "command": "node",
      "args": ["/path/to/synx-mcp/index.js"],
      "env": {
        "SYNX_MCP_ROOT": "/path/to/your/project"
      }
    }
  }
}
```

多个根目录：`"SYNX_MCP_ROOTS": "path1,path2"`。文件大小限制：10 MB。

---

## 二进制格式 (.synxb)

SYNX可以编译为二进制格式（`.synxb`）——用于快速解析和紧凑存储。二进制格式编码与文本SYNX相同的数据模型，但使用长度前缀二进制编码代替UTF-8文本。

**编译：**

```bash
synx compile config.synx -o config.synxb
```

```rust
use synx_core::compile;
let bytes = compile(&value)?;
std::fs::write("config.synxb", &bytes)?;
```

**反编译：**

```bash
synx decompile config.synxb
```

```rust
use synx_core::decompile;
let bytes = std::fs::read("config.synxb")?;
let value = decompile(&bytes)?;
```

**权衡：**

- **更快的解析** — 无需分词和缩进计数
- **更小的文件** — 键值驻留和紧凑的整数编码
- **不可人工编辑** — 人工修改的配置文件请使用文本SYNX
- **往返安全** — compile → decompile产生相同数据（非相同文本）

---

## 结构化Diff

比较两个SYNX文档，获取类型化的更改列表：添加、删除和修改，每个都带有点分隔的键路径。

```bash
synx diff old.synx new.synx
```

```rust
use synx_core::{parse, diff, DiffOp};

let a = parse("x 1\ny 2\nz old")?;
let b = parse("x 1\ny 3\nw new")?;

for op in diff(&a, &b) {
    match op {
        DiffOp::Added { path, value } =>
            println!("+ {} = {}", path, value),
        DiffOp::Removed { path } =>
            println!("- {}", path),
        DiffOp::Modified { path, from, to } =>
            println!("~ {} : {} → {}", path, from, to),
    }
}
```

```javascript
import { Synx } from '@aperturesyndicate/synx-format';

const ops = Synx.diff(
  Synx.parse("x 1\ny 2"),
  Synx.parse("x 1\ny 3\nz new")
);
// [{ op: "modified", path: "y", from: 2, to: 3 },
//  { op: "added",    path: "z", value: "new" }]
```

---

## JSON Schema

### 生成

从`!active` SYNX文档的约束生成Draft 2020-12 JSON Schema。

```bash
synx schema config.synx
```

```synx
!active
port[type:int, min:1024, max:65535] 8080
host[required, type:string] localhost
debug[type:bool] false
```

→

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {
    "port":  { "type": "integer", "minimum": 1024, "maximum": 65535 },
    "host":  { "type": "string" },
    "debug": { "type": "boolean" }
  },
  "required": ["host"]
}
```

**Rust API:**

```rust
use synx_core::schema_json;

let parsed = synx_core::parse(input)?;
let schema = schema_json::metadata_to_json_schema(&parsed)?;
```

**JavaScript API:**

```typescript
import { Synx } from '@aperturesyndicate/synx-format';

const schema = Synx.schema(`
!active
port[type:int, min:1024] 8080
host[required] localhost
`);
```

### 验证

针对生成的或外部的JSON Schema验证JSON数据。

```bash
# 针对JSON schema验证JSON文件
synx json-validate data.json schema.json

# 自验证：从!active文档生成schema并验证
synx validate --self-schema config.synx

# 使用外部schema验证
synx validate --json-schema schema.json config.synx
```

---

## 参考

### 一致性测试套件

所有官方绑定都针对相同的11个一致性测试用例进行测试。每个测试由一个`.synx`输入文件和一个`.expected.json`文件组成。为所有11个测试生成相同JSON的绑定被视为符合标准。

| # | 名称 | 测试内容 |
|---|------|---------------|
| 01 | `scalar-types` | 所有标量：string、int、float、bool、null |
| 02 | `nesting` | 嵌套对象（3+层深度） |
| 03 | `arrays` | 标量和对象的数组 |
| 04 | `type-casting` | `key(int)`、`key(float)`、`key(bool)`、`key(string)` |
| 05 | `comments` | `#`、`//`和`### ... ###`多行注释 |
| 06 | `multiline` | 通过缩进实现的多行值 |
| 07 | `mixed` | 混合结构：同级的对象+数组 |
| 08 | `strings-with-spaces` | 不带引号的含空格值 |
| 09 | `empty-values` | `key ""`（空字符串）、`key ~`（null） |
| 10 | `tool-mode` | `!tool`和`!schema` — 输出重构 |
| 11 | `llm-directive` | `!llm` — 数据树不变 |

```bash
# Rust
cargo test -p synx-core --test conformance

# C#
cd parsers/dotnet && dotnet test

# JavaScript
cd packages/synx-js && npm test
```

### 性能

**输入限制：**

SYNX强制执行硬限制以防御恶意输入：

| 限制 | 值 |
|-------|-------|
| 最大输入大小 | 16 MiB |
| 最大嵌套深度 | 128 层 |
| 最大数组元素数 | 1,000,000 |
| 最大块大小 | 1 MiB |
| :calc表达式长度 | 4,096 字符 |
| :include深度 | 16 层 |
| :include文件大小 | 10 MB |

**模糊测试:**

解析器使用三个目标进行持续模糊测试：

- `fuzz_parse` — 解析器+引擎（任意输入）
- `fuzz_compile` — 二进制编解码器往返（compile → decompile）
- `fuzz_format` — 格式化器稳定性

模糊测试语料库包含在长时间模糊测试中发现的**7,177**个有趣输入。这些在每次CI运行时用作回归测试。

```bash
cargo install cargo-fuzz
cargo fuzz run fuzz_parse
cargo fuzz run fuzz_compile
cargo fuzz run fuzz_format
```

### 安全性

**输入验证：** 切勿在没有大小限制的情况下解析不受信任的SYNX。解析器强制执行硬限制（16 MiB，深度128），但您应在应用程序级别添加额外检查。

**环境标记：** `:env`标记从进程环境中读取。确保在不受信任的用户可以影响SYNX源的上下文中，敏感环境变量不可访问。

**Include路径：** `:include`标记相对于文档解析路径。在不受信任的输入场景中，通过设置`SYNX_DISABLE_INCLUDE=1`或使用API标志`ParseOptions::no_includes()`来禁用`:include`。

> **切勿使用`:secret`解析不受信任的`!active`文档。**`:secret`标记连接到您的密钥后端。仅处理来自受信任源的`!active`文档。

### FAQ

**为什么不直接用YAML？**
YAML有许多陷阱：挪威问题（国家代码`NO`变为`false`）、自动类型强制转换、多文档流、具有复杂作用域规则的锚点，以及与SYNX更简单模型不同的空白处理。SYNX有意收窄功能面以消除这些意外。

**可以用Tab缩进吗？**
不可以。Tab是解析错误。使用2个空格（规范形式）或任何一致数量的空格。格式化器会标准化为2个空格。

**含空格的字符串需要引号吗？**
不需要。键（和可选标记）之后的所有内容都被视为值，包括空格。引号仅在表示空字符串时需要：`key ""`。

**总是需要`!active`吗？**
仅在需要标记（`:env`、`:calc`等）或约束（`[type:int]`）时。无需动态解析的数据文件在静态模式下完美运行。

**SYNX输出始终是有效的JSON吗？**
是的。`synx parse`和所有`parse()` API返回JSON兼容的值。`synx convert --to json`生成严格的JSON。

**规范会改变吗？**
SYNX v3.6.0是冻结规范。语法不会改变。新功能（如有）将是附加的，并在新的主版本号下发布。


---

## 🔗 链接

| 资源 | URL |
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
