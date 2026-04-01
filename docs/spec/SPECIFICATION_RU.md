# 📘 SYNX — Полная спецификация формата

**Версия:** 4.2  
**Название:** SYNX (Syndicate Exchange), читается как «Синус Х»  
**Расширение файлов:** `.synx`

---

## 1. Философия

SYNX — это формат данных, который убирает весь синтаксический мусор: кавычки, запятые, скобки, двоеточия вокруг значений. Остаётся только **ключ**, **пробел**, **значение**.

| Критерий | JSON | YAML | SYNX |
|---|---|---|---|
| Кавычки | Обязательны | Иногда | Нет |
| Запятые / скобки | Да | Нет | Нет |
| Встроенная логика | Нет | Нет | Да (`!active`) |
| Токены для AI | ~100% | ~75% | ~40% |

---

## 2. Базовый синтаксис (работает всегда)

### 2.1 Ключ-значение

Всё, что идёт после **первого пробела** — это значение. Кавычки не нужны.

```synx
имя Иван
возраст 25
фраза Я люблю программировать и пить кофе!
```

Эквивалент в JSON:
```json
{
  "имя": "Иван",
  "возраст": 25,
  "фраза": "Я люблю программировать и пить кофе!"
}
```

**Правила для ключей:**
- Без пробелов (используйте `_` или `camelCase`)
- Не могут начинаться с `-`, `#`, `//`, `!`
- Могут содержать буквы любого алфавита, цифры, `_`, `-` (внутри)

---

### 2.2 Автоопределение типов

Парсер автоматически определяет типы:

```synx
строка Hello World
целое 42
дробное 3.14
правда true
ложь false
пусто null
```

Результат:
```json
{
  "строка": "Hello World",
  "целое": 42,
  "дробное": 3.14,
  "правда": true,
  "ложь": false,
  "пусто": null
}
```

**Принудительное приведение типа** (если нужно число как строку):
```synx
zip_code(string) 90210
id(int) 007
```

Поддерживаемые касты: `(int)`, `(float)`, `(bool)`, `(string)`

---

### 2.3 Вложенность (объекты)

Отступ **2 пробела** создаёт вложенный объект. Ключ без значения — это группа (папка).

```synx
сервер
  хост 0.0.0.0
  порт 8080
  ssl
    включён true
    сертификат /etc/ssl/cert.pem
```

Результат:
```json
{
  "сервер": {
    "хост": "0.0.0.0",
    "порт": 8080,
    "ssl": {
      "включён": true,
      "сертификат": "/etc/ssl/cert.pem"
    }
  }
}
```

> ⚠️ Используйте **пробелы**, а не TAB. Стандарт — **2 пробела** на уровень.
---

### 2.4 Списки

Символ `- ` (дефис + пробел) создаёт элемент списка. Элементы без имён — просто порядковый набор.

```synx
инвентарь
  - Меч
  - Щит
  - Зелье здоровья
```

Результат:
```json
{
  "инвентарь": ["Меч", "Щит", "Зелье здоровья"]
}
```

**Список объектов** (каждый элемент — мини-анкета):

```synx
гараж
  - марка BMW
    цвет чёрный
    год 2023
  - марка Audi
    цвет белый
    год 2021
```

Результат:
```json
{
  "гараж": [
    { "марка": "BMW", "цвет": "чёрный", "год": 2023 },
    { "марка": "Audi", "цвет": "белый", "год": 2021 }
  ]
}
```

---

### 2.5 Многострочный текст (блоки)

Символ `|` после ключа начинает блок текста. Всё, что идёт ниже с отступом — часть этого текста.

```synx
описание |
  Это длинное описание,
  которое занимает несколько строк.
  Каждая строка склеивается через перенос.
```

Результат:
```json
{
  "описание": "Это длинное описание,\nкоторое занимает несколько строк.\nКаждая строка склеивается через перенос."
}
```

Принудительный перенос строки внутри одной строки: `/n`

```synx
баннер Добро пожаловать!/nУдачной игры!
```

Результат: 
```
Добро пожаловать!
Удачной игры!
```

---

### 2.6 Комментарии

Поддерживаются **два стиля** комментариев:

```synx
# Это комментарий (стиль Python/YAML)
// Это тоже комментарий (стиль JS/C++)

имя Иван  # Инлайн-комментарий после значения
порт 8080 // Тоже инлайн
```

Комментарии полностью игнорируются парсером.

---

## 3. Режим `!active` — живой конфиг

### 3.1 Что это такое

По умолчанию `.synx` файл — это **статичные данные**. Просто ключи и значения, как JSON.

Но если в **первой строке** файла написать `!active`, файл превращается в **живой конфиг** — включаются:

- **Функции** (`:random`, `:calc`, `:env` и др.)
- **Ограничения** (`[min:3]`, `[type:int]` и др.)

```synx
!active

порт:env PORT
приветствие:random
  - Привет!
  - Здорово!
```

Без `!active` — маркеры `:random`, `:calc` и квадратные скобки `[]` **полностью игнорируются**. Парсер читает файл как обычный текст:

```synx
// Нет !active — функции НЕ работают
порт:env PORT           // ключ = "порт:env", значение = "PORT" (просто строка)
приветствие:random       // ключ = "приветствие:random", значение = {} (объект)
```

> 💡 Это сделано для безопасности: статичный файл **никогда** не выполнит код, не полезет в переменные окружения и не запустит команды.

---

### 3.2 Полный список функций (`:`)

Функции записываются через двоеточие сразу после имени ключа: `ключ:функция значение`.

---

#### `:random` — случайный выбор

Выбирает **один** элемент из списка при каждом парсинге.

**Равные шансы** (без процентов):
```synx
!active

боевой_клич:random
  - Время побеждать!
  - Ва-ха-ха!
  - За Синдикат!
```
Каждый элемент имеет шанс 33.3%.

**Взвешенный рандом** (с процентами):
```synx
!active

// Проценты указываются после :random через пробел
// Порядок процентов соответствует порядку элементов
награда:random 70 20 10
  - Обычный сундук
  - Редкий сундук
  - Легендарный сундук
```

Здесь:
- «Обычный сундук» выпадает с шансом **70%**
- «Редкий сундук» — **20%**
- «Легендарный сундук» — **10%**

