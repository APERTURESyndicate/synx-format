<p align="center">
  <a href="https://aperturesyndicate.com/branding/aperturesyndicate.png" target="_blank">
    <img src="https://aperturesyndicate.com/branding/aperturesyndicate.png" alt="APERTURESyndicate" width="400" />
  </a>
</p>

> **🔗 [Logo anzeigen →](https://aperturesyndicate.com/branding/aperturesyndicate.png)**

<h1 align="center">SYNX v3.0 — Vollständiger Leitfaden</h1>

<p align="center">
  <strong>Besser als JSON. Günstiger als YAML. Gemacht für KI und Menschen.</strong>
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

## Inhaltsverzeichnis

- [Designphilosophie](#-designphilosophie)
- [Demonstration](#-demonstration)
- [Funktionsweise](#-funktionsweise)
- [Leistung und Benchmarks](#-leistung-und-benchmarks)
- [Installation](#-installation)
- [Syntax-Referenz](#-syntax-referenz)
  - [Grundsyntax](#grundsyntax)
  - [Verschachtelung](#verschachtelung)
  - [Listen](#listen)
  - [Typkonvertierung](#typkonvertierung)
  - [Mehrzeiliger Text](#mehrzeiliger-text)
  - [Kommentare](#kommentare)
- [Aktiver Modus (`!active`)](#-aktiver-modus-active)
- [Vollständige Marker-Referenz](#-vollständige-marker-referenz)
  - [:env — Umgebungsvariablen](#env--umgebungsvariablen)
  - [:default — Standardwert](#default--standardwert)
  - [:calc — Arithmetische Ausdrücke](#calc--arithmetische-ausdrücke)
  - [:random — Zufällige Auswahl](#random--zufällige-auswahl)
  - [:alias — Verweis auf Anderen Schlüssel](#alias--verweis-auf-anderen-schlüssel)
  - [:secret — Versteckter Wert](#secret--versteckter-wert)
  - [:template — String-Interpolation](#template--string-interpolation)
  - [:include — Externe Datei Importieren](#include--externe-datei-importieren)
  - [:unique — Duplikate Entfernen](#unique--duplikate-entfernen)
  - [:split — String zu Array](#split--string-zu-array)
  - [:join — Array zu String](#join--array-zu-string)
  - [:geo — Regionsbasierte Auswahl](#geo--regionsbasierte-auswahl)
  - [:clamp — Numerische Begrenzung](#clamp--numerische-begrenzung)
  - [:round — Rundung](#round--rundung)
  - [:map — Nachschlagetabelle](#map--nachschlagetabelle)
  - [:format — Zahlenformatierung](#format--zahlenformatierung)
  - [:fallback — Dateipfad-Fallback](#fallback--dateipfad-fallback)
  - [:once — Generieren und Persistieren](#once--generieren-und-persistieren)
  - [:version — Semantischer Versionsvergleich](#version--semantischer-versionsvergleich)
  - [:watch — Externe Datei Lesen](#watch--externe-datei-lesen)
- [Einschränkungen](#-einschränkungen)
- [Marker-Verkettung](#-marker-verkettung)
- [Codebeispiele](#-codebeispiele)
- [Editor-Unterstützung](#-editor-unterstützung)
- [Architektur](#-architektur)
- [Links](#-links)

---

## 💡 Designphilosophie

Konfiguration ist das Fundament jeder Anwendung. Doch die Industriestandard-Formate — **JSON** und **YAML** — wurden nie dafür entworfen:

| Problem | JSON | YAML | SYNX |
|---|:---:|:---:|:---:|
| Erfordert Anführungszeichen für Strings/Schlüssel | ✓ | ✗ | ✗ |
| Trailing-Komma bricht Parsing | ✗ | — | ✓ |
| Leerzeichensensitive Einrückung | — | ✗ (gefährlich) | ✓ (sicher, 2 Leerzeichen) |
| Kommentar-Unterstützung | ✗ | ✓ | ✓ |
| Umgebungsvariablen | ✗ | ✗ | ✓ nativ |
| Berechnete Werte | ✗ | ✗ | ✓ nativ |
| KI-Token-Kosten (110 Schlüssel) | ~3300 Zeichen | ~2500 Zeichen | **~2000 Zeichen** |
| Lesbarkeit | Niedrig | Mittel | **Hoch** |

SYNX basiert auf drei Prinzipien:

1. **Minimale Syntax** — Schlüssel, Leerzeichen, Wert. Keine Anführungszeichen, keine Kommas, keine geschweiften Klammern, keine Doppelpunkte.
2. **Von Natur aus aktiv** — Konfiguration ist nicht nur Daten, sie ist Logik. Umgebungsvariablen, Mathematik, Referenzen, Zufallsauswahl und Validierung — alles in das Format integriert.
3. **Token-effizient** — Beim Senden von Konfiguration über ein LLM zählt jedes Zeichen. SYNX spart 30–40% Token im Vergleich zu JSON.

> **SYNX ist kein JSON-Ersatz. SYNX ist das, was JSON hätte sein sollen.**

---

## 🎬 Demonstration

### Daten schreiben — sauber und einfach

Nur **Schlüssel**, **Leerzeichen**, **Wert**. Keine Anführungszeichen, keine Kommas, keine geschweiften Klammern:

<p align="center">
  <a href="https://aperturesyndicate.com/branding/gifs/synx/synx.gif" target="_blank">
    <img src="https://aperturesyndicate.com/branding/gifs/synx/synx.gif" alt="Statisches SYNX schreiben" width="720" />
  </a>
</p>

> **📺 [Demo ansehen →](https://aperturesyndicate.com/branding/gifs/synx/synx.gif)**

### `!active` Modus — Konfiguration mit Logik

Füge `!active` in der ersten Zeile hinzu, und deine Konfiguration wird lebendig — Funktionen direkt im Format integriert:

<p align="center">
  <a href="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" target="_blank">
    <img src="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" alt="Aktives SYNX mit Markern schreiben" width="720" />
  </a>
</p>

> **📺 [Demo ansehen →](https://aperturesyndicate.com/branding/gifs/synx/synx2.gif)**

---

## ⚙ Funktionsweise

Die SYNX-Pipeline hat **zwei Stufen** — diese Trennung ist der Schlüssel zur Leistung:

```
┌───────────────┐         ┌─────────────┐         ┌──────────────┐
│  .synx-Datei  │ ──────► │   Parser    │ ──────► │   Ausgabe    │
│  (Text)       │         │  (immer)    │         │ (JS-Objekt)  │
└───────────────┘         └──────┬──────┘         └──────────────┘
                                 │
                            hat !active?
                                 │
                            ┌────▼────┐
                            │  Engine │
                            │(führt   │
                            │ Marker  │
                            │  aus)   │
                            └─────────┘
```

### Stufe 1 — Parser

Der **Parser** liest den Rohtext und baut den Schlüssel-Wert-Baum auf. Er verarbeitet Schlüssel-Wert-Paare, Verschachtelung (2-Leerzeichen-Einrückung), Listen, Typkonvertierung, Kommentare und mehrzeiligen Text.

Der Parser zeichnet Marker (`:env`, `:calc` usw.) als an jeden Schlüssel angehängte **Metadaten** auf, **führt sie aber nicht aus**. Das bedeutet, dass **das Hinzufügen neuer Marker das Parsing nicht verlangsamt**.

### Stufe 2 — Engine (nur mit `!active`)

Wenn die Datei mit `!active` beginnt, durchläuft die **Engine** den geparsten Baum und löst jeden Marker auf.

**Dateien ohne `!active` berühren die Engine nie.**

---

## 📊 Leistung und Benchmarks

Alle Benchmarks mit echten Daten, ausgeführt auf einer Standard-SYNX-Konfiguration mit 110 Schlüsseln (2,5 KB):

### Rust (criterion, direkter Aufruf)

| Benchmark | Zeit |
|---|---|
| `Synx::parse` (110 Schlüssel) | **~39 µs** |
| `parse_to_json` (110 Schlüssel) | **~42 µs** |
| `Synx::parse` (4 Schlüssel) | **~1,2 µs** |

### Node.js (50.000 Iterationen)

| Parser | µs/Op | vs JSON | vs YAML |
|---|---:|---:|---:|
| `JSON.parse` (3,3 KB) | 6,08 µs | 1× | — |
| **`synx-js` reines TS** | **39,20 µs** | 6,4× | **2,1× schneller als YAML** |
| `js-yaml` (2,5 KB) | 82,85 µs | 13,6× | 1× |

### Python (10.000 Iterationen)

| Parser | µs/Op | vs YAML |
|---|---:|---:|
| `json.loads` (3,3 KB) | 13,04 µs | — |
| **`synx_native.parse`** | **55,44 µs** | **67× schneller als YAML** |
| `yaml.safe_load` (2,5 KB) | 3.698 µs | 1× |

> In Python parst SYNX **67-mal** schneller als YAML.

---

## 📦 Installation

### Node.js / Browser

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

### VS Code-Erweiterung

Suche nach **"SYNX"** im Erweiterungs-Panel, oder:

```bash
code --install-extension APERTURESyndicate.synx-vscode
```

---

## 📝 Syntax-Referenz

### Grundsyntax

Grundregel: **Schlüssel** `(Leerzeichen)` **Wert**.

```synx
name John
age 25
phrase Ich liebe Programmieren!
empty_value
```

> Zahlen, Booleans (`true`/`false`) und `null` werden automatisch erkannt. Alles andere ist String.

---

### Verschachtelung

Einrückung erzeugt Hierarchie — **2 Leerzeichen** pro Ebene:

```synx
server
  host 0.0.0.0
  port 8080
  ssl
    enabled true
```

---

### Listen

Zeilen, die mit `- ` beginnen, erzeugen Arrays:

```synx
fruits
  - Apple
  - Banana
  - Cherry
```

---

### Typkonvertierung

Verwende `(Typ)` nach dem Schlüsselnamen, um den Typ zu erzwingen:

```synx
zip_code(string) 90210
id(int) 007
ratio(float) 3
enabled(bool) 1
```

Verfügbare Typen: `int`, `float`, `bool`, `string`.

---

### Mehrzeiliger Text

Verwende den `|`-Operator:

```synx
description |
  Dies ist eine lange Beschreibung,
  die sich über mehrere Zeilen erstreckt.
```

---

### Kommentare

```synx
# Hash-Kommentar
// Schrägstrich-Kommentar
name John  # Inline-Kommentar
```

---

## 🔥 Aktiver Modus (`!active`)

Setze `!active` in die **erste Zeile**, um Marker und Einschränkungen freizuschalten.

```synx
!active

port:env PORT
boss_hp:calc base_hp * 5
```

---

## 🧩 Vollständige Marker-Referenz

SYNX v3.0 bietet **20 Marker**. Jeder Marker ist eine Funktion, die über die `:marker`-Syntax an einen Schlüssel angehängt wird.

### `:env` — Umgebungsvariablen

```synx
!active
port:env PORT
port:env:default:8080 PORT
```

### `:default` — Standardwert

```synx
!active
theme:default dark
```

### `:calc` — Arithmetische Ausdrücke

```synx
!active
base_price 100
tax:calc base_price * 0.2
total:calc base_price + tax
```

Operatoren: `+` `-` `*` `/` `%` `(` `)`

### `:random` — Zufällige Auswahl

```synx
!active
loot:random 70 20 10
  - common
  - rare
  - legendary
```

### `:alias` — Verweis auf Anderen Schlüssel

```synx
!active
admin_email alex@example.com
billing:alias admin_email
```

### `:secret` — Versteckter Wert

```synx
!active
api_key:secret sk-1234567890
```

### `:template` — String-Interpolation

```synx
!active
name John
greeting:template Hallo, {name}!
```

### `:include` — Externe Datei Importieren

```synx
!active
database:include ./db.synx
```

### `:unique` — Duplikate Entfernen

```synx
!active
tags:unique
  - action
  - rpg
  - action
```

Ergebnis: `["action", "rpg"]`

### `:split` — String zu Array

```synx
!active
colors:split red, green, blue
words:split:space hello world foo
```

Trennzeichen-Schlüsselwörter: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`

### `:join` — Array zu String

```synx
!active
path:join:slash
  - home
  - user
  - docs
```

Ergebnis: `"home/user/docs"`

### `:geo` — Regionsbasierte Auswahl

```synx
!active
currency:geo
  - US USD
  - DE EUR
  - JP JPY
```

### `:clamp` — Numerische Begrenzung

```synx
!active
volume:clamp:0:100 150
```

Ergebnis: `100`

### `:round` — Rundung

```synx
!active
price:round:2 19.999
profit:calc:round:2 revenue * 0.337
```

### `:map` — Nachschlagetabelle

```synx
!active
status_code 1
status:map:status_code
  - 0 offline
  - 1 online
  - 2 abwesend
```

Ergebnis: `"online"`

### `:format` — Zahlenformatierung

```synx
!active
price:format:%.2f 1234.5
id:format:%06d 42
```

Ergebnis: `"1234.50"`, `"000042"`

### `:fallback` — Dateipfad-Fallback

```synx
!active
icon:fallback:./default.png ./custom.png
```

### `:once` — Generieren und Persistieren

```synx
!active
session_id:once uuid
app_seed:once random
build_time:once timestamp
```

Generierungstypen: `uuid` (Standard), `random`, `timestamp`

### `:version` — Semantischer Versionsvergleich

```synx
!active
runtime:version:>=:18.0 20.11.0
```

Ergebnis: `true`. Operatoren: `>=` `<=` `>` `<` `==` `!=`

### `:watch` — Externe Datei Lesen

```synx
!active
app_name:watch:name ./package.json
config:watch ./data.txt
```

---

## 🔒 Einschränkungen

Einschränkungen validieren Werte beim Parsing. Sie werden in `[eckigen Klammern]` nach dem Schlüsselnamen definiert.

| Einschränkung | Syntax | Beschreibung |
|---|---|---|
| `required` | `key[required]` | Muss einen Wert haben |
| `readonly` | `key[readonly]` | Schreibgeschützt |
| `min:N` | `key[min:3]` | Mindestlänge/-wert |
| `max:N` | `key[max:100]` | Maximale Länge/Wert |
| `type:T` | `key[type:int]` | Typ erzwingen |
| `pattern:R` | `key[pattern:^\d+$]` | Regex-Validierung |
| `enum:A\|B` | `key[enum:light\|dark]` | Erlaubte Werte |

```synx
!active
app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
```

---

## 🔗 Marker-Verkettung

```synx
!active
port:env:default:8080 PORT
profit:calc:round:2 revenue * margin
```

---

## 💻 Codebeispiele

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

## 🛠 Editor-Unterstützung

### Visual Studio Code

Vollständige Sprachunterstützung: Syntaxhervorhebung, IntelliSense (20 Marker), Echtzeit-Diagnose (15 Prüfungen), Gehe-zu-Definition, Formatierung, Farbvorschau, `:calc` Inline-Hinweise, Live-JSON-Vorschau.

### Visual Studio 2022

MEF-Erweiterung: Syntaxhervorhebung, IntelliSense, Fehlermarkierung, Code-Faltung, Konvertierungsbefehle.

---

## 🏗 Architektur

```
synx-format/
├── crates/synx-core/          # Rust-Kern — Parser + Engine
├── bindings/
│   ├── node/                  # NAPI-RS → npm-Nativmodul
│   └── python/                # PyO3 → PyPI-Nativmodul
├── packages/
│   ├── synx-js/               # Reiner TypeScript-Parser + Engine
│   ├── synx-vscode/           # VS Code-Erweiterung
│   └── synx-visualstudio/     # Visual Studio 2022-Erweiterung
├── publish-npm.bat
├── publish-pypi.bat
└── publish-crates.bat
```

---

## 🔗 Links

| Ressource | URL |
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
