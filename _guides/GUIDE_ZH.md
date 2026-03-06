<p align="center">
  <a href="https://aperturesyndicate.com/branding/aperturesyndicate.png" target="_blank">
    <img src="https://aperturesyndicate.com/branding/aperturesyndicate.png" alt="APERTURESyndicate" width="400" />
  </a>
</p>

> **🔗 [查看徽标 →](https://aperturesyndicate.com/branding/aperturesyndicate.png)**

<h1 align="center">SYNX v3.0 — 完整指南</h1>

<p align="center">
  <strong>比 JSON 更好。比 YAML 更便宜。为 AI 和人类而生。</strong>
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

## 目录

- [设计哲学](#-设计哲学)
- [实际演示](#-实际演示)
- [工作原理](#-工作原理)
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
- [标记完整参考](#-标记完整参考)
  - [:env — 环境变量](#env--环境变量)
  - [:default — 默认值](#default--默认值)
  - [:calc — 算术表达式](#calc--算术表达式)
  - [:random — 随机选择](#random--随机选择)
  - [:alias — 引用另一个键](#alias--引用另一个键)
  - [:secret — 隐藏值](#secret--隐藏值)
  - [:template — 字符串插值](#template--字符串插值)
  - [:include — 导入外部文件](#include--导入外部文件)
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
```

---

## 🔥 活动模式 (`!active`)

将 `!active` 放在**第一行**以解锁标记和约束。

```synx
!active

port:env PORT
boss_hp:calc base_hp * 5
```

---

## 🧩 标记完整参考

SYNX v3.0 提供 **20 个标记**。每个标记都是通过 `:标记` 语法附加到键的函数。

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

### `:random` — 随机选择

```synx
!active
loot:random 70 20 10
  - common
  - rare
  - legendary
```

### `:alias` — 引用另一个键

```synx
!active
admin_email alex@example.com
billing:alias admin_email
```

### `:secret` — 隐藏值

```synx
!active
api_key:secret sk-1234567890
```

### `:template` — 字符串插值

```synx
!active
name John
greeting:template 你好，{name}！
```

### `:include` — 导入外部文件

```synx
!active
database:include ./db.synx
```

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

分隔符关键词：`space`、`pipe`、`dash`、`dot`、`semi`、`tab`

### `:join` — 数组转字符串

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

---

## 💻 代码示例

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

## 🛠 编辑器支持

### Visual Studio Code

完整语言支持：语法高亮、IntelliSense（20个标记）、实时诊断（15项检查）、跳转到定义、格式化、颜色预览、`:calc` 内联提示、实时 JSON 预览。

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

## 🔗 链接

| 资源 | URL |
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