**Правила процентов:**
| Ситуация | Поведение |
| Процентов нет | Все элементы равновероятны |
| Сумма = 100 | Используются как есть |
| Сумма ≠ 100 | Автоматически нормализуются (пропорции сохраняются) |
| Процентов меньше, чем элементов | Остаток делится поровну между элементами без процента |
| Процентов больше, чем элементов | Лишние проценты игнорируются |

Пример нормализации:
```synx
!active

// 2 + 1 = 3, нормализация: ~66.7% и ~33.3%
тип:random 2 1
  - Воин
  - Маг
```

Пример неполных процентов:
```synx
!active

// Первому — 80%, оставшиеся 20% делятся на два: по 10%
дроп:random 80
  - Ничего
  - Меч
  - Щит
```

---

#### `:calc` — вычисление выражения

Вычисляет арифметическое выражение. Может ссылаться на другие ключи по имени.

```synx
!active

цена 100
налог:calc цена * 0.2
итого:calc цена + налог
```

Результат:
```
цена 100
налог 20 // < программа получит это значение
итого 120 // < программа получит это значение
```

Если брать файл .json
```json
{
  "цена": 100,
  "налог": 20,
  "итого": 120
}
```

**Поддерживаемые операции:** `+`, `-`, `*`, `/`, `%` (остаток), `(`, `)`.

> ⚠️ Только арифметика. Никакого произвольного кода. Парсер использует безопасный вычислитель, не `eval()`.

---

#### `:env` — переменная окружения

Подставляет значение переменной окружения системы.

```synx
!active

порт:env PORT
домашняя_папка:env HOME
```

Если переменная не найдена — значение будет `null`.

**С дефолтом** (через `:default`):
```synx
!active

порт:env:default:8080 PORT
```
Если `PORT` не задан в системе → вернётся `8080`.

---

#### `:alias` — ссылка на другой ключ

Копирует значение другого ключа. Не дублирует данные — ссылается.

```synx
!active

главный_админ alex@mail.com
почта_для_жалоб:alias главный_админ
```

Результат:
```json
{
  "главный_админ": "alex@mail.com",
  "почта_для_жалоб": "alex@mail.com"
}
```

---

#### `:secret` — скрытое значение

Значение читается программой, но **не выводится** в логи, при `freeze`, при сериализации в JSON. Защита от случайной утечки.

```synx
!active

api_key:secret sk-1234567890abcdef
db_password:secret P@ssw0rd!
```

При `console.log(data)`, `print(data)`, `JSON.stringify(data)` — покажет `"[SECRET]"`.  
Чтобы **получить реальное значение**, используйте метод `.reveal()`:

```javascript
// JavaScript / TypeScript
const key = data.api_key;          // SynxSecret объект
console.log(String(key));           // "[SECRET]"
console.log(JSON.stringify(key));   // '"[SECRET]"'
console.log(key.reveal());          // "sk-1234567890abcdef"  ← реальное значение
```

```python
# Python
key = data['api_key']               # SynxSecret объект
print(key)                          # [SECRET]
print(key.reveal())                 # sk-1234567890abcdef  ← реальное значение
```

> ⚠️ **Важно:** Никогда не логируйте результат `.reveal()`. Этот метод предназначен только для передачи значения в API, подключения к БД и т.д.

---

#### `:default` — значение по умолчанию

Задаёт fallback, если основное значение пустое или не найдено. Чаще всего комбинируется с `:env`.

```synx
!active

// Если переменная PORT не задана — будет 3000
порт:env:default:3000 PORT

// Просто значение по умолчанию (если значение = null или пусто)
тема:default dark
```

---

#### `:unique` — убрать дубликаты из списка

Оставляет только уникальные элементы.

```synx
!active

теги:unique
  - экшн
  - рпг
  - экшн
  - симулятор
  - рпг
```

Результат: `["экшн", "рпг", "симулятор"]`

---

#### `:include` — подключить другой файл

Вставляет содержимое другого `.synx` файла в текущий. Путь относительный от текущего файла.

```synx
!active

// Подтянуть настройки базы данных из отдельного файла
database:include ./db.synx
```

Где `db.synx`:
```synx
host localhost
port 5432
name mydb
```

Результат:
```json
{
  "database": {
    "host": "localhost",
    "port": 5432,
    "name": "mydb"
  }
}
```

---

#### `:geo` — значение по геолокации / региону

Выбирает значение на основе региона пользователя (определяется по IP или системной локали).

```synx
!active

валюта:geo
  - RU RUB
  - US USD
  - EU EUR
```

> Эта функция требует поддержки в рантайме. Парсер передаёт текущий регион движку.

---

#### `:template` — интерполяция строк

Подставляет значения из объекта в строку с использованием `{placeholders}`. Поддерживает точечную нотацию для доступа к вложенным полям.

```synx
!active

приветствие:template Привет, {имя}!
  имя Иван

сообщение:template Сервер {server.host}:{server.port}
  server
    host localhost
    port 8080
```

Результат:
```json
{
  "приветствие": "Привет, Иван!",
  "сообщение": "Сервер localhost:8080"
}
```

---

#### `:split` — разделение строки на массив

Преобразует строку в массив, разделяя по указанному разделителю. Поддерживает ключевые слова: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`.

```synx
!active

теги:split space
  Python Django Flask

координаты:split pipe
  10|20|30

части:split dash
  section-one-two-three
```

Результат:
```json
{
  "теги": ["Python", "Django", "Flask"],
  "координаты": ["10", "20", "30"],
  "части": ["section", "one", "two", "three"]
}
```

> Если значение выглядит как число, оно автоматически приводится к числовому типу.

---

#### `:join` — объединение массива в строку

Преобразует массив в строку, соединяя элементы указанным разделителем. Поддерживает те же ключевые слова: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`.

```synx
!active

фраза:join space
  - Hello
  - World
  - 2024

путь:join dot
  - app
  - modules
  - config
```

Результат:
```json
{
  "фраза": "Hello World 2024",
  "путь": "app.modules.config"
}
```

---

### 3.3 Сводная таблица функций

