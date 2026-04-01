<p align="center">
  <a href="https://media.aperturesyndicate.com/asother/as/branding/png/aperturesyndicate.png" target="_blank">
    <img src="https://media.aperturesyndicate.com/asother/as/branding/png/aperturesyndicate.png" alt="APERTURESyndicate" width="400" />
  </a>
</p>

> **🔗 [Посмотреть логотип →](https://media.aperturesyndicate.com/asother/as/branding/png/aperturesyndicate.png)**

<h1 align="center">SYNX v3.6 — Полное руководство</h1>

<p align="center">
  <strong>Лучше чем JSON. Дешевле чем YAML. Создан для AI и людей.</strong>
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

## Содержание

- [Философия](#-философия)
- [Смотри в действии](#-смотри-в-действии)
- [Как это работает](#-как-это-работает)
- [Модель безопасности (v3.5.0+)](#-модель-безопасности-v350)
- [Производительность и бенчмарки](#-производительность-и-бенчмарки)
- [Установка](#-установка)
- [Грамматика](#-грамматика)
  - [Базовый синтаксис](#базовый-синтаксис)
  - [Вложенность](#вложенность)
  - [Списки](#списки)
  - [Приведение типов](#приведение-типов)
  - [Многострочный текст](#многострочный-текст)
  - [Комментарии](#комментарии)
- [Активный режим (`!active`)](#-активный-режим-active)
- [Режим блокировки (`!lock`)](#-режим-блокировки-lock)
- [Директива `!include`](#-директива-include)
- [Каноническая форма (`format`)](#-каноническая-форма-format)
- [Маркеры — полный справочник](#-маркеры--полный-справочник)
  - [:env](#env--переменная-окружения)
  - [:default](#default--значение-по-умолчанию)
  - [:calc](#calc--арифметическое-выражение)
  - [:random](#random--случайный-выбор)
  - [:alias](#alias--ссылка-на-другой-ключ)
  - [:ref](#ref--ссылка-с-цепочкой)
  - [:inherit](#inherit--наследование-блоков)
  - [:i18n](#i18n--мультиязычные-значения)
  - [:secret](#secret--скрытое-значение)
  - [auto-{}](#auto---интерполяция-строк)
  - [:include / :import](#include--import--импорт-внешнего-файла)
  - [:unique](#unique--дедупликация-списка)
  - [:split](#split--строка-в-массив)
  - [:join](#join--массив-в-строку)
  - [:geo](#geo--выбор-по-региону)
  - [:clamp](#clamp--ограничение-числа)
  - [:round](#round--округление)
  - [:map](#map--таблица-подстановки)
  - [:format](#format--форматирование-чисел)
  - [:fallback](#fallback--запасной-файл)
  - [:once](#once--сгенерировать-и-сохранить)
  - [:version](#version--сравнение-версий)
  - [:watch](#watch--чтение-внешнего-файла)
  - [:spam](#spam--ограничение-частоты-доступа)
- [Ограничения (Constraints)](#-ограничения-constraints)
- [Цепочки маркеров](#-цепочки-маркеров)
- [Бинарный формат (`.synxb`)](#-бинарный-формат-synxb)
- [LLM Tool Use (`!tool`)](#-llm-tool-use-tool)
- [C++ SDK (`bindings/cpp`)](#-c-sdk-bindingscpp)
- [Go (`bindings/go`, cgo)](#-go-bindingsgo-cgo)
- [Swift (`bindings/swift`, SwiftPM)](#-swift-bindingsswift-swiftpm)
- [Kotlin / JVM (`bindings/kotlin`, JNA)](#-kotlin--jvm-bindingsskotlin-jna)
- [Mojo (`bindings/mojo` + `synx_native`)](#-mojo-bindingsmojo--synx_native)
- [C# / .NET парсер (`Synx.Core`)](#-c---net-парсер-synxcore)
- [CLI (Rust) — `synx`](#-cli-rust--synx)
- [Примеры кода](#-примеры-кода)
  - [JavaScript / TypeScript](#javascript--typescript)
  - [Python](#python)
  - [Rust](#rust)
- [Поддержка редакторов](#-поддержка-редакторов)
- [Архитектура](#-архитектура)
- [Спецификация](#-спецификация)
- [Ссылки](#-ссылки)

---

## 💡 Философия

Конфигурация — это основа каждого приложения. Но стандартные форматы — **JSON** и **YAML** — никогда не были предназначены для этой задачи:

| Проблема | JSON | YAML | SYNX |
|---|:---:|:---:|:---:|
| Требуются кавычки для строк и ключей | ✓ | ✗ | ✗ |
| Запятые ломают парсер | ✗ | — | ✓ |
| Значимые отступы | — | ✗ (опасно) | ✓ (безопасно, 2 пробела) |
| Комментарии | ✗ | ✓ | ✓ |
| Переменные окружения | ✗ | ✗ | ✓ встроено |
| Вычисляемые значения | ✗ | ✗ | ✓ встроено |
| AI-токены (110 ключей) | ~3300 символов | ~2500 символов | **~2000 символов** |
| Читаемость | Низкая | Средняя | **Высокая** |

SYNX построен на трёх принципах:

1. **Минимальный синтаксис** — Ключ, пробел, значение. Всё. Без кавычек, запятых, скобок, двоеточий.
2. **Активный по дизайну** — Конфиги — это не просто данные, это логика. Переменные окружения, математика, ссылки, рандом и валидация — всё встроено в сам формат.
3. **Экономия токенов** — Каждый символ важен при работе с LLM. SYNX использует на 30–40% меньше токенов, чем JSON для тех же данных.

> **SYNX — это не замена JSON. Это то, чем JSON должен был быть.**

---

## 🎬 Смотри в действии

### Запись данных — чисто и просто

Просто **ключ**, **пробел**, **значение**. Без кавычек, запятых и скобок:

<p align="center">
  <a href="https://aperturesyndicate.com/branding/gifs/synx/synx.gif" target="_blank">
    <img src="https://aperturesyndicate.com/branding/gifs/synx/synx.gif" alt="Запись статического SYNX" width="720" />
  </a>
</p>

> **📺 [Смотреть демонстрацию →](https://aperturesyndicate.com/branding/gifs/synx/synx.gif)**

### Режим `!active` — конфиги с логикой

Добавь `!active` на первую строку — и конфиг оживает, с функциями прямо внутри формата:

<p align="center">
  <a href="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" target="_blank">
    <img src="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" alt="Запись активного SYNX с маркерами" width="720" />
  </a>
</p>

> **📺 [Смотреть демонстрацию →](https://aperturesyndicate.com/branding/gifs/synx/synx2.gif)**

---

## ⚙ Как это работает

Конвейер SYNX состоит из **двух этапов** — и это разделение ключевое для производительности:

```
┌─────────────┐         ┌─────────────┐         ┌──────────────┐
│  Файл .synx │ ──────► │   ПАРСЕР    │ ──────► │   РЕЗУЛЬТАТ  │
│  (текст)    │         │ (всегда)    │         │  (JS-объект) │
└─────────────┘         └──────┬──────┘         └──────────────┘
                               │
                          есть !active?
                               │
                          ┌────▼────┐
                          │ ДВИЖОК  │
                          │(маркеры)│
                          └─────────┘
```

### Этап 1 — Парсер

**Парсер** читает текст и строит дерево ключей-значений. Он обрабатывает:
- Пары ключ-значение (первый пробел разделяет ключ и значение)
- Вложенность (отступы по 2 пробела)
- Списки (`- элемент`)
- Приведение типов (`ключ(int) значение`)
- Комментарии (`#` и `//`)
- Многострочный текст (`|`)

Парсер записывает маркеры (`:env`, `:calc` и т.д.) как **метаданные** к каждому ключу, но **не выполняет их**. Это значит, что **добавление новых маркеров не замедляет парсинг**.

### Этап 2 — Движок (только с `!active`)

Если файл начинается с `!active`, **движок** обходит разобранное дерево и разрешает каждый маркер. Каждый обработчик запускается только для ключей, которые явно его используют.

**Файлы без `!active` вообще не запускают движок.** Парсер обрабатывает их и возвращает результат мгновенно.

### Автоматическое переключение движка (Node.js)

В Node.js библиотека автоматически выбирает оптимальный движок:

| Размер файла | Движок | Почему |
|---|---|---|
| < 5 КБ | Чистый TypeScript | Нет накладных расходов на запуск |
| ≥ 5 КБ | Нативный Rust (NAPI) | Быстрее на больших файлах |

Если нативный Rust-биндинг не установлен, всегда используется чистый TypeScript.

---

## Модель безопасности (v3.5.0+)

SYNX сохраняет весь функционал маркеров, но добавляет защитные ограничения выполнения для файловых и вычислительных операций.

- **Path jail для файловых маркеров**: `:include`, `:import`, `:watch`, `:fallback` работают только внутри `basePath`. Абсолютные пути и выход через `../` за пределы базы блокируются.
- **Ограничение глубины вложений**: для цепочек include/watch по умолчанию максимум `16` уровней (настраивается).
  Rust опция: `max_include_depth`
  JS опция: `maxIncludeDepth`
- **Ограничение размера файла**: файлы больше `10 MB` отклоняются до чтения.
- **Ограничение длины выражения `:calc`**: выражения длиннее `4096` символов отклоняются.
- **Поведение движка**: парсер по-прежнему только записывает метаданные, обработчики маркеров запускаются только в `!active`.

Важно:
- SYNX не выполняет произвольный код из конфигов (нет YAML-подобных object-конструкторов и нет `eval`).

---

## 📊 Производительность и бенчмарки

Все бенчмарки реальные, на стандартном SYNX-конфиге из 110 ключей (2.5 КБ):

### Rust (criterion, прямой вызов)

| Бенчмарк | Время |
|---|---|
| `Synx::parse` (110 ключей) | **~39 мкс** |
| `parse_to_json` (110 ключей) | **~42 мкс** |
| `Synx::parse` (4 ключа) | **~1.2 мкс** |

### Node.js (50 000 итераций)

| Парсер | мкс/парсинг | vs JSON | vs YAML |
|---|---:|---:|---:|
| `JSON.parse` (3.3 КБ JSON) | 6.08 мкс | 1× | — |
| **`synx-js` чистый TS** | **39.20 мкс** | 6.4× | **2.1× быстрее** |
| `js-yaml` (2.5 КБ YAML) | 82.85 мкс | 13.6× | 1× |

### Python (10 000 итераций)

| Парсер | мкс/парсинг | vs YAML |
|---|---:|---:|
| `json.loads` (3.3 КБ) | 13.04 мкс | — |
| **`synx_native.parse`** | **55.44 мкс** | **67× быстрее** |
| `yaml.safe_load` (2.5 КБ) | 3 698 мкс | 1× |

### Стоимость в AI-токенах (110 ключей, токенизатор GPT-4)

| Формат | Символов | Токенов | Цена @ $0.01/1K |
|---|---:|---:|---:|
| JSON | ~3 300 | ~980 | $0.0098 |
| YAML | ~2 500 | ~760 | $0.0076 |
| **SYNX** | **~2 000** | **~580** | **$0.0058** |

> SYNX экономит **~40% AI-токенов** по сравнению с JSON и **~24%** по сравнению с YAML.

---

## 📦 Установка

### Node.js / Браузер

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

### Расширение VS Code

Ищи **"SYNX"** в панели расширений, или:

```bash
code --install-extension APERTURESyndicate.synx-vscode
```

### Visual Studio 2022

Скачай `.vsix` из [GitHub Releases](https://github.com/APERTURESyndicate/synx-format/releases) и дважды кликни для установки.

---

## 📝 Грамматика

### Базовый синтаксис

Базовое правило: **ключ** `(пробел)` **значение**.

Первый пробел разделяет ключ и значение. Всё после первого пробела — это значение, включая дополнительные пробелы.

```synx
name John
age 25
phrase Я люблю программирование!
empty_value
```

**Результат:**

```json
{
  "name": "John",
  "age": 25,
  "phrase": "Я люблю программирование!",
  "empty_value": null
}
```

> Числа, булевы значения (`true`/`false`) и `null` определяются автоматически. Всё остальное — строка.

> **Значения в кавычках** не приводятся автоматически: `"null"`, `"true"`, `"42"` остаются строками.

Правила авто-определения типа парсером (если нет явного `(type)`):

1. Точное `true`/`false` -> Bool
2. Точное `null` -> Null
3. Паттерн целого числа -> Int
4. Паттерн десятичного числа -> Float
5. Иначе -> String

```synx
status "null"
enabled "true"
count "42"
```

```json
{
  "status": "null",
  "enabled": "true",
  "count": "42"
}
```

---

### Вложенность

Отступы создают иерархию — **2 пробела** на уровень:

```synx
server
  host 0.0.0.0
  port 8080
  ssl
    enabled true
    cert /etc/ssl/cert.pem

database
  host localhost
  port 5432
```

---

### Списки

Строки, начинающиеся с `- ` (дефис + пробел), создают массивы:

```synx
fruits
  - Apple
  - Banana
  - Cherry
```

```json
{ "fruits": ["Apple", "Banana", "Cherry"] }
```

---

### Приведение типов

Принудительное указание типа через `(тип)` после имени ключа:

```synx
zip_code(string) 90210
id(int) 007
ratio(float) 3
enabled(bool) 1
```

Доступные типы: `int`, `float`, `bool`, `string`.

#### Генерация случайных значений

Генерируй случайные значения при парсинге с помощью `(random)`:

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

Доступные типы: `(random)` (int), `(random:int)`, `(random:float)`, `(random:bool)`.

> Значения генерируются при каждом парсинге — каждый вызов даёт новые значения.

---

### Многострочный текст

Используй оператор `|` для многострочных строк:

```synx
description |
  Это длинное описание,
  которое занимает несколько строк.
```

---

### Комментарии

Три стиля — все игнорируются парсером:

```synx
# Это хеш-комментарий
// Это слэш-комментарий

name John  # инлайн-комментарий

###
Это блочный комментарий.
Всё между ### игнорируется.
Каждую строку не нужно помечать.
###
```

В VSCode-расширении в комментариях поддерживается форматирование:
- `*курсив*` — зелёный
- `**жирный**` — фиолетовый
- `***жирный+курсив***` — золотой
- `` `код` `` — оранжевый с фоном

---

## 🔥 Активный режим (`!active`)

Помести `!active` на **первую строку**, чтобы разблокировать маркеры и ограничения.

Без `!active` все маркеры типа `:env`, `:calc`, `:random` воспринимаются как **обычный текст** в имени ключа.

```synx
!active

# Теперь маркеры работают!
port:env PORT
boss_hp:calc base_hp * 5
```

---

## 🔐 Режим блокировки (`!lock`)

Добавь `!lock`, чтобы запретить внешнему коду модифицировать значения через `Synx.set()`, `Synx.add()`, `Synx.remove()`. Внутренние маркеры SYNX работают нормально.

```synx
!active
!lock

max_players 100
server_name MyServer
greeting:random
  - Привет!
  - Добро пожаловать!
```

```typescript
const config = Synx.loadSync('./config.synx');

Synx.set(config, 'max_players', 200);
// ❌ ошибка: "SYNX: Cannot set "max_players" — config is locked (!lock)"

console.log(config.max_players); // ✅ 100 (чтение всегда разрешено)
```

Используй `Synx.isLocked(config)` для проверки.

---

## 📎 Директива `!include`

Директива `!include` импортирует ключи другого `.synx` файла для использования в `{ключ:алиас}` интерполяции. В отличие от маркера `:include` (встраивает файл как дочерний блок), `!include` делает ключи верхнего уровня доступными для строковой интерполяции.

```synx
!active
!include ./db.synx
!include ./cache.synx redis

app_name MyApp
db_url postgresql://{host:db}:{port:db}/{name:db}
cache_url redis://{host:redis}:{port:redis}
```

**Правила алиасов:**

| Директива | Алиас | Доступ |
|---|---|---|
| `!include ./db.synx` | `db` (авто из имени файла) | `{host:db}` |
| `!include ./cache.synx redis` | `redis` (явный) | `{host:redis}` |
| `!include ./config.synx` (единственный include) | — | `{host:include}` |

---

## 🧹 Каноническая форма (`format`)

`Synx.format()` перезаписывает любой `.synx`-файл в единственную нормализованную форму.

**Что делает:**
- **Сортирует все ключи по алфавиту** на каждом уровне вложенности
- **Нормализует отступы** до ровно 2 пробелов на уровень
- **Удаляет комментарии** — каноническая форма содержит только данные
- **Одна пустая строка** между блоками верхнего уровня (объекты и списки)
- **Сохраняет директивы** (`!active`, `!lock`) вверху файла
- **Порядок элементов списков сохраняется** — сортируются только именованные ключи

### Зачем это нужно для Git

Без канонической формы два программиста пишут один и тот же конфиг по-разному:

```synx
# Программист A              # Программист B
server                       server
    port 8080                  host 0.0.0.0
    host 0.0.0.0               port 8080
```

`git diff` показывает весь блок как изменённый — хотя данные идентичны.

После `Synx.format()` оба получают:

```synx
server
  host 0.0.0.0
  port 8080
```

Одна форма. Ноль шума в диффах.

### Использование

**JavaScript / TypeScript:**

```typescript
import { Synx } from '@aperturesyndicate/synx-format';
import * as fs from 'fs';

const raw = fs.readFileSync('config.synx', 'utf-8');
fs.writeFileSync('config.synx', Synx.format(raw));
```

**Rust:**

```rust
use synx_core::Synx;

let raw = std::fs::read_to_string("config.synx").unwrap();
std::fs::write("config.synx", Synx::format(&raw)).unwrap();
```

---

## 🧩 Маркеры — полный справочник

SYNX v3.6 содержит **24 маркера**. Каждый маркер — это функция, присоединяемая к ключу через синтаксис `:маркер`.

> **Все маркеры требуют режим `!active`.**

---

### `:env` — Переменная окружения

Читает системную переменную окружения при парсинге.

```synx
!active

port:env PORT
api_url:env API_BASE_URL
```

В сочетании с `:default`:

```synx
!active

port:env:default:8080 PORT
```

Если `PORT` не установлен → возвращает `8080`.

---

### `:default` — Значение по умолчанию

Устанавливает запасное значение, если основное пустое или отсутствует.

```synx
!active

theme:default dark
port:env:default:3000 PORT
```

---

### `:calc` — Арифметическое выражение

Вычисляет математическое выражение. Ссылается на другие числовые ключи по имени.

```synx
!active

base_price 100
tax_rate 0.2
tax:calc base_price * tax_rate
total:calc base_price + tax
```

```json
{ "tax": 20, "total": 120 }
```

Поддерживается доступ к вложенным полям через dot-path:

```synx
!active

stats
  base_hp 100
  multiplier 3
  armor 25

total_hp:calc stats.base_hp * stats.multiplier
effective_hp:calc total_hp + stats.armor
```

```json
{
  "stats": { "base_hp": 100, "multiplier": 3, "armor": 25 },
  "total_hp": 300,
  "effective_hp": 325
}
```

Операторы: `+` `-` `*` `/` `%` `(` `)`.

> **Безопасный вычислитель** — без `eval()`. Только арифметика.

---

### `:random` — Случайный выбор

Выбирает случайный элемент из списка.

**Равная вероятность:**

```synx
!active

greeting:random
  - Привет!
  - Добро пожаловать!
  - Здравствуйте!
```

**С весами:**

```synx
!active

loot:random 70 20 10
  - common
  - rare
  - legendary
```

---

### `:alias` — Ссылка на другой ключ

Копирует разрешённое значение другого ключа. Измени источник один раз — все алиасы обновятся.

```synx
!active

admin_email alex@example.com
complaints_email:alias admin_email
billing_email:alias admin_email
```

`:alias` разрешает источник, поэтому можно ссылаться на ключи с другими маркерами:

```synx
!active

base_port:env:default:3000 PORT
api_port:alias base_port
```

> **`:alias` vs `:ref`:** Оба копируют значение, но `:alias` — терминальная операция. Используй `:ref`, если нужно продолжить цепочку (например, `:ref:calc:*2`).

---

### `:ref` — Ссылка с цепочкой

Как `:alias`, но передаёт разрешённое значение дальше по цепочке маркеров.

```synx
!active

base_rate 50
quick_rate:ref base_rate
double_rate:ref:calc:*2 base_rate
boosted_rate:ref:calc:+25 base_rate
```

**Сокращённый синтаксис:** `:ref:calc:*2` разрешает ссылку, затем применяет оператор. Поддерживаются: `+`, `-`, `*`, `/`, `%`.

**Пример — масштабирование сложности:**

```synx
!active

base_hp 100
easy_hp:ref:calc:*0.5 base_hp
hard_hp:ref:calc:*2 base_hp
```

> **Когда `:ref`, а когда `:alias`:** Используй `:ref`, если нужно дополнительно обработать значение. Для простого копирования — `:alias`.

---

### `:inherit` — Наследование блоков

Объединяет все поля родительского блока с дочерним. Значения дочернего блока имеют приоритет. Префикс `_` делает блок приватным — он исключается из итогового результата.

```synx
!active

_base_resource
  weight 10
  stackable true
  category misc

steel:inherit:_base_resource
  weight 25
  material metal
```

Поддерживается наследование от нескольких родителей. Родители применяются слева направо: более правый родитель переопределяет более левый, а дочерний блок переопределяет всех.

```synx
!active

_movable
  speed 10
  can_move true

_damageable
  hp 100
  armor 5

_attackable
  damage 15
  range 1

tank:inherit:_movable:_damageable:_attackable
  name Tank
  armor 50
  damage 120
```

**Многоуровневое наследование:**

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

Цепочки наследования работают: `_entity` → `_enemy` → `goblin`. Приватные блоки (`_entity`, `_enemy`) исключаются из вывода.

---

### `:i18n` — Мультиязычные значения

Выбирает локализованное значение из вложенных ключей-языков. Передайте `lang` в опциях для выбора языка. Откат: `en` → первое доступное.

```synx
!active

title:i18n
  en Hello World
  ru Привет мир
  de Hallo Welt
```

```javascript
const config = Synx.parse(text, { lang: 'ru' });
// config.title → "Привет мир"
```

Плюрализация: укажи поле-счётчик через `:i18n:COUNT_FIELD`.

```synx
!active

item_count 5

label:i18n:item_count
  en
    one {count} item found
    other {count} items found
  ru
    one {count} предмет найден
    few {count} предмета найдено
    many {count} предметов найдено
    other {count} предметов найдено
```

```javascript
const en = Synx.parse(text, { lang: 'en' });
// en.label -> "5 items found"

const ru = Synx.parse(text, { lang: 'ru' });
// ru.label -> "5 предметов найдено"
```

`{count}` подставляется автоматически.

---

### `:secret` — Скрытое значение

Значение доступно коду, но скрыто в логах и `JSON.stringify()`.

```synx
!active

api_key:secret sk-1234567890abcdef
```

```javascript
console.log(config.api_key);          // "[SECRET]"
console.log(config.api_key.reveal()); // "sk-1234567890abcdef"
```

---

### Auto-`{}` — Интерполяция строк

В режиме `!active` любое строковое значение с `{ключ}` автоматически интерполируется — маркер не нужен. Поддерживает точечные пути.

```synx
!active

first_name John
last_name Doe
greeting Привет, {first_name} {last_name}!

server
  host api.example.com
  port 443
api_url https://{server.host}:{server.port}/v1
```

**Кросс-файловая интерполяция с `!include`:**

```synx
!active
!include ./db.synx

conn_string postgresql://{host:db}:{port:db}/{name:db}
```

Синтаксис: `{ключ}` — локальный, `{ключ:алиас}` — из включённого файла, `{ключ:include}` — из единственного включённого файла.

> **Совместимость:** Маркер `:template` по-прежнему работает, но не нужен — auto-`{}` обрабатывает интерполяцию автоматически.

---

### `:include / :import` — Импорт внешнего файла

Импортирует содержимое другого `.synx` файла как дочерний объект. Путь относительный.

`:import` — алиас `:include` (поведение одинаковое), добавлен для снижения путаницы с директивой `!include`.

```synx
!active

database:import ./db.synx
logging:include ./logging.synx
```

Сравнение механизмов импорта:

| Механизм | Где используется | Что делает |
|---|---|---|
| `!include ./file.synx [alias]` | директива вверху файла | делает ключи доступными для `{key:alias}` интерполяции |
| `key:include ./file.synx` / `key:import ./file.synx` | маркер на ключе | встраивает файл как дочерний объект по ключу |

---

### `:unique` — Дедупликация списка

Удаляет дубликаты из списка:

```synx
!active

tags:unique
  - action
  - rpg
  - action
```

Результат: `["action", "rpg"]`

---

### `:split` — Строка в массив

Разбивает строку по разделителю в массив.

```synx
!active

colors:split red, green, blue
words:split:space hello world foo
```

Ключевые слова разделителей: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`, `slash`. По умолчанию: запятая.

---

### `:join` — Массив в строку

Объединяет элементы списка в строку.

Ключевые слова разделителей: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`, `slash`. По умолчанию: запятая.

```synx
!active

path:join:slash
  - home
  - user
  - documents
```

Результат: `"home/user/documents"`

---

### `:geo` — Выбор по региону

Выбирает значение по географическому региону пользователя.

```synx
!active

currency:geo
  - US USD
  - EU EUR
  - RU RUB
```

---

### `:clamp` — Ограничение числа

Зажимает число в диапазон `[min, max]`.

```synx
!active

volume:clamp:0:100 150
```

Результат: `100`

---

### `:round` — Округление

Округляет число до N знаков после запятой. Особенно полезно после `:calc`.

```synx
!active

price:round:2 19.999
profit:calc:round:2 revenue * 0.337
```

---

### `:map` — Таблица подстановки

Подставляет значение по ключу-источнику через таблицу поиска.

```synx
!active

status_code 2
status_label:map:status_code
  - 0 офлайн
  - 1 онлайн
  - 2 отошёл
```

Результат: `"отошёл"`

---

### `:format` — Форматирование чисел

Форматирует число в стиле printf.

```synx
!active

price:format:%.2f 1234.5
order_id:format:%06d 42
```

Результат: `"1234.50"`, `"000042"`

---

### `:fallback` — Запасной файл

Если файл по пути не существует, использует запасной путь.

```synx
!active

icon:fallback:./default.png ./custom.png
```

---

### `:once` — Сгенерировать и сохранить

Генерирует значение **один раз** и сохраняет в `.synx.lock`. Все последующие парсинги возвращают то же значение.

```synx
!active

session_id:once uuid
app_seed:once random
created_at:once timestamp
```

Типы генерации: `uuid` (по умолчанию), `random`, `timestamp`.

---

### `:version` — Сравнение версий

Сравнивает версию со требуемой. Возвращает булево значение.

```synx
!active

runtime:version:>=:18.0 20.11.0
```

Результат: `true`

Операторы: `>=` `<=` `>` `<` `==` `!=`

---

### `:watch` — Чтение внешнего файла

Читает внешний файл при парсинге. Можно извлечь конкретный ключ из JSON или SYNX.

```synx
!active

app_name:watch:name ./package.json
db_host:watch:database.host ./config.synx
```

---

### `:spam` — Ограничение частоты доступа

Ограничивает количество обращений к целевому ключу/файлу в заданное окно времени.

Синтаксис: `:spam:MAX_CALLS[:WINDOW_SEC]`.
Если `WINDOW_SEC` не указан, используется значение `1`.

```synx
!active

secret_token abc
access:spam:3:10 secret_token

# WINDOW_SEC по умолчанию = 1
burst_access:spam:5 secret_token
```

При превышении лимита движок возвращает `SPAM_ERR: ...`.

---

### `:prompt` — Форматирование для LLM-промпта

Преобразует поддерево (объект) в SYNX-строку, обёрнутую в маркированный блок кода — готовую для вставки в системный промпт LLM.

Синтаксис: `:prompt:МЕТКА`. Если метка не указана, используется имя ключа.

```synx
!active

memory:prompt:Core
  identity ASAI
  version 3.0
  creator APERTURESyndicate
```

Результат — ключ `memory` становится строкой: `Core (SYNX):\n```synx\n...\n````. Это удобно для AI-агентов, которым нужны блоки SYNX в контекстном окне.

---

### `:vision` — Намерение генерации изображения

Метаданные-маркер. Движок распознаёт его (без ошибки), но значение проходит без изменений. Приложения определяют `:vision` через метаданные и направляют запрос в API генерации изображений.

```synx
!active

cover:vision Закат над горами
diagram:vision Архитектурная диаграмма системы
```

Движок **НЕ** генерирует изображения — он аннотирует поле для обработки на уровне приложения.

---

### `:audio` — Намерение генерации аудио

Метаданные-маркер. Работает аналогично `:vision`, но для аудио/TTS.

```synx
!active

narration:audio Прочитай это резюме вслух
sfx:audio Драматический оркестровый аккорд
```

Движок **НЕ** генерирует аудио — он аннотирует поле для обработки на уровне приложения.

---

## 🔒 Ограничения (Constraints)

Ограничения проверяют значения при парсинге. Определяются в `[скобках]` после имени ключа.

| Ограничение | Синтаксис | Описание |
|---|---|---|
| `required` | `key[required]` | Обязательное значение |
| `readonly` | `key[readonly]` | Только для чтения |
| `min:N` | `key[min:3]` | Минимальная длина/значение |
| `max:N` | `key[max:100]` | Максимальная длина/значение |
| `type:T` | `key[type:int]` | Принудительный тип |
| `pattern:R` | `key[pattern:^\d+$]` | Регулярное выражение |
| `enum:A\|B` | `key[enum:light\|dark]` | Допустимые значения |

```synx
!active

app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
```

---

## 🔗 Цепочки маркеров

Маркеры можно объединять:

```synx
!active

port:env:default:8080 PORT
profit:calc:round:2 revenue * margin
```

Порядок важен — маркеры выполняются слева направо.

### ✅ Совместимость маркеров

Хорошо работающие комбинации:

- `env:default`
- `calc:round`
- `split:unique`
- `split:join` (через промежуточный массив)

Важные ограничения:

- Нужен `!active`, иначе маркеры не вычисляются.
- Некоторые маркеры зависят от типа: `split` ожидает строку, `join` ожидает массив, `round`/`clamp` ожидают число.
- Аргументы читаются справа от маркера в цепочке (например, `clamp:min:max`, `round:n`, `map:key`).
- Если тип после предыдущего маркера изменился, следующий маркер может не сработать.

---

## 📦 Бинарный формат (`.synxb`)

> Добавлено в v3.6.0.

SYNX поддерживает компактное бинарное представление **`.synxb`**. Оно сохраняет данные и метаданные с **полной обратимостью** (round-trip) и обычно даёт **40%+** экономии по размеру относительно текста.

Быстрые примеры:

```rust
use synx_core::Synx;

let text = "name Alice\nage 30\n";
let bin  = Synx::compile(text, false).unwrap();
assert!(Synx::is_synxb(&bin));
let back = Synx::decompile(&bin).unwrap();
```

```python
import synx_native
data = synx_native.compile("name Alice\nage 30\n", resolved=False)
assert synx_native.is_synxb(data)
text = synx_native.decompile(data)
```

---

## 🤖 LLM Tool Use (`!tool`)

> Добавлено в v3.6.0.

Директива `!tool` описывает **вызов инструмента** для LLM в компактном виде: первая строка после `!tool` — имя инструмента, дальше — параметры.

```synx
!tool
web_search
  query "synx format"
  limit 5
```

Результат (JSON) через `parse_tool`:

```json
{ "tool": "web_search", "params": { "query": "synx format", "limit": 5 } }
```

В .NET:

```csharp
using Synx;
var tool = SynxFormat.ParseTool("!tool\nweb_search\n  query test\n");
var json = SynxFormat.ToJson(tool);
```

**C++** (`bindings/cpp/include/synx/synx.hpp`) — тот же движок через `synx::parse_tool`:

```cpp
#include <synx/synx.hpp>
auto j = synx::parse_tool("!tool\nweb_search\n  query test\n");
```

**Go** (`bindings/go`, cgo):

```go
import synx "github.com/APERTURESyndicate/synx-format/bindings/go"

j, err := synx.ParseTool("!tool\nweb_search\n  query test\n")
```

**Swift** (`bindings/swift`, SwiftPM) — тот же движок через `SynxEngine`:

```swift
import Synx
let j = try SynxEngine.parseTool("!tool\nweb_search\n  query test\n")
```

**Kotlin/JVM** (`bindings/kotlin`, JNA) — тот же движок через `SynxEngine`:

```kotlin
import com.aperturesyndicate.synx.SynxEngine

val j = SynxEngine.parseTool("!tool\nweb_search\n  query test\n")
```

**Mojo** (`bindings/mojo`) — тот же движок через CPython `synx_native`:

```mojo
from synx.interop import parse_tool_json
```

---

## 🧠 C++ SDK (`bindings/cpp`)

> v3.6.0 — паритет с **`synx-core`** (это не второй парсер).

Слой C++ — заголовок **C++17** (`include/synx/synx.hpp`) поверх нативной библиотеки **`synx-c`** из **`bindings/c-header`**. Доступны те же **`!active`**, **`!tool`**, **`.synxb`**, канонический **JSON** и структурный **`diff`**, что и в Rust — потому что внутри это **`synx-core`**.

**1. Сборка нативной библиотеки** (корень репозитория):

```bash
cargo build -p synx-c --release
```

**2. Include:** `bindings/cpp/include` и `bindings/c-header/include`.

**3. Линковка:** `libsynx_c.so` / `libsynx_c.dylib` / `synx_c.dll` (и при необходимости import `.lib` на Windows).

**Пример:**

```cpp
#include <synx/synx.hpp>
#include <iostream>

int main() {
    auto json = synx::parse_active("!active\nport:env:default:8080 PORT\n");
    if (!json) return 1;
    std::cout << *json << '\n';
    return 0;
}
```

**API:** `synx::parse`, `parse_active`, `stringify`, `format`, `parse_tool`, `diff`, `compile`, `decompile`, `is_synxb` — подробности в `bindings/cpp/README.md` и `bindings/cpp/CMakeLists.txt` (`SYNX_C_LIBRARY` для примера `synx_cpp_minimal`).

---

## 🐹 Go (`bindings/go`, cgo)

> v3.6.0 — **полный паритет** с **`synx-core`** через **`synx-c`**, без отдельного парсера на Go.

Нужны `CGO_ENABLED=1`, C toolchain, `cargo build -p synx-c --release`. Импорт:

```go
import synx "github.com/APERTURESyndicate/synx-format/bindings/go"
```

**API:** `Parse`, `ParseActive`, `Stringify`, `Format`, `ParseTool`, `Compile`, `Decompile`, `IsSynxb`, `Diff` — строки JSON / SYNX или `[]byte` для `.synxb`; при ошибке FFI — `synx.ErrSynx`.

**Linux/macOS:** в `synx.go` по умолчанию `-L../../target/release -lsynx_c`.

**Windows (MSVC DLL + MinGW gcc):** задайте `CGO_LDFLAGS` на `synx_c.dll.lib`, `CGO_LDFLAGS_ALLOW=.*`, добавьте `target\release` в `PATH` для `synx_c.dll` — [`bindings/go/README.md`](../../bindings/go/README.md).

---

## 🦜 Swift (`bindings/swift`, SwiftPM)

> v3.6.0 — **полный паритет** с **`synx-core`** через **`synx-c`** (пакет SwiftPM **`Synx`**, C-модуль **`CSynx`** — не отдельный парсер на Swift).

**1. Сборка нативной библиотеки** (корень репозитория): `cargo build -p synx-c --release`.

**2. Сборка / тесты** из `bindings/swift` с флагами линковки на `target/release` (`-L … -lsynx_c`). На **macOS** при необходимости задайте **`DYLD_LIBRARY_PATH`** или положите `libsynx_c.dylib` рядом с бинарником.

**API:** `SynxEngine.parse`, `parseActive`, `stringify(json:)`, `format`, `parseTool`, `diff`, `compile`, `decompile`, `isSynxb` — при ошибке FFI выбрасывается `SynxEngineError`.

Файл `Sources/CSynx/synx.h` дублирует `bindings/c-header/include/synx.h` (синхронизировать при смене C API).

Подробности: [`bindings/swift/README.md`](../../bindings/swift/README.md).

---

## ☕ Kotlin / JVM (`bindings/kotlin`, JNA)

> v3.6.0 — **полный паритет** с **`synx-core`** через **`synx-c`** и [**JNA**](https://github.com/java-native-access/jna) (Gradle-модуль **`synx-kotlin`**, не отдельный парсер на Kotlin).

**1. Сборка нативной библиотеки** (корень репозитория): `cargo build -p synx-c --release`.

**2. Задайте `SYNX_LIB_DIR`** на `target/release` (каталог с `libsynx_c` / `synx_c.dll`), затем из `bindings/kotlin`: `./gradlew test` (или `gradlew.bat test` на Windows). Для Gradle нужен **JDK 17+**; в `settings.gradle.kts` подключён провайдер цепочек **Foojay**.

**API:** `SynxEngine.parse`, `parseActive`, `stringify`, `format`, `parseTool`, `diff`, `compile`, `decompile`, `isSynxb` — при ошибке FFI выбрасывается `SynxEngineError`. До публикации в Maven Central: **`publishToMavenLocal`**.

Подробности: [`bindings/kotlin/README.md`](../../bindings/kotlin/README.md).

---

## 🔮 Mojo (`bindings/mojo` + `synx_native`)

> v3.6.0 — **полный паритет** с **`synx-core`** за счёт вызова **`synx_native`** из Mojo ([**Python from Mojo**](https://docs.modular.com/mojo/manual/python/python-from-mojo)).

В монорепо **Python** — это уже **Rust** (`synx-core`) через **PyO3**, а не второй парсер. Отдельная **чистая** реализация всей грамматики 3.6 + движок `!active` + `.synxb` на Mojo — отдельный крупный проект. Поддерживаемый путь: **один движок**, interop с CPython на границе.

```bash
pip install synx-format
```

API: `synx/interop.mojo` — `parse_json`, `parse_active_json`, `parse_tool_json`, `stringify_json`, `format_synx`, `diff_json`, `compile_hex`, `decompile_hex`, `is_synxb_hex`. Демо: `bindings/mojo/examples/demo.mojo`. Подробности: [`bindings/mojo/README.md`](../../bindings/mojo/README.md).

---

## 🧩 C# / .NET парсер (`Synx.Core`)

> Добавлено в v3.6.0.

Реализация на .NET живёт в `parsers/dotnet/` и предоставляет:

**NuGet:** `dotnet add package APERTURESyndicate.Synx` — [nuget.org/packages/APERTURESyndicate.Synx](https://www.nuget.org/packages/APERTURESyndicate.Synx). Идентификатор `Synx.Core` на nuget.org занят другим пакетом; в репозитории проект по-прежнему `Synx.Core`, пространства имён — `Synx`. Пока пакета нет в ленте — ссылка на проект или локальный `.nupkg`, см. [`parsers/dotnet/README.md`](../../parsers/dotnet/README.md).

- `SynxFormat.Parse(text)` — статический парсинг
- `SynxFormat.ParseActive(text, options)` — `!active` с маркерами/движком
- `SynxFormat.ParseTool(text)` — `!tool` (tool-call reshape)
- `SynxFormat.ToJson(...)` — канонический JSON (ключи отсортированы)

Запуск тестов:

```bash
cd parsers/dotnet
dotnet test -c Release
```

Для replay корпуса (как “фаззинг без мутаций”): `parsers/dotnet/tools/Synx.FuzzReplay`.

---

## 💻 CLI (Rust) — `synx`

> Добавлено в v3.6.0.

Установка из исходников репозитория:

```bash
cargo install --path crates/synx-cli
```

Или (если опубликовано): `cargo install synx-cli`.

Примеры:

```bash
# Parse → JSON
synx parse config.synx

# Validate (exit 1 on errors)
synx validate config.synx --strict

# Convert SYNX → JSON/YAML/TOML/ENV
synx convert config.synx --format json
synx convert config.synx --format yaml > values.yaml
synx convert config.synx --format toml
synx convert config.synx --format env > .env

# Dot-path query
synx query server.host config.synx

# Parse !tool call
synx tool call.synx

# Compile / decompile binary .synxb
synx compile config.synx
synx decompile config.synxb
```

Команда `synx watch` тоже поддерживается (горячая перезагрузка), см. `synx --help`.

### `synx schema` — Извлечение JSON Schema из ограничений

```bash
synx schema config.synx
# Выводит JSON Schema на основе [required, min:N, max:N, type:T, enum:A|B, pattern:R]
```

---

## 📤 Форматы экспорта (JS/TS API)

> Добавлено в v3.1.3.

Конвертация объекта SYNX в JSON, YAML, TOML или .env:

```typescript
import Synx from '@aperturesyndicate/synx-format';

const config = Synx.loadSync('config.synx');

// JSON
const json = Synx.toJSON(config);          // форматированный
const compact = Synx.toJSON(config, false); // компактный

// YAML
const yaml = Synx.toYAML(config);

// TOML
const toml = Synx.toTOML(config);

// .env (формат KEY=VALUE)
const env = Synx.toEnv(config);            // APP_NAME=TotalWario
const prefixed = Synx.toEnv(config, 'APP'); // APP_APP_NAME=TotalWario
```

---

## 📋 Экспорт схемы

> Добавлено в v3.1.3.

Извлечение ограничений SYNX в виде объекта JSON Schema:

```typescript
const schema = Synx.schema(`
!active
app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
`);
```

Результат:

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

## � Структурный Diff

> Добавлено в v3.6.0.

Сравнение двух разобранных SYNX-объектов с получением структурной разницы:

```typescript
const before = Synx.parse('name Alice\nage 30\nrole user');
const after  = Synx.parse('name Bob\nage 30\nstatus active');
const diff   = Synx.diff(before, after);
```

Результат:

```json
{
  "added":     { "status": "active" },
  "removed":   { "role": "user" },
  "changed":   { "name": { "from": "Alice", "to": "Bob" } },
  "unchanged": ["age"]
}
```

Применение:
- **Управление памятью AI** — определение что изменилось между ходами диалога
- **Аудит конфигов** — логирование точных изменений полей
- **Горячая перезагрузка** — применение только изменённых полей

---

## �👁 Наблюдатель файлов

> Добавлено в v3.1.3.

Следите за `.synx` файлом и получайте обновлённый конфиг при каждом изменении:

```typescript
const handle = Synx.watch('config.synx', (config, error) => {
  if (error) {
    console.error('Ошибка обновления конфига:', error.message);
    return;
  }
  console.log('Конфиг обновлён:', config.server.port);
}, { strict: true });

// Остановить наблюдение
handle.close();
```

---

## 🐳 Руководство по деплою

> Добавлено в v3.1.3.

### Docker + Docker Compose

SYNX служит **единым источником правды** для всей конфигурации сервисов. Сервисы, которым нужен собственный формат (Nginx, Redis и т.д.), получают сгенерированные конфиги при запуске.

**Паттерн:**

```
┌─────────────────┐     ┌────────────────┐     ┌─────────────────┐
│   config.synx   │────▶│  скрипт запуска │────▶│  nginx.conf     │
│  (один файл)    │     │  или CLI convert│     │  .env           │
│  :env :default  │     │                 │     │  redis.conf     │
│  :template      │     │                 │     │  настройки      │
└─────────────────┘     └────────────────┘     └─────────────────┘
```

**Шаг 1 — Напишите конфиг:**

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

**Шаг 2 — Генерация .env для Docker Compose:**

```bash
synx convert config.synx --format env > .env
```

**Шаг 3 — Использование в docker-compose.yml:**

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

### Генерация конфига Nginx

Используйте шаблон + скрипт запуска для генерации `nginx.conf` из SYNX:

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

### Подключение Redis

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

### Подключение PostgreSQL

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

K8s монтирует секреты как файлы в `/run/secrets/`. Используйте `:watch` для их чтения:

```synx
!active

db_password:watch /run/secrets/db-password
api_key:watch /run/secrets/api-key
```

Docker Secrets работают аналогично — монтируются в `/run/secrets/`.

### HashiCorp Vault

Используйте Vault Agent для записи секретов в файлы, затем читайте через `:watch`:

```synx
!active

db_creds:watch:password /vault/secrets/database
api_key:watch:key /vault/secrets/api-key
```

Или инжектируйте через переменные окружения с помощью `env_template` Vault Agent:

```synx
!active

db_password:env VAULT_DB_PASSWORD
api_key:env VAULT_API_KEY
```

### Helm / Kubernetes

Конвертация SYNX в YAML для Helm values:

```bash
synx convert config.synx --format yaml > helm/values.yaml
helm upgrade my-release ./chart -f helm/values.yaml
```

### Terraform

Terraform принимает JSON-файлы переменных:

```bash
synx convert config.synx --format json > terraform.tfvars.json
terraform apply -var-file=terraform.tfvars.json
```

### Валидация в CI/CD пайплайне

Добавьте в CI пайплайн для проверки конфигов перед деплоем:

```yaml
# Пример GitHub Actions
- name: Валидация SYNX конфига
  run: npx @aperturesyndicate/synx-format validate config.synx --strict
```

---

## �💻 Примеры кода

### JavaScript / TypeScript

```typescript
import { Synx } from '@aperturesyndicate/synx-format';

const config = Synx.parse(`
  app_name TotalWario
  server
    host 0.0.0.0
    port 8080
`);

console.log(config.app_name);     // "TotalWario"
console.log(config.server.port);  // 8080
```

```typescript
// Загрузка файла
const config = Synx.loadSync('./config.synx');
const config = await Synx.load('./config.synx');
```

**Управление конфигом (set / add / remove):**

```typescript
import { Synx } from '@aperturesyndicate/synx-format';

const config = Synx.loadSync('./game.synx');

// Установить значение
Synx.set(config, 'max_players', 100);
Synx.set(config, 'server.host', 'localhost');

// Получить значение
const port = Synx.get(config, 'server.port'); // 8080

// Добавить в список
Synx.add(config, 'maps', 'Arena of Doom');

// Удалить из списка
Synx.remove(config, 'maps', 'Arena of Doom');

// Удалить ключ целиком
Synx.remove(config, 'deprecated_key');

// Проверить блокировку
if (!Synx.isLocked(config)) {
  Synx.set(config, 'motd', 'Добро пожаловать!');
}
```

> **Примечание:** Если в файле `.synx` есть `!lock`, все вызовы `set`/`add`/`remove` выбросят ошибку.

**Методы доступа (JS/TS API):**

- `Synx.get(obj, keyPath)` — получить значение по dot-path.
- `Synx.set(obj, keyPath, value)` — установить значение по dot-path.
- `Synx.add(obj, keyPath, item)` — добавить элемент в список.
- `Synx.remove(obj, keyPath, item?)` — удалить элемент из списка или ключ целиком.
- `Synx.isLocked(obj)` — проверить, заблокирован ли конфиг через `!lock`.

### Python

Сейчас `synx_native` экспортирует только: `parse`, `parse_active`, `parse_to_json`.

В Python эквиваленты `get`/`set`/`add`/`remove` можно использовать так:

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

# Python access helpers usage
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

let json = Synx::parse_to_json("name Alice\nage 30");
```

---

## 🛠 Поддержка редакторов

### Visual Studio Code

Полная языковая поддержка: подсветка, IntelliSense (21 маркер), диагностика (15 проверок), Go to Definition, форматирование, цветовые превью, инлайн-подсказки для `:calc`, живой предпросмотр.

### Visual Studio 2022

MEF-расширение: подсветка, IntelliSense, тэггер ошибок, сворачивание кода, конвертация.

---

## 🏗 Архитектура

```
synx-format/
├── crates/synx-core/          # Ядро на Rust — парсер + движок
├── bindings/
│   ├── node/                  # NAPI-RS → нативный npm-модуль
│   └── python/                # PyO3 → нативный PyPI-модуль
├── packages/
│   ├── synx-js/               # Чистый TypeScript парсер + движок
│   ├── synx-vscode/           # Расширение VS Code
│   └── synx-visualstudio/     # Расширение Visual Studio 2022
├── publish-npm.bat            # Публикация на npmjs.com
├── publish-pypi.bat           # Публикация на pypi.org
└── publish-crates.bat         # Публикация на crates.io
```

---

## 📖 Спецификация

- **[SPECIFICATION (English)](../spec/SPECIFICATION_EN.md)**
- **[SPECIFICATION (Русский)](../spec/SPECIFICATION_RU.md)**

---

## 🔗 Ссылки

| Ресурс | URL |
|---|---|
| **GitHub** | [github.com/APERTURESyndicate/synx-format](https://github.com/APERTURESyndicate/synx-format) |
| **npm** | [npmjs.com/package/@aperturesyndicate/synx-format](https://www.npmjs.com/package/@aperturesyndicate/synx-format) |
| **PyPI** | [pypi.org/project/synx-format](https://pypi.org/project/synx-format/) |
| **crates.io** | [crates.io/crates/synx-core](https://crates.io/crates/synx-core) |
| **VS Code** | [marketplace.visualstudio.com](https://marketplace.visualstudio.com/items?itemName=APERTURESyndicate.synx-vscode) |
| **Сайт** | [aperturesyndicate.com](https://aperturesyndicate.com) |

---

<p align="center">
  <img src="https://media.aperturesyndicate.com/asother/as/branding/png/asp_128.png" width="96" height="96" />
</p>

<p align="center">
  MIT — © <a href="https://github.com/APERTURESyndicate">APERTURESyndicate</a>
</p>
