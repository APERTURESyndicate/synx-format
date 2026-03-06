<p align="center">
  <a href="https://aperturesyndicate.com/branding/aperturesyndicate.png" target="_blank">
    <img src="https://aperturesyndicate.com/branding/aperturesyndicate.png" alt="APERTURESyndicate" width="400" />
  </a>
</p>

<h1 align="center">SYNX v3.0 — Полное руководство</h1>

<p align="center">
  <strong>Лучше чем JSON. Дешевле чем YAML. Создан для AI и людей.</strong>
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

## Содержание

- [Философия](#-философия)
- [Смотри в действии](#-смотри-в-действии)
- [Как это работает](#-как-это-работает)
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
- [Маркеры — полный справочник](#-маркеры--полный-справочник)
  - [:env](#env--переменная-окружения)
  - [:default](#default--значение-по-умолчанию)
  - [:calc](#calc--арифметическое-выражение)
  - [:random](#random--случайный-выбор)
  - [:alias](#alias--ссылка-на-другой-ключ)
  - [:secret](#secret--скрытое-значение)
  - [:template](#template--интерполяция-строк)
  - [:include](#include--импорт-внешнего-файла)
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
- [Ограничения (Constraints)](#-ограничения-constraints)
- [Цепочки маркеров](#-цепочки-маркеров)
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

### Расширение VS Code

Ищи **"SYNX"** в панели расширений, или:

```bash
code --install-extension APERTURESyndicate.synx-vscode
```

### Visual Studio 2022

Скачай `.vsix` из [GitHub Releases](https://github.com/kaiserrberg/synx-format/releases) и дважды кликни для установки.

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

Два стиля — оба игнорируются парсером:

```synx
# Это хеш-комментарий
// Это слэш-комментарий

name John  # инлайн-комментарий
```

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
import { Synx } from '@aperturesyndicate/synx';
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

SYNX v3.0 содержит **20 маркеров**. Каждый маркер — это функция, присоединяемая к ключу через синтаксис `:маркер`.

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

Копирует значение другого ключа без дублирования.

```synx
!active

admin_email alex@example.com
complaints_email:alias admin_email
```

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

### `:template` — Интерполяция строк

Подставляет `{имя}` значениями других ключей. Поддерживает точечные пути.

```synx
!active

first_name John
last_name Doe
greeting:template Привет, {first_name} {last_name}!
```

---

### `:include` — Импорт внешнего файла

Импортирует содержимое другого `.synx` файла. Путь относительный.

```synx
!active

database:include ./db.synx
```

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

## 💻 Примеры кода

### JavaScript / TypeScript

```typescript
import { Synx } from '@aperturesyndicate/synx';

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
import { Synx } from '@aperturesyndicate/synx';

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

Полная языковая поддержка: подсветка, IntelliSense (20 маркеров), диагностика (15 проверок), Go to Definition, форматирование, цветовые превью, инлайн-подсказки для `:calc`, живой предпросмотр.

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

- **[SPECIFICATION (English)](https://github.com/kaiserrberg/synx-format/blob/main/SPECIFICATION_EN.md)**
- **[SPECIFICATION (Русский)](https://github.com/kaiserrberg/synx-format/blob/main/SPECIFICATION_RU.md)**

---

## 🔗 Ссылки

| Ресурс | URL |
|---|---|
| **GitHub** | [github.com/kaiserrberg/synx-format](https://github.com/kaiserrberg/synx-format) |
| **npm** | [npmjs.com/package/@aperturesyndicate/synx](https://www.npmjs.com/package/@aperturesyndicate/synx) |
| **PyPI** | [pypi.org/project/synx-format](https://pypi.org/project/synx-format/) |
| **crates.io** | [crates.io/crates/synx-core](https://crates.io/crates/synx-core) |
| **VS Code** | [marketplace.visualstudio.com](https://marketplace.visualstudio.com/items?itemName=APERTURESyndicate.synx-vscode) |
| **Сайт** | [aperturesyndicate.com](https://aperturesyndicate.com) |

---

<p align="center">
  <img src="https://aperturesyndicate.com/branding/logos/asp_128.png" width="96" height="96" />
</p>

<p align="center">
  MIT — © <a href="https://github.com/kaiserrberg">APERTURESyndicate</a>
</p>