| Функция | Описание | Пример |
|---|---|---|
| `:random` | Случайный элемент из списка | `фраза:random` |
| `:random N N N` | Взвешенный рандом (проценты) | `дроп:random 70 20 10` |
| `:calc` | Арифметическое вычисление | `итого:calc цена * 1.2` |
| `:env` | Переменная окружения | `порт:env PORT` |
| `:alias` | Ссылка на другой ключ | `копия:alias оригинал` |
| `:secret` | Скрытое от логов значение | `пароль:secret abc123` |
| `:default` | Значение по умолчанию | `тема:default dark` |
| `:default:X` | Fallback (в комбинации) | `порт:env:default:8080 PORT` |
| `:unique` | Дедупликация списка | `теги:unique` |
| `:include` | Подключить внешний файл | `бд:include ./db.synx` |
| `:geo` | Значение по региону | `валюта:geo` |
| `:template` | Интерполяция строк | `приветствие:template Привет, {имя}!` |
| `:split` | Разделение строки на массив | `теги:split space` |
| `:join` | Объединение массива в строку | `фраза:join space` |

**Комбинирование функций** — через цепочку `:`:
```synx
!active
порт:env:default:8080 PORT
```

---

### 3.4 Ограничения (`[]`) — валидация данных

Ограничения записываются в квадратных скобках между ключом и функцией (или значением). Они работают **только** в режиме `!active`.

Общий синтаксис:
```
ключ[ограничение1:значение, ограничение2:значение]:функция значение
```

---

#### `min` / `max` — минимум и максимум

Для **чисел** — ограничивает диапазон значения.  
Для **строк** — ограничивает длину (количество символов).

```synx
!active

// Строка от 3 до 30 символов
имя_приложения[min:3, max:30] TotalWario

// Число от 1 до 100
громкость[min:1, max:100] 75

// Только минимум
пароль[min:8] мойпароль123
```

---

#### `required` — обязательное поле

Парсер выкинет ошибку, если значение пустое или отсутствует.

```synx
!active

api_key[required]:env API_KEY
имя[required, min:1] Wario
```

---

#### `pattern` — регулярное выражение

Значение должно соответствовать regex-шаблону.

```synx
!active

код_страны[pattern:^[A-Z]{2}$] RU
телефон[pattern:^\+\d{10,15}$] +79991234567
```

---

#### `enum` — допустимые значения

Значение должно быть одним из перечисленных.

```synx
!active

тема[enum:light|dark|auto] dark
регион[enum:EU|US|RU|AS] RU
```

---

#### `readonly` — только для чтения

Значение нельзя изменить через API / горячую перезагрузку конфига. Только ручное редактирование файла.

```synx
!active

версия[readonly] 2.0.0
```

---

### 3.5 Сводная таблица ограничений

| Ограничение | Описание | Синтаксис |
|---|---|---|
| `min` | Минимум (длина или значение) | `[min:3]` |
| `max` | Максимум (длина или значение) | `[max:100]` |
| `type` | Тип данных | `[type:int]` |
| `required` | Обязательное поле | `[required]` |
| `pattern` | Regex-валидация | `[pattern:^\d+$]` |
| `enum` | Список допустимых значений | `[enum:a\|b\|c]` |
| `readonly` | Запрет на изменение | `[readonly]` |

Ограничения можно комбинировать через запятую:
```synx
!active
пароль[required, min:8, max:64, type:string] MyP@ssw0rd
```

---

## 4. Полные примеры

### 4.1 Обычный файл (без `!active`)

Простой статичный конфиг. Никакой магии — просто данные.

```synx
# Конфиг игры TotalWario (статичный)

app_name TotalWario
version 2.0.0

server
  host 0.0.0.0
  port 8080
  ssl_enabled false

gameplay
  base_hp 100
  boss_hp 500
  max_players 16
  greeting Prepare to fight.

map_rotation
  - Arena of Doom
  - Crystal Caverns
  - Wario Stadium

rules |
  1. Не читерить.
  2. Уважать Синдикат.
  3. Весело проводить время!

credits
  lead_dev KaiserBerg
  studio APERTURESyndicate
  year 2026
```

Результат (`Synx.parse()`):
```json
{
  "app_name": "TotalWario",
  "version": "2.0.0",
  "server": {
    "host": "0.0.0.0",
    "port": 8080,
    "ssl_enabled": false
  },
  "gameplay": {
    "base_hp": 100,
    "boss_hp": 500,
    "max_players": 16,
    "greeting": "Prepare to fight."
  },
  "map_rotation": [
    "Arena of Doom",
    "Crystal Caverns",
    "Wario Stadium"
  ],
  "rules": "1. Не читерить.\n2. Уважать Синдикат.\n3. Весело проводить время!",
  "credits": {
    "lead_dev": "KaiserBerg",
    "studio": "APERTURESyndicate",
    "year": 2026
  }
}
```

---

### 4.2 Файл с `!active` — живой конфиг

Тот же конфиг, но с динамическими функциями и валидацией.

```synx
!active
# Живой конфиг проекта TotalWario

app_name[required, min:3, max:30] TotalWario
version[readonly] 2.0.0

server
  // Порт из переменной окружения, если нет — 8080
  port:env:default:8080 PORT
  host 0.0.0.0
  ssl_enabled false

gameplay
  base_hp 100
  // Движок сам посчитает: 100 * 5 = 500
  boss_hp:calc base_hp * 5
  max_players[type:int, min:2, max:64] 16
  difficulty[enum:easy|normal|hard|nightmare] normal

  // Каждый раз при парсинге — случайная фраза
  greeting:random
    - Welcome to the arena!
    - Prepare to fight.
    - Wario time!

  // Взвешенный рандом: обычный дроп 70%, редкий 20%, легенда 10%
  loot_tier:random 70 20 10
    - common
    - rare
    - legendary

map_rotation
  - Arena of Doom
  - Crystal Caverns
  - Wario Stadium

rules |
  1. Не читерить.
  2. Уважать Синдикат.
  3. Весело проводить время!

// Подключить настройки БД из отдельного файла
database:include ./db.synx

// Секреты — не попадут в логи
api_key[required]:secret sk-live-abc123def456

credits
  lead_dev KaiserBerg
  studio APERTURESyndicate
  year 2026
  contact:alias lead_dev
```

