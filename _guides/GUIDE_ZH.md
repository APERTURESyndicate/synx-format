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
  - [:ref — 链式引用](#ref--链式引用)
  - [:inherit — 块继承](#inherit--块继承)
  - [:i18n — 多语言值](#i18n--多语言值)
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
import { Synx } from '@aperturesyndicate/synx';
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

### `:ref` — 链式引用

类似 `:alias`，但将解析后的值传递给后续标记。支持简写计算。

```synx
!active

base_rate 50
quick_rate:ref base_rate
double_rate:ref:calc:*2 base_rate
```

简写 `:ref:calc:*2` 解析引用，然后将算术运算应用于该值。

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

---

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
npm install -g @aperturesyndicate/synx
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
import Synx from '@aperturesyndicate/synx';

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

## 👁 文件监视器

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
const Synx = require('@aperturesyndicate/synx');
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
  run: npx @aperturesyndicate/synx validate config.synx --strict
```

---

## �💻 代码示例

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

**运行时管理 (set / add / remove)：**

```typescript
import { Synx } from '@aperturesyndicate/synx';

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
