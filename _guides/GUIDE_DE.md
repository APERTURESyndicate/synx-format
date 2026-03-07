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
  - [:ref — Referenz mit Verkettung](#ref--referenz-mit-verkettung)
  - [:inherit — Blockvererbung](#inherit--blockvererbung)
  - [:i18n — Mehrsprachige Werte](#i18n--mehrsprachige-werte)
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

#### Zufallswert-Generierung

Generiere Zufallswerte beim Parsen mit `(random)`:

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

Verfügbare Typen: `(random)` (int), `(random:int)`, `(random:float)`, `(random:bool)`.

> Werte werden bei jedem Parsen generiert — jeder Aufruf erzeugt neue Werte.

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

###
Dies ist ein Blockkommentar.
Alles zwischen ### wird ignoriert.
###
```

In der VSCode-Erweiterung wird Formatierung in Kommentaren unterstützt:
- `*kursiv*` — grün
- `**fett**` — lila
- `***fett+kursiv***` — gold
- `` `code` `` — orange mit Hintergrund

---

## 🔥 Aktiver Modus (`!active`)

Setze `!active` in die **erste Zeile**, um Marker und Einschränkungen freizuschalten.

```synx
!active

port:env PORT
boss_hp:calc base_hp * 5
```

---

## 🔐 Sperrmodus (`!lock`)

Füge `!lock` hinzu, um zu verhindern, dass externer Code Werte über `Synx.set()`, `Synx.add()`, `Synx.remove()` ändert. Interne SYNX-Marker funktionieren weiterhin normal.

```synx
!active
!lock

max_players 100
greeting:random
  - Hallo!
  - Willkommen!
```

```typescript
const config = Synx.loadSync('./config.synx');

Synx.set(config, 'max_players', 200);
// ❌ Fehler: "SYNX: Cannot set "max_players" — config is locked (!lock)"

console.log(config.max_players); // ✅ 100 (Lesen ist immer erlaubt)
```

Verwende `Synx.isLocked(config)` zur Überprüfung.

---

## 🧹 Kanonisches Format (`format`)

`Synx.format()` schreibt jede `.synx`-Datei in eine einzige, normalisierte Form um.

**Was es tut:**
- **Schlüssel alphabetisch sortiert** auf jeder Verschachtelungsebene
- **Einrückung normalisiert** auf genau 2 Leerzeichen pro Ebene
- **Kommentare entfernt** — das kanonische Format enthält nur Daten
- **Eine Leerzeile** zwischen Top-Level-Blöcken (Objekte und Listen)
- **Direktiven** (`!active`, `!lock`) bleiben am Anfang der Datei
- **Reihenfolge von Listenelementen bleibt erhalten** — nur benannte Schlüssel werden sortiert

### Warum das für Git wichtig ist

Ohne kanonisches Format schreiben zwei Entwickler dieselbe Konfiguration unterschiedlich:

```synx
# Entwickler A               # Entwickler B
server                       server
    port 8080                  host 0.0.0.0
    host 0.0.0.0               port 8080
```

`git diff` zeigt den gesamten Block als geändert — obwohl die Daten identisch sind.

Nach `Synx.format()` erzeugen beide:

```synx
server
  host 0.0.0.0
  port 8080
```

Eine kanonische Form. Null Rauschen in Diffs.

### Verwendung

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

### `:ref` — Referenz mit Verkettung

Wie `:alias`, gibt aber den aufgelösten Wert an nachfolgende Marker weiter. Unterstützt Kurzform-Berechnungen.

```synx
!active

base_rate 50
quick_rate:ref base_rate
double_rate:ref:calc:*2 base_rate
```

Die Kurzform `:ref:calc:*2` löst die Referenz auf und wendet die arithmetische Operation auf den Wert an.

---

### `:inherit` — Blockvererbung

Führt alle Felder eines Elternblocks mit einem Kindblock zusammen. Kindwerte haben Vorrang. Präfix `_` macht den Block privat — er wird aus der Ausgabe ausgeschlossen.

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

### `:i18n` — Mehrsprachige Werte

Wählt einen lokalisierten Wert aus verschachtelten Sprachschlüsseln. Übergeben Sie `lang` in den Optionen. Fallback: `en` → erster verfügbarer Wert.

```synx
!active

title:i18n
  en Hello World
  de Hallo Welt
  ru Привет мир
```

```javascript
const config = Synx.parse(text, { lang: 'de' });
// config.title → "Hallo Welt"
```

---

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

Trennzeichen-Schlüsselwörter: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`, `slash`

### `:join` — Array zu String

Trennzeichen-Schlüsselwörter: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`, `slash`. Standard: Komma.

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

### ✅ Marker-Kompatibilitaet

Gut funktionierende Kombinationen:

- `env:default`
- `calc:round`
- `split:unique`
- `split:join` (ueber ein Zwischen-Array)

Wichtige Einschraenkungen:

- `!active` ist erforderlich, sonst werden Marker nicht ausgewertet.
- Einige Marker sind typabhaengig: `split` erwartet String, `join` erwartet Array, `round`/`clamp` erwarten Zahlen.
- Marker-Argumente werden rechts in der Kette gelesen (z. B. `clamp:min:max`, `round:n`, `map:key`).
- Wenn ein frueher Marker den Typ aendert, kann ein spaeter Marker nicht mehr greifen.

---

## � CLI-Werkzeug

> Hinzugefügt in v3.1.3.

Installation über npm:

```bash
npm install -g @aperturesyndicate/synx
```

### `synx convert` — Export in andere Formate

```bash
# SYNX → JSON
synx convert config.synx --format json

# SYNX → YAML (für Helm, Ansible, K8s)
synx convert config.synx --format yaml > values.yaml

# SYNX → TOML
synx convert config.synx --format toml

# SYNX → .env (für Docker Compose)
synx convert config.synx --format env > .env

# Mit striktem Modus (Fehler bei jedem Marker-Problem)
synx convert config.synx --format json --strict
```

### `synx validate` — CI/CD-Validierung

```bash
synx validate config.synx --strict
# Exit-Code 0 bei Erfolg, 1 bei INCLUDE_ERR / WATCH_ERR / CALC_ERR / CONSTRAINT_ERR
```

### `synx watch` — Live-Neuladen

```bash
# JSON bei jeder Änderung ausgeben
synx watch config.synx --format json

# Befehl bei jeder Änderung ausführen (z. B. Nginx neuladen)
synx watch config.synx --exec "nginx -s reload"
```

### `synx schema` — JSON Schema aus Constraints extrahieren

```bash
synx schema config.synx
# Gibt JSON Schema basierend auf [required, min:N, max:N, type:T, enum:A|B, pattern:R] aus
```

---

## 📤 Exportformate (JS/TS API)

> Hinzugefügt in v3.1.3.

Ein geparsten SYNX-Objekt in JSON, YAML, TOML oder .env konvertieren:

```typescript
import Synx from '@aperturesyndicate/synx';

const config = Synx.loadSync('config.synx');

// JSON
const json = Synx.toJSON(config);          // formatiert
const compact = Synx.toJSON(config, false); // kompakt

// YAML
const yaml = Synx.toYAML(config);

// TOML
const toml = Synx.toTOML(config);

// .env (KEY=VALUE-Format)
const env = Synx.toEnv(config);            // APP_NAME=TotalWario
const prefixed = Synx.toEnv(config, 'APP'); // APP_APP_NAME=TotalWario
```

---

## 📋 Schema-Export

> Hinzugefügt in v3.1.3.

SYNX-Constraints als JSON-Schema-Objekt extrahieren:

```typescript
const schema = Synx.schema(`
!active
app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
`);
```

Ergebnis:

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

## 👁 Dateiüberwachung

> Hinzugefügt in v3.1.3.

Eine `.synx`-Datei überwachen und bei jeder Änderung die aktualisierte Konfiguration erhalten:

```typescript
const handle = Synx.watch('config.synx', (config, error) => {
  if (error) {
    console.error('Konfiguration konnte nicht neu geladen werden:', error.message);
    return;
  }
  console.log('Konfiguration aktualisiert:', config.server.port);
}, { strict: true });

// Überwachung stoppen
handle.close();
```

---

## 🐳 Deployment-Handbuch

> Hinzugefügt in v3.1.3.

### Docker + Docker Compose

SYNX dient als **einzige Quelle der Wahrheit** für die gesamte Dienstkonfiguration. Dienste, die ihr eigenes Konfigurationsformat benötigen (Nginx, Redis usw.), erhalten beim Start generierte Konfigurationen.

**Muster:**

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   config.synx   │────▶│  Startskript    │────▶│  nginx.conf     │
│  (eine Datei)   │     │  oder CLI conv. │     │  .env           │
│  :env :default  │     │                 │     │  redis.conf     │
│  :template      │     │                 │     │  App-Settings   │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

**Schritt 1 — Konfiguration schreiben:**

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

**Schritt 2 — .env für Docker Compose generieren:**

```bash
synx convert config.synx --format env > .env
```

**Schritt 3 — In docker-compose.yml verwenden:**

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

### Nginx-Konfiguration generieren

Verwenden Sie ein Template + Startskript, um `nginx.conf` aus SYNX zu generieren:

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

### Redis-Verbindung

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

### PostgreSQL-Verbindung

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

K8s mountet Secrets als Dateien unter `/run/secrets/`. Verwenden Sie `:watch` zum Lesen:

```synx
!active

db_password:watch /run/secrets/db-password
api_key:watch /run/secrets/api-key
```

Docker Secrets funktionieren identisch — gemountet unter `/run/secrets/`.

### HashiCorp Vault

Verwenden Sie Vault Agent, um Secrets in Dateien zu schreiben, dann mit `:watch` lesen:

```synx
!active

db_creds:watch:password /vault/secrets/database
api_key:watch:key /vault/secrets/api-key
```

Oder per Umgebungsvariablen injizieren mit Vault Agents `env_template`:

```synx
!active

db_password:env VAULT_DB_PASSWORD
api_key:env VAULT_API_KEY
```

### Helm / Kubernetes

SYNX in YAML für Helm-Values konvertieren:

```bash
synx convert config.synx --format yaml > helm/values.yaml
helm upgrade my-release ./chart -f helm/values.yaml
```

### Terraform

Terraform akzeptiert JSON-Variablendateien:

```bash
synx convert config.synx --format json > terraform.tfvars.json
terraform apply -var-file=terraform.tfvars.json
```

### CI/CD-Pipeline-Validierung

Fügen Sie diese Prüfung zu Ihrer CI-Pipeline hinzu:

```yaml
# GitHub Actions Beispiel
- name: SYNX-Konfiguration validieren
  run: npx @aperturesyndicate/synx validate config.synx --strict
```

---

## �💻 Codebeispiele

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

**Laufzeit-Manipulation (set / add / remove):**

```typescript
import { Synx } from '@aperturesyndicate/synx';

const config = Synx.loadSync('./game.synx');

// Wert setzen
Synx.set(config, 'max_players', 100);
Synx.set(config, 'server.host', 'localhost');

// Wert abrufen
const port = Synx.get(config, 'server.port'); // 8080

// Zur Liste hinzufügen
Synx.add(config, 'maps', 'Arena of Doom');

// Aus Liste entfernen
Synx.remove(config, 'maps', 'Arena of Doom');

// Schlüssel komplett löschen
Synx.remove(config, 'deprecated_key');

// Sperrstatus prüfen
if (!Synx.isLocked(config)) {
  Synx.set(config, 'motd', 'Willkommen!');
}
```

> **Hinweis:** Wenn die `.synx`-Datei `!lock` enthält, werfen alle `set`/`add`/`remove`-Aufrufe einen Fehler.

**Zugriffsmethoden (JS/TS API):**

- `Synx.get(obj, keyPath)` — Wert per Dot-Path lesen.
- `Synx.set(obj, keyPath, value)` — Wert per Dot-Path setzen.
- `Synx.add(obj, keyPath, item)` — Element zu einem Array hinzufuegen.
- `Synx.remove(obj, keyPath, item?)` — Array-Element entfernen oder Schluessel loeschen.
- `Synx.isLocked(obj)` — pruefen, ob die Konfiguration durch `!lock` gesperrt ist.

### Python

Aktuell exportiert `synx_native`: `parse`, `parse_active`, `parse_to_json`.

Python-Aequivalente fuer `get`/`set`/`add`/`remove`:

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

# Nutzung der Python-Access-Helper
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