Результат одного из вызовов `Synx.parse()`:
```json
{
  "app_name": "TotalWario",
  "version": "2.0.0",
  "server": {
    "port": 8080,
    "host": "0.0.0.0",
    "ssl_enabled": false
  },
  "gameplay": {
    "base_hp": 100,
    "boss_hp": 500,
    "max_players": 16,
    "difficulty": "normal",
    "greeting": "Wario time!",
    "loot_tier": "common"
  },
  "map_rotation": [
    "Arena of Doom",
    "Crystal Caverns",
    "Wario Stadium"
  ],
  "rules": "1. Не читерить.\n2. Уважать Синдикат.\n3. Весело проводить время!",
  "database": {
    "host": "localhost",
    "port": 5432,
    "name": "mydb"
  },
  "api_key": "[SECRET]",
  "credits": {
    "lead_dev": "KaiserBerg",
    "studio": "APERTURESyndicate",
    "year": 2026,
    "contact": "KaiserBerg"
  }
}
```

> При следующем вызове `parse()` поля `greeting` и `loot_tier` будут другими — в этом и смысл `:random`.

---

## 5. Использование в коде — напрямую, без конвертации

> 🚀 **Главный принцип:** SYNX читается вашим кодом **напрямую**. Никакой конвертации в JSON не нужно. Библиотека сама парсит `.synx` файл и возвращает готовый родной объект вашего языка — `dict` в Python, `Object` в JavaScript. Вы работаете с данными сразу.

### 5.1 Установка

```bash
# JavaScript / TypeScript
npm install @aperturesyndicate/synx-format

# Python
pip install synx-format
```

---

### 5.2 Python — чтение `.synx` напрямую

```python
from synx import Synx

# Читаем .synx файл — и сразу получаем dict
data = Synx.load('config.synx')

# Всё. Данные готовы. Работайте как с обычным словарём:
print(data['app_name'])                # "TotalWario"
print(data['server']['port'])          # 8080
print(data['gameplay']['base_hp'])     # 100
print(data['gameplay']['boss_hp'])     # 500 (посчитано через :calc)
print(data['gameplay']['greeting'])    # "Wario time!" (выбрано :random)

# Списки — обычные list:
for map_name in data['map_rotation']:
    print(f'Загружаю карту: {map_name}')

# Вложенность — обычные dict:
if data['server']['ssl_enabled']:
    print('SSL включён')
```

**Метод `Synx.load(path)`** — читает файл и парсит за один вызов.  
**Метод `Synx.parse(text)`** — парсит строку (если файл уже прочитан).

```python
# Альтернативно — из строки:
with open('config.synx', 'r', encoding='utf-8') as f:
    data = Synx.parse(f.read())
```

---

### 5.3 JavaScript — чтение `.synx` напрямую

```javascript
const Synx = require('@aperturesyndicate/synx-format');

// Читаем .synx файл — получаем обычный JS-объект
const data = Synx.loadSync('config.synx');

// Работайте как с обычным объектом через точку:
console.log(data.app_name);               // "TotalWario"
console.log(data.server.port);             // 8080
console.log(data.gameplay.base_hp);        // 100
console.log(data.gameplay.boss_hp);        // 500
console.log(data.gameplay.greeting);       // случайная фраза

// Списки — обычные Array:
data.map_rotation.forEach(map => {
    console.log(`Загружаю карту: ${map}`);
});

// Деструктуризация:
const { base_hp, boss_hp, greeting } = data.gameplay;
console.log(`HP босса: ${boss_hp}, приветствие: ${greeting}`);
```

**`Synx.loadSync(path)`** — читает файл и парсит синхронно.  
**`Synx.load(path)`** — асинхронная версия (возвращает `Promise`).  
**`Synx.parse(text)`** — парсит строку.

```javascript
// Асинхронный вариант:
const data = await Synx.load('config.synx');

// Из строки:
const fs = require('fs');
const text = fs.readFileSync('config.synx', 'utf-8');
const data = Synx.parse(text);
```

---

### 5.4 TypeScript — с типизацией

```typescript
import Synx from '@aperturesyndicate/synx-format';

// Опишите структуру вашего конфига:
interface GameConfig {
  app_name: string;
  version: string;
  server: {
    port: number;
    host: string;
    ssl_enabled: boolean;
  };
  gameplay: {
    base_hp: number;
    boss_hp: number;
    greeting: string;
    loot_tier: string;
  };
  map_rotation: string[];
}

// Парсер вернёт типизированный объект:
const data = Synx.loadSync<GameConfig>('config.synx');

console.log(data.gameplay.boss_hp);  // number, автодополнение работает
console.log(data.server.port);       // number
```

---

### 5.5 Использование данных SYNX для CSS

SYNX возвращает обычный объект — `Object` в JS, `dict` в Python. Вы можете использовать эти данные для генерации CSS, CSS-переменных, или передавать в любую систему стилей.

**Пример: CSS-переменные из SYNX в Node.js**

```synx
# theme.synx
primary #5a6eff
secondary #ff5a8a
font_size 16
border_radius 8
spacing 12
```

```javascript
const Synx = require('@aperturesyndicate/synx-format');
const fs = require('fs');

const theme = Synx.loadSync('theme.synx');

// Генерация CSS-переменных
const css = `:root {
  --color-primary: ${theme.primary};
  --color-secondary: ${theme.secondary};
  --font-size: ${theme.font_size}px;
  --border-radius: ${theme.border_radius}px;
  --spacing: ${theme.spacing}px;
}`;

fs.writeFileSync('theme.css', css);
```

**Пример: Inline-стили в React**

```tsx
import Synx from '@aperturesyndicate/synx-format';

const theme = Synx.loadSync('theme.synx');

function Button({ children }) {
  return (
    <button style={{
      backgroundColor: theme.primary,
      borderRadius: `${theme.border_radius}px`,
      padding: `${theme.spacing}px`,
      fontSize: `${theme.font_size}px`,
    }}>
      {children}
    </button>
  );
}
```

**Пример: CSS-in-JS (styled-components, Tailwind config)**

```javascript
// tailwind.config.js
const Synx = require('@aperturesyndicate/synx-format');
const theme = Synx.loadSync('theme.synx');

module.exports = {
  theme: {
    extend: {
      colors: {
        primary: theme.primary,
        secondary: theme.secondary,
      },
      spacing: {
        base: `${theme.spacing}px`,
      },
    },
  },
};
```

> SYNX не заменяет CSS — он предоставляет **данные** (цвета, размеры, токены), которые ваш код
использует для генерации стилей. Это особенно полезно для дизайн-систем и тематизации.

---

### 5.6 Rust — типобезопасный парсинг

```bash
# Добавить зависимость
cargo add synx
```

```rust
use synx::Synx;

fn main() {
    // Читаем .synx файл — парсер возвращает Value
    let data = Synx::load("config.synx").expect("Ошибка при чтении файла");
    
    // Доступ к полям через индексацию:
    println!("Имя приложения: {}", &data["app_name"]);
    println!("Порт сервера: {}", &data["server"]["port"]);
    println!("HP босса: {}", &data["gameplay"]["boss_hp"]);
    
    // Методы для типобезопасного доступа:
    if let Some(app_name) = data["app_name"].as_str() {
        println!("Приложение: {}", app_name);
    }
    
    if let Some(port) = data["server"]["port"].as_int() {
        println!("Порт: {}", port);
    }
    
    // Работа с массивами:
    if let Some(maps) = data["map_rotation"].as_array() {
        for map in maps {
            if let Some(name) = map.as_str() {
                println!("Карта: {}", name);
            }
        }
    }
    
    // Проверка типов:
    match &data["gameplay"]["ssl_enabled"] {
        synx::Value::Bool(b) => println!("SSL: {}", b),
        synx::Value::Null => println!("SSL не задан"),
        _ => println!("Неверный тип"),
    }
}
```

**Методы Value:**
- `as_str() -> Option<&str>` — строка
- `as_int() -> Option<i64>` — целое число
- `as_float() -> Option<f64>` — число с плавающей точкой
- `as_bool() -> Option<bool>` — булев
- `as_array() -> Option<&[Value]>` — массив
- `as_object() -> Option<&Map<String, Value>>` — объект (map)
- `is_null() -> bool` — null-проверка

> 📌 **Важно:** Rust-парсер работает в режиме `static` — все вычисления (`:random`, `:calc`, `:env`) должны быть выполнены до загрузки и включены в значения как заранее вычисленные данные. Функции не вычисляются на лету.

---

### 5.7 C — FFI к `synx-core` (строки JSON)

Поддерживаемая C-интеграция — **`bindings/c-header`**: заголовок `include/synx.h` и **разделяемая или статическая библиотека**, которую собирает Rust-крейт **`synx-c`** (`cdylib` / `staticlib`). Функции возвращают **UTF-8 JSON** (или байты `.synxb`); дерева `SynxValue` в C **нет** — для дерева используйте ваш JSON-парсер.

**Сборка библиотеки** (из корня репозитория):

```bash
cargo build -p synx-c --release
# Linux: target/release/libsynx_c.so
# macOS: target/release/libsynx_c.dylib
# Windows: target/release/synx_c.dll (+ import .lib для MSVC)
```

**Подключение:**

```c
#include "synx.h"
```

**Память:** каждый `char*` нужно один раз освободить через `synx_free()`. Буфер из `synx_compile` — через `synx_free_bytes(ptr, len)`.

**Функции (v3.6.0):** `synx_parse`, `synx_parse_active`, `synx_stringify`, `synx_format`, `synx_parse_tool`, `synx_compile`, `synx_decompile`, `synx_is_synxb`, `synx_diff` — см. `bindings/c-header/include/synx.h`.

**Минимальный пример:**

```c
#include "synx.h"
#include <stdio.h>

int main(void) {
    char *json = synx_parse("name John\nage 25\n");
    if (!json) {
        fputs("parse failed\n", stderr);
        return 1;
    }
    puts(json);
    synx_free(json);
    return 0;
}
```

> **Паритет:** тот же движок, что у **`synx-core` 3.6.x** (`!active`, `!tool`, `.synxb`, канонический JSON).

---

### 5.8 C++ — тонкая обёртка (`bindings/cpp`)

Официальная C++-обвязка — **`bindings/cpp/include/synx/synx.hpp`** (пространство имён `synx`): **тонкая обёртка C++17** над **§5.7**, с `std::optional` и `std::vector<unsigned char>` для `.synxb`. Нужна та же библиотека **`synx-c`** и оба include-пути (`bindings/cpp/include`, `bindings/c-header/include`).

**Пример:**

```cpp
#include <synx/synx.hpp>
#include <iostream>

int main() {
    auto json = synx::parse("name Wario\nage 30\n");
    if (!json) return 1;
    std::cout << *json << '\n';
    return 0;
}
```

**Обёртки:** `parse`, `parse_active`, `stringify`, `format`, `parse_tool`, `diff`, `compile`, `decompile`, `is_synxb` — семантика как у `synx.h`.

Сборка: `bindings/cpp/README.md`, `bindings/cpp/CMakeLists.txt` (опциональный пример).

> **Паритет:** поведение совпадает с **Rust `synx-core`** для всех операций, экспортируемых через FFI.

---

### 5.9 C# — библиотека для .NET 8 (`parsers/dotnet`)

Поддерживаемая реализация — **`parsers/dotnet/src/Synx.Core`**, целевой фреймворк **.NET 8.0**. NuGet: **`APERTURESyndicate.Synx`** (идентификатор **`Synx.Core`** на nuget.org занят другим пакетом).

**Установка (NuGet):**

```bash
dotnet add package APERTURESyndicate.Synx
```

Карточка пакета: [nuget.org/packages/APERTURESyndicate.Synx](https://www.nuget.org/packages/APERTURESyndicate.Synx). Пока пакета нет в ленте — ссылка на проект или локальный `.nupkg`, см. [`parsers/dotnet/README.md`](../../parsers/dotnet/README.md).

**Пример:**

```csharp
using Synx;

var map = SynxFormat.Parse("server\n  host 0.0.0.0\n  port 8080\n");
if (map["server"] is SynxValue.Obj server
    && server.Map["port"] is SynxValue.Int port)
    Console.WriteLine(port.Value);

var json = SynxFormat.ToJson(map);

// Разрешение !active (маркеры, ограничения, include)
var resolved = SynxFormat.ParseActive("!active\nport:env:default:8080 PORT\n");
```

**Точки входа (`SynxFormat`):** `Parse`, `ParseActive`, `ParseFull`, `ParseFullActive`, `ParseTool`, `ToJson`.

**Значения:** дискриминированные записи `SynxValue` — `Null`, `Bool`, `Int`, `Float`, `Str`, `Secret`, `Arr`, `Obj`.

> **Паритет:** статический разбор и канонический JSON совпадают с **`synx-core`** на покрытых conformance-кейсах; **`ParseActive`** запускает управляемый движок `!active`. **`.synxb`** в C# пока нет (используйте **`synx-core`** или привязки к **`synx-c`**).

---

### 5.10 Go — cgo к `synx-c` (API через JSON)

Поддерживаемая интеграция — **`bindings/go`**: **cgo** оборачивает тот же **`synx.h`** / **`synx-c`**, что и C/C++. Функции возвращают **UTF-8 строки** (JSON или текст SYNX) или **срезы байт** (`.synxb`); дерево разбора — через **`encoding/json`**.

**Требования:** Go 1.21+, `CGO_ENABLED=1`, C-компилятор, собранный **`synx-c`** (`cargo build -p synx-c --release`). Linux/macOS: линковка по умолчанию `-L../../target/release -lsynx_c` относительно `bindings/go`. **Windows:** `CGO_LDFLAGS` на `synx_c.dll.lib`, `synx_c.dll` в `PATH` — см. [`bindings/go/README.md`](../../bindings/go/README.md).

**Пример:**

```go
package main

import (
    "fmt"
    synx "github.com/APERTURESyndicate/synx-format/bindings/go"
)

func main() {
    j, err := synx.Parse("name Wario\nage 30\n")
    if err != nil {
        panic(err)
    }
    fmt.Println(j)
}
```

**API (v3.6.0):** `Parse`, `ParseActive`, `Stringify`, `Format`, `ParseTool`, `Compile`, `Decompile`, `IsSynxb`, `Diff` — как в `synx.h`.

> **Паритет:** тот же движок, что у **`synx-core`** (`!active`, `!tool`, `.synxb`, канонический JSON).

---

### 5.11 Mojo — Python interop (`bindings/mojo`)

[Mojo](https://docs.modular.com/mojo/) вызывает **CPython** через [`Python.import_module`](https://docs.modular.com/mojo/manual/python/python-from-mojo). Чтобы получить **паритет SYNX 3.6.0** с **`synx-core`**, используйте модуль **`synx_native`** (тот же **PyO3 / maturin**, что и `pip install synx-format`). Отдельная **чистая** реализация грамматики на Mojo в этом репозитории **не** поставляется — это был бы отдельный крупный порт (как поддерживать `synx-js` рядом с Rust).

**Установка:** Python-пакет с **`synx_native`**. При локальной сборке — `maturin develop`; при необходимости `Python.add_to_path`.

**Строковые API на стороне Python:** `parse_to_json`, `parse_active_to_json`, `parse_tool_to_json`, `stringify_json`, `format`, `compile_hex`, `decompile_hex`, `is_synxb_hex`, `diff_json`.

Пример: `bindings/mojo/examples/demo.mojo`, документация — [`bindings/mojo/README.md`](../../bindings/mojo/README.md).

> **Паритет:** как у **`synx-core`**, потому что выполняется **`synx_native`** (Rust).

---

### 5.12 Kotlin / JVM — `bindings/kotlin` (JNA + `synx-c`)

Поддерживаемая JVM-интеграция в этом репозитории — **`bindings/kotlin`**: **`SynxEngine`** подгружает **`synx_c`** через [**JNA**](https://github.com/java-native-access/jna). Результаты — **канонический JSON** в виде **`String`** (или **`ByteArray`** для `.synxb`), при необходимости декодируйте сами.

**1. Сборка нативной библиотеки** (корень репозитория): `cargo build -p synx-c --release`.

**2. Сборка / тесты** из `bindings/kotlin`: задайте **`SYNX_LIB_DIR`** на каталог с `libsynx_c` / `synx_c.dll`, затем `./gradlew test`. См. [`bindings/kotlin/README.md`](../../bindings/kotlin/README.md).

**Пример:**

```kotlin
import com.aperturesyndicate.synx.SynxEngine

val json = SynxEngine.parse("name Wario\nage 30\n")
val tool = SynxEngine.parseTool("!tool\nweb_search\n  query test\n")
val active = SynxEngine.parseActive("!active\nport:env:default:8080 PORT\n")
```

**API (v3.6.0):** `SynxEngine.parse`, `parseActive`, `stringify`, `format`, `parseTool`, `diff`, `compile`, `decompile`, `isSynxb` — как в [`bindings/c-header/include/synx.h`](../../bindings/c-header/include/synx.h).

> **Паритет:** тот же движок, что **`synx-core`** (включая **`!active`**, **`!tool`**, **`.synxb`**, **`diff`**). Для Gradle нужен **JDK 17+**; артефакт **`com.aperturesyndicate:synx-kotlin`** на Maven Central может появиться позже; пока — **`publishToMavenLocal`**.

---

### 5.13 Swift — SwiftPM + `synx-c` (C interop)

Поддерживаемая Swift-интеграция — **`bindings/swift`**: пакет **SwiftPM** с модулем **`CSynx`** (`synx.h`, линковка `-lsynx_c`). На границе — **`String`** и **`Data`**; результаты парсинга в основном **канонический JSON** (при необходимости декодируйте сами).

**Сборка C-библиотеки** (корень репозитория): `cargo build -p synx-c --release`, затем `swift build` с `-L` / `-lsynx_c`. См. [`bindings/swift/README.md`](../../bindings/swift/README.md).

**Пример:**

```swift
import Synx

let json = try SynxEngine.parse("name Wario\nage 30\n")
print(json)
```

**API (v3.6.0):** `SynxEngine.parse`, `parseActive`, `stringify(json:)`, `format`, `parseTool`, `diff`, `compile`, `decompile`, `isSynxb` — как в `synx.h`.

> **Паритет:** тот же движок, что **`synx-core`**. Файл `Sources/CSynx/synx.h` должен совпадать с [`bindings/c-header/include/synx.h`](../../bindings/c-header/include/synx.h).

---

### 5.14 Lua — парсер без зависимостей

**Установка:**

Скопируйте `synx.lua` в свой проект:

```lua
local synx = require("synx")
```

**Использование:**

```lua
local synx = require("synx")

-- Парсинг строки
local data = synx.parse(text)
print(data:get("server"):get("port"):as_int())  -- 8080

-- Парсинг файла
local data = synx.parse_file("config.synx")
print(data:get("server"):get("host"):as_str())  -- localhost

-- Полный парсинг (определение режима)
local result = synx.parse_full(text)
print(result.mode)  -- "static" или "active"
```

**API:**

- `synx.parse(text)` → `SynxValue`
- `synx.parse_full(text)` → `{ root, mode }`
- `synx.parse_file(path)` → `SynxValue, err`
- `SynxValue:get(key)` — дочерний элемент по ключу или индексу (с 1)
- `SynxValue:as_bool()`, `:as_int()`, `:as_float()`, `:as_str()`
- `SynxValue:len()`, `:keys()`, `:type()`, `:is_null()`

> 📌 **Важно:** Lua-парсер работает только в режиме `static` — активные маркеры не вычисляются. Работает с Lua 5.1+ и LuaJIT.

---

### 5.15 Dart / Flutter — нативный парсер

**Установка (pubspec.yaml):**

```yaml
dependencies:
  synx:
    git:
      url: https://github.com/APERTURESyndicate/synx-format.git
      path: packages/synx-dart
```

**Использование:**

```dart
import 'package:synx/synx.dart';

// Парсинг строки
final data = Synx.parse(text);
print(data['server']['port'].asInt); // 8080

// Парсинг файла
final data = Synx.parseFile('config.synx');

// Полный парсинг (определение режима)
final result = Synx.parseFull(text);
print(result.mode); // SynxMode.active
```

**Использование во Flutter:**

```dart
import 'package:synx/synx.dart';
import 'package:flutter/services.dart' show rootBundle;

Future<SynxValue> loadConfig() async {
  final text = await rootBundle.loadString('assets/config.synx');
  return Synx.parse(text);
}
```

**API:**

- `Synx.parse(text)` → `SynxValue`
- `Synx.parseFull(text)` → `SynxParseResult { root, mode }`
- `Synx.parseFile(path)` → `SynxValue`
- `SynxValue[key]` — доступ по ключу или индексу
- `.asBool`, `.asInt`, `.asFloat`, `.asStr`
- `.length`, `.keys`, `.type`, `.isNull`

> 📌 **Важно:** Dart-парсер работает только в режиме `static` — активные маркеры не вычисляются. Требуется Dart 3.0+.

---

### 5.16 PHP — нативный парсер

**Установка:**

Скопируйте `Synx.php` в свой проект:

```php
require_once 'Synx.php';
```

**Использование:**

```php
require_once 'Synx.php';

// Парсинг из строки
$data = Synx::parse($text);
echo $data->get('server')->get('port')->asInt(); // 8080

// Парсинг из файла
$data = Synx::loadFile('config.synx');

// Полный парсинг (определение режима)
$result = Synx::parseFull($text);
echo $result->mode; // "static" или "active"
```

**API:**

- `Synx::parse($text)` → `SynxValue`
- `Synx::parseFull($text)` → `SynxParseResult { root, mode }`
- `Synx::loadFile($path)` → `SynxValue`
- `->get($key)` — дочерний элемент по строковому ключу или целочисленному индексу
- `->asBool()`, `->asInt()`, `->asFloat()`, `->asStr()`
- `->length()`, `->keys()`, `->type()`, `->isNull()`

> 📌 **Важно:** PHP-парсер работает только в режиме `static` — активные маркеры не вычисляются. Требуется PHP 8.0+.

---

### 5.17 Bash / PowerShell — парсеры для командных оболочек

#### Bash (4.0+)

**Установка:**

```bash
source synx.sh
```

**Использование:**

```bash
source synx.sh

synx_parse_file "config.synx"
echo "$(synx_get server.host)"     # localhost
echo "$(synx_get server.port)"     # 8080
echo "$(synx_type server.port)"    # int
echo "$(synx_mode)"                # static

# Массивы
echo "$(synx_get items.0)"         # Sword
echo "$(synx_get items.__length)"  # 3
```

**API:**

- `synx_parse "$text"` — парсинг текста SYNX
- `synx_parse_file "$path"` — чтение и парсинг файла
- `synx_get "$path"` — значение по dot-notation пути
- `synx_type "$path"` — тип значения
- `synx_mode` — режим документа

> **Примечание:** Значения хранятся в плоских ассоциативных массивах с dot-notation ключами (например `server.host`).

#### PowerShell (5.1+)

**Установка:**

```powershell
. .\Synx.ps1
```

**Использование:**

```powershell
. .\Synx.ps1

$result = Read-SynxFile "config.synx"
$result.Root.server.host           # localhost
$result.Root.server.port           # 8080 (int)
$result.Root.items[0]              # Sword
$result.Mode                       # static
```

**API:**

- `ConvertFrom-Synx $text` → `PSCustomObject { Root, Mode }`
- `Read-SynxFile $path` → то же самое

Объекты — `[ordered]@{}`, массивы — нативные PowerShell-массивы, скаляры типизированы (`[long]`, `[double]`, `[bool]`, `[string]`, `$null`).

> 📌 **Важно:** Оба парсера для командных оболочек работают только в режиме `static` — активные маркеры не вычисляются.

---

### 5.18 Сравнение: SYNX напрямую vs JSON

| | JSON | SYNX |
|---|---|---|
| **Чтение (JS)** | `JSON.parse(fs.readFileSync(...))` | `Synx.loadSync('file.synx')` |
| **Чтение (Python)** | `json.load(open(...))` | `Synx.load('file.synx')` |
| **Чтение (Rust)** | `serde_json::from_str(...)` | `Synx::parse_file(...)` |
| **Чтение (C)** | `cJSON_Parse(...)` | `synx_parse(text)` |
| **Чтение (C++)** | `nlohmann::json::parse(...)` | `synx::Synx::load(path)` |
| **Чтение (C#)** | `JsonSerializer.Deserialize(...)` | `SynxFormat.Parse(File.ReadAllText(path))` |
| **Чтение (Go)** | `json.Unmarshal(...)` | `synx.ParseFile(path)` |
| **Чтение (Java)** | `new ObjectMapper().readTree(...)` | `Synx.load(path)` |
| **Чтение (Swift)** | `JSONDecoder().decode(...)` | `Synx.load(path)` |
| **Чтение (Lua)** | `cjson.decode(...)` | `synx.parse(text)` |
| **Чтение (Dart)** | `jsonDecode(...)` | `Synx.parse(text)` |
| **Чтение (PHP)** | `json_decode(...)` | `Synx::parse($text)` |
| **Чтение (Bash)** | `jq '.key' file.json` | `synx_get "key"` |
| **Чтение (PowerShell)** | `ConvertFrom-Json` | `ConvertFrom-Synx` |
| **Промежуточный формат** | Нет | Нет — тоже нет! |
| **Что получаете** | Object / dict / Value | Object / dict / Value (то же самое) |
| **Встроенная логика** | Нет — пишете руками | `:random`, `:calc`, `:env` из коробки |
| **Валидация** | Нет — нужна отдельная библиотека | `[min:3, type:int]` прямо в файле |
| **Размер файла** | ~100% | ~40% меньше (нет кавычек/скобок) |

**По языкам:**

| Язык | Пакет | Метод | Примечание |
|---|---|---|---|
| JavaScript | `@aperturesyndicate/synx-format` | `Synx.loadSync()` | Полный движок (active + static) |
| Python | `synx-format` | `Synx.load()` | Полный движок (active + static) |
| Rust | `synx` (crates.io) | `Synx::parse()` | Без зависимостей, static-only |
| C | `synx.h` + lib `synx-c` | `synx_parse()` → JSON | FFI к `synx-core` 3.6.x |
| C++ | `synx/synx.hpp` + та же lib | `synx::parse()` → `optional<string>` | Тонкая обёртка, тот же движок |
| C# | `APERTURESyndicate.Synx` (NuGet) | `SynxFormat` / API парсера | Библиотека .NET 8; идентификатор `Synx.Core` занят на nuget.org |
| Go | `bindings/go` (cgo) + `synx-c` | `synx.Parse()` → JSON | Тот же движок, что Rust |
| Mojo | `bindings/mojo` + CPython `synx_native` | `parse_json()` / `parse_active_json()` … | Паритет через PyO3; не чистый Mojo-парсер |
| Kotlin/JVM | `bindings/kotlin` + `synx-c` | `SynxEngine.parse()` → JSON `String` | JNA; сборка JDK 17+; тот же движок, что Rust |
| Swift | `bindings/swift` + `synx-c` | `SynxEngine.parse()` → JSON `String` | C interop; тот же движок, что Rust |
| Lua | `synx.lua` (копирование) | `synx.parse()` | Lua 5.1+, без зависимостей, static-only |
| Dart/Flutter | `synx` (pub.dev / git) | `Synx.parse()` | Dart 3.0+, static-only |
| PHP | `Synx.php` (копирование) | `Synx::parse()` | PHP 8.0+, без зависимостей, static-only |
| Bash | `synx.sh` (копирование) | `synx_get()` | Bash 4.0+, без зависимостей, static-only |
| PowerShell | `Synx.ps1` (копирование) | `ConvertFrom-Synx` | PowerShell 5.1+, без зависимостей, static-only |

---

## 6. Конвертация в JSON (опционально)

> 📎 Конвертация в JSON — это **необязательная** возможность. Она нужна только если вы хотите передать данные в систему, которая понимает исключительно JSON (сторонний API, legacy-код, и т.д.).

### 6.1 Через VS Code (расширение SYNX)

Если установлено расширение **SYNX for VS Code**:

1. Откройте `.synx` файл
2. Нажмите **ПКМ** (правой кнопкой мыши) по файлу
3. Выберите **«Конвертировать в JSON»**
4. Готовый `.json` файл появится рядом

### 6.2 Через терминал (CLI)

```bash
# Конвертировать в JSON и вывести в консоль
synx to-json config.synx

# Конвертировать и сохранить в файл
synx to-json config.synx -o config.json

# Заморозить !active конфиг в статичный .synx (без функций)
synx freeze active_config.synx -o static_config.synx
```

### 6.3 Программно (если очень нужно)

```python
import json
from synx import Synx

data = Synx.load('config.synx')
json_string = json.dumps(data, ensure_ascii=False, indent=2)
```

```javascript
const Synx = require('@aperturesyndicate/synx-format');
const data = Synx.loadSync('config.synx');
const jsonString = JSON.stringify(data, null, 2);
```

> Но ещё раз: **это не нужно для работы**. Ваш код читает `.synx` напрямую.

---

## 7. Краткая шпаргалка

```
КЛЮЧ ЗНАЧЕНИЕ                    → простая пара
ключ                             → пустой объект (группа)
  вложенный_ключ значение        → вложенность (2 пробела)
ключ |                           → многострочный текст
  строка 1                         (блок)
  строка 2
список                           → список
  - элемент 1
  - элемент 2
# комментарий                    → однострочный комментарий
// комментарий                   → однострочный комментарий

─── Только с !active ───────────────────────────
ключ:random                      → случайный элемент
ключ:random 70 20 10             → взвешенный рандом
ключ:calc A * B                  → арифметика
ключ:env VAR                     → переменная окружения
ключ:env:default:X VAR           → env с дефолтом
ключ:alias другой_ключ           → ссылка на другой ключ
ключ:secret значение             → скрытое значение
ключ:unique                      → дедупликация списка
ключ:include ./файл.synx         → подключить файл
ключ[min:N, max:N]               → ограничения длины/значения
ключ[type:int]                   → тип данных
ключ[required]                   → обязательное поле
ключ[enum:a|b|c]                 → допустимые значения
ключ[pattern:regex]              → regex-валидация
ключ[readonly]                   → только для чтения
```
