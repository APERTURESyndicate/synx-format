<p align="center">
  <a href="https://aperturesyndicate.com/branding/aperturesyndicate.png" target="_blank">
    <img src="https://aperturesyndicate.com/branding/aperturesyndicate.png" alt="APERTURESyndicate" width="400" />
  </a>
</p>

> **🔗 [Ver logotipo →](https://aperturesyndicate.com/branding/aperturesyndicate.png)**

<h1 align="center">SYNX v3.0 — Guía Completa</h1>

<p align="center">
  <strong>Mejor que JSON. Más barato que YAML. Hecho para IA y humanos.</strong>
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

## Tabla de Contenidos

- [Filosofía de Diseño](#-filosofía-de-diseño)
- [Demostración](#-demostración)
- [Cómo Funciona](#-cómo-funciona)
- [Rendimiento y Benchmarks](#-rendimiento-y-benchmarks)
- [Instalación](#-instalación)
- [Referencia de Sintaxis](#-referencia-de-sintaxis)
  - [Sintaxis Básica](#sintaxis-básica)
  - [Anidación](#anidación)
  - [Listas](#listas)
  - [Conversión de Tipos](#conversión-de-tipos)
  - [Texto Multilínea](#texto-multilínea)
  - [Comentarios](#comentarios)
- [Modo Activo (`!active`)](#-modo-activo-active)
- [Referencia de Marcadores](#-referencia-completa-de-marcadores)
  - [:env — Variables de Entorno](#env--variables-de-entorno)
  - [:default — Valor por Defecto](#default--valor-por-defecto)
  - [:calc — Expresiones Aritméticas](#calc--expresiones-aritméticas)
  - [:random — Selección Aleatoria](#random--selección-aleatoria)
  - [:alias — Referencia a Otra Clave](#alias--referencia-a-otra-clave)
  - [:secret — Valor Oculto](#secret--valor-oculto)
  - [:template — Interpolación de Cadenas](#template--interpolación-de-cadenas)
  - [:include — Importar Archivo Externo](#include--importar-archivo-externo)
  - [:unique — Eliminar Duplicados](#unique--eliminar-duplicados)
  - [:split — Cadena a Arreglo](#split--cadena-a-arreglo)
  - [:join — Arreglo a Cadena](#join--arreglo-a-cadena)
  - [:geo — Selección por Región](#geo--selección-por-región)
  - [:clamp — Limitación Numérica](#clamp--limitación-numérica)
  - [:round — Redondeo](#round--redondeo)
  - [:map — Tabla de Búsqueda](#map--tabla-de-búsqueda)
  - [:format — Formateo de Números](#format--formateo-de-números)
  - [:fallback — Ruta de Archivo con Respaldo](#fallback--ruta-de-archivo-con-respaldo)
  - [:once — Generar y Persistir](#once--generar-y-persistir)
  - [:version — Comparación Semántica de Versiones](#version--comparación-semántica-de-versiones)
  - [:watch — Leer Archivo Externo](#watch--leer-archivo-externo)
- [Restricciones](#-restricciones)
- [Encadenamiento de Marcadores](#-encadenamiento-de-marcadores)
- [Ejemplos de Código](#-ejemplos-de-código)
- [Soporte de Editores](#-soporte-de-editores)
- [Arquitectura](#-arquitectura)
- [Enlaces](#-enlaces)

---

## 💡 Filosofía de Diseño

La configuración es la base de cada aplicación. Sin embargo, los formatos estándar de la industria — **JSON** y **YAML** — nunca fueron diseñados para esto:

| Problema | JSON | YAML | SYNX |
|---|:---:|:---:|:---:|
| Requiere comillas para strings/claves | ✓ | ✗ | ✗ |
| Error por coma final | ✗ | — | ✓ |
| Indentación sensible a espacios | — | ✗ (peligroso) | ✓ (seguro, 2 espacios) |
| Soporte de comentarios | ✗ | ✓ | ✓ |
| Variables de entorno | ✗ | ✗ | ✓ nativo |
| Valores calculados | ✗ | ✗ | ✓ nativo |
| Costo en tokens IA (110 claves) | ~3300 chars | ~2500 chars | **~2000 chars** |
| Legibilidad | Baja | Media | **Alta** |

SYNX se construye sobre tres principios:

1. **Sintaxis mínima** — clave, espacio, valor. Sin comillas, sin comas, sin llaves, sin dos puntos.
2. **Activo por naturaleza** — la configuración no es solo datos, es lógica. Variables de entorno, matemáticas, referencias, selección aleatoria y validación — todo integrado.
3. **Eficiente en tokens** — al enviar configuración a través de un LLM, cada carácter importa. SYNX ahorra 30–40% de tokens respecto a JSON.

> **SYNX no es un reemplazo de JSON. SYNX es lo que JSON debió haber sido.**

---

## 🎬 Demostración

### Escritura de datos — limpia y sencilla

Solo **clave**, **espacio**, **valor**. Sin comillas, sin comas, sin llaves:

<p align="center">
  <a href="https://aperturesyndicate.com/branding/gifs/synx/synx.gif" target="_blank">
    <img src="https://aperturesyndicate.com/branding/gifs/synx/synx.gif" alt="Escribir SYNX estático" width="720" />
  </a>
</p>

> **📺 [Ver demostración →](https://aperturesyndicate.com/branding/gifs/synx/synx.gif)**

### Modo `!active` — configuración con lógica

Agrega `!active` en la primera línea y tu configuración cobra vida — funciones integradas directamente en el formato:

<p align="center">
  <a href="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" target="_blank">
    <img src="https://aperturesyndicate.com/branding/gifs/synx/synx2.gif" alt="Escribir SYNX activo con marcadores" width="720" />
  </a>
</p>

> **📺 [Ver demostración →](https://aperturesyndicate.com/branding/gifs/synx/synx2.gif)**

---

## ⚙ Cómo Funciona

El pipeline de SYNX tiene **dos etapas** — esta separación es clave para el rendimiento:

```
┌───────────────┐         ┌─────────────┐         ┌──────────────┐
│  Archivo .synx│ ──────► │   Parser   │ ──────► │    Salida    │
│  (texto)      │         │ (siempre)  │         │ (objeto JS)  │
└───────────────┘         └──────┬──────┘         └──────────────┘
                                 │
                          ¿tiene !active?
                                 │
                            ┌────▼────┐
                            │  Motor  │
                            │(ejecuta │
                            │marcado- │
                            │  res)   │
                            └─────────┘
```

### Etapa 1 — Parser

El **parser** lee el texto crudo y construye el árbol de clave-valor. Maneja pares clave-valor, anidación (indentación de 2 espacios), listas, conversión de tipos, comentarios y texto multilínea.

El parser registra los marcadores (`:env`, `:calc`, etc.) como **metadatos** adjuntos a cada clave, pero **no los ejecuta**. Esto significa que **agregar nuevos marcadores no ralentiza el parsing**.

### Etapa 2 — Motor (solo con `!active`)

Si el archivo comienza con `!active`, el **motor** recorre el árbol parseado y resuelve cada marcador.

**Los archivos sin `!active` nunca tocan el motor.**

---

## 📊 Rendimiento y Benchmarks

Todos los benchmarks son con datos reales, ejecutados sobre una configuración SYNX estándar de 110 claves (2.5 KB):

### Rust (criterion, llamada directa)

| Benchmark | Tiempo |
|---|---|
| `Synx::parse` (110 claves) | **~39 µs** |
| `parse_to_json` (110 claves) | **~42 µs** |
| `Synx::parse` (4 claves) | **~1.2 µs** |

### Node.js (50,000 iteraciones)

| Parser | µs/op | vs JSON | vs YAML |
|---|---:|---:|---:|
| `JSON.parse` (3.3 KB) | 6.08 µs | 1× | — |
| **`synx-js` TS puro** | **39.20 µs** | 6.4× | **2.1× más rápido que YAML** |
| `js-yaml` (2.5 KB) | 82.85 µs | 13.6× | 1× |

### Python (10,000 iteraciones)

| Parser | µs/op | vs YAML |
|---|---:|---:|
| `json.loads` (3.3 KB) | 13.04 µs | — |
| **`synx_native.parse`** | **55.44 µs** | **67× más rápido que YAML** |
| `yaml.safe_load` (2.5 KB) | 3,698 µs | 1× |

> En Python, SYNX parsea **67 veces** más rápido que YAML.

---

## 📦 Instalación

### Node.js / Navegador

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

### Extensión VS Code

Busca **"SYNX"** en el panel de extensiones, o:

```bash
code --install-extension APERTURESyndicate.synx-vscode
```

---

## 📝 Referencia de Sintaxis

### Sintaxis Básica

Regla fundamental: **clave** `(espacio)` **valor**.

```synx
name John
age 25
phrase ¡Me encanta programar!
empty_value
```

> Los números, booleanos (`true`/`false`) y `null` se detectan automáticamente. Todo lo demás es cadena.

---

### Anidación

La indentación crea jerarquía — **2 espacios** por nivel:

```synx
server
  host 0.0.0.0
  port 8080
  ssl
    enabled true
```

---

### Listas

Las líneas que comienzan con `- ` crean arreglos:

```synx
fruits
  - Apple
  - Banana
  - Cherry
```

---

### Conversión de Tipos

Usa `(tipo)` después del nombre de la clave para forzar el tipo:

```synx
zip_code(string) 90210
id(int) 007
ratio(float) 3
enabled(bool) 1
```

Tipos disponibles: `int`, `float`, `bool`, `string`.

#### Generación de Valores Aleatorios

Genera valores aleatorios al analizar usando `(random)`:

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

Tipos disponibles: `(random)` (int), `(random:int)`, `(random:float)`, `(random:bool)`.

> Los valores se generan en cada análisis — cada llamada produce valores diferentes.

---

### Texto Multilínea

Usa el operador `|`:

```synx
description |
  Esta es una descripción larga
  que abarca múltiples líneas.
```

---

### Comentarios

```synx
# Comentario con almohadilla
// Comentario con barras
name John  # Comentario en línea

###
Esto es un comentario de bloque.
Todo entre ### se ignora.
###
```

En la extensión de VSCode, se admite formato en comentarios:
- `*cursiva*` — verde
- `**negrita**` — morado
- `***negrita+cursiva***` — dorado
- `` `código` `` — naranja con fondo

---

## 🔥 Modo Activo (`!active`)

Coloca `!active` en la **primera línea** para desbloquear marcadores y restricciones.

```synx
!active

port:env PORT
boss_hp:calc base_hp * 5
```

---

## 🔐 Modo Bloqueado (`!lock`)

Agrega `!lock` para evitar que el código externo modifique valores mediante `Synx.set()`, `Synx.add()`, `Synx.remove()`. Los marcadores internos de SYNX siguen funcionando normalmente.

```synx
!active
!lock

max_players 100
greeting:random
  - ¡Hola!
  - ¡Bienvenido!
```

```typescript
const config = Synx.loadSync('./config.synx');

Synx.set(config, 'max_players', 200);
// ❌ error: "SYNX: Cannot set "max_players" — config is locked (!lock)"

console.log(config.max_players); // ✅ 100 (la lectura siempre está permitida)
```

Usa `Synx.isLocked(config)` para verificar el estado.

---

## 🧹 Formato Canónico (`format`)

`Synx.format()` reescribe cualquier archivo `.synx` en una forma única y normalizada.

**Qué hace:**
- **Ordena todas las claves alfabéticamente** en cada nivel de anidamiento
- **Normaliza la indentación** a exactamente 2 espacios por nivel
- **Elimina comentarios** — el formato canónico contiene solo datos
- **Una línea en blanco** entre bloques de nivel superior (objetos y listas)
- **Conserva las directivas** (`!active`, `!lock`) al inicio del archivo
- **El orden de los elementos de lista se preserva** — solo se ordenan las claves con nombre

### Por qué es importante para Git

Sin formato canónico, dos programadores escriben la misma configuración de forma diferente:

```synx
# Programador A              # Programador B
server                       server
    port 8080                  host 0.0.0.0
    host 0.0.0.0               port 8080
```

`git diff` muestra el bloque completo como modificado — aunque los datos son idénticos.

Después de `Synx.format()`, ambos producen:

```synx
server
  host 0.0.0.0
  port 8080
```

Una forma canónica. Cero ruido en los diffs.

### Uso

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

## 🧩 Referencia Completa de Marcadores

SYNX v3.0 proporciona **20 marcadores**. Cada marcador es una función que se adjunta a una clave mediante la sintaxis `:marcador`.

### `:env` — Variables de Entorno

```synx
!active
port:env PORT
port:env:default:8080 PORT
```

### `:default` — Valor por Defecto

```synx
!active
theme:default dark
```

### `:calc` — Expresiones Aritméticas

```synx
!active
base_price 100
tax:calc base_price * 0.2
total:calc base_price + tax
```

Operadores: `+` `-` `*` `/` `%` `(` `)`

### `:random` — Selección Aleatoria

```synx
!active
loot:random 70 20 10
  - common
  - rare
  - legendary
```

### `:alias` — Referencia a Otra Clave

```synx
!active
admin_email alex@example.com
billing:alias admin_email
```

### `:secret` — Valor Oculto

```synx
!active
api_key:secret sk-1234567890
```

### `:template` — Interpolación de Cadenas

```synx
!active
name John
greeting:template ¡Hola, {name}!
```

### `:include` — Importar Archivo Externo

```synx
!active
database:include ./db.synx
```

### `:unique` — Eliminar Duplicados

```synx
!active
tags:unique
  - action
  - rpg
  - action
```

Resultado: `["action", "rpg"]`

### `:split` — Cadena a Arreglo

```synx
!active
colors:split red, green, blue
words:split:space hello world foo
```

Palabras clave de separador: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`, `slash`

### `:join` — Arreglo a Cadena

Palabras clave de separador: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`, `slash`. Valor predeterminado: coma.

```synx
!active
path:join:slash
  - home
  - user
  - docs
```

Resultado: `"home/user/docs"`

### `:geo` — Selección por Región

```synx
!active
currency:geo
  - US USD
  - EU EUR
  - MX MXN
```

### `:clamp` — Limitación Numérica

```synx
!active
volume:clamp:0:100 150
```

Resultado: `100`

### `:round` — Redondeo

```synx
!active
price:round:2 19.999
profit:calc:round:2 revenue * 0.337
```

### `:map` — Tabla de Búsqueda

```synx
!active
status_code 1
status:map:status_code
  - 0 desconectado
  - 1 en línea
  - 2 ausente
```

Resultado: `"en línea"`

### `:format` — Formateo de Números

```synx
!active
price:format:%.2f 1234.5
id:format:%06d 42
```

Resultado: `"1234.50"`, `"000042"`

### `:fallback` — Ruta de Archivo con Respaldo

```synx
!active
icon:fallback:./default.png ./custom.png
```

### `:once` — Generar y Persistir

```synx
!active
session_id:once uuid
app_seed:once random
build_time:once timestamp
```

Tipos de generación: `uuid` (por defecto), `random`, `timestamp`

### `:version` — Comparación Semántica de Versiones

```synx
!active
runtime:version:>=:18.0 20.11.0
```

Resultado: `true`. Operadores: `>=` `<=` `>` `<` `==` `!=`

### `:watch` — Leer Archivo Externo

```synx
!active
app_name:watch:name ./package.json
config:watch ./data.txt
```

---

## 🔒 Restricciones

Las restricciones validan valores durante el parsing. Se definen en `[corchetes]` después del nombre de clave.

| Restricción | Sintaxis | Descripción |
|---|---|---|
| `required` | `key[required]` | Debe tener un valor |
| `readonly` | `key[readonly]` | Solo lectura |
| `min:N` | `key[min:3]` | Longitud/valor mínimo |
| `max:N` | `key[max:100]` | Longitud/valor máximo |
| `type:T` | `key[type:int]` | Forzar tipo |
| `pattern:R` | `key[pattern:^\d+$]` | Validar con regex |
| `enum:A\|B` | `key[enum:light\|dark]` | Valores permitidos |

```synx
!active
app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
```

---

## 🔗 Encadenamiento de Marcadores

```synx
!active
port:env:default:8080 PORT
profit:calc:round:2 revenue * margin
```

### ✅ Compatibilidad de Marcadores

Combinaciones que funcionan bien:

- `env:default`
- `calc:round`
- `split:unique`
- `split:join` (con un arreglo intermedio)

Limitaciones importantes:

- Se requiere `!active`, de lo contrario los marcadores no se resuelven.
- Algunos marcadores dependen del tipo: `split` espera string, `join` espera arreglo, `round`/`clamp` esperan números.
- Los argumentos se leen a la derecha en la cadena (por ejemplo `clamp:min:max`, `round:n`, `map:key`).
- Si un marcador anterior cambia el tipo, el siguiente puede dejar de aplicar.

---

## � Herramienta CLI

> Añadido en v3.1.3.

Instalación global via npm:

```bash
npm install -g @aperturesyndicate/synx
```

### `synx convert` — Exportar a otros formatos

```bash
# SYNX → JSON
synx convert config.synx --format json

# SYNX → YAML (para Helm, Ansible, K8s)
synx convert config.synx --format yaml > values.yaml

# SYNX → TOML
synx convert config.synx --format toml

# SYNX → .env (para Docker Compose)
synx convert config.synx --format env > .env

# Con modo estricto (error ante cualquier problema de marcador)
synx convert config.synx --format json --strict
```

### `synx validate` — Validación CI/CD

```bash
synx validate config.synx --strict
# Código de salida 0 en éxito, 1 en INCLUDE_ERR / WATCH_ERR / CALC_ERR / CONSTRAINT_ERR
```

### `synx watch` — Recarga en vivo

```bash
# Imprimir JSON en cada cambio
synx watch config.synx --format json

# Ejecutar un comando en cada cambio (ej. recargar Nginx)
synx watch config.synx --exec "nginx -s reload"
```

### `synx schema` — Extraer JSON Schema de restricciones

```bash
synx schema config.synx
# Genera JSON Schema basado en [required, min:N, max:N, type:T, enum:A|B, pattern:R]
```

---

## 📤 Formatos de exportación (API JS/TS)

> Añadido en v3.1.3.

Convertir un objeto SYNX parseado a JSON, YAML, TOML o .env:

```typescript
import Synx from '@aperturesyndicate/synx';

const config = Synx.loadSync('config.synx');

// JSON
const json = Synx.toJSON(config);          // formateado
const compact = Synx.toJSON(config, false); // compacto

// YAML
const yaml = Synx.toYAML(config);

// TOML
const toml = Synx.toTOML(config);

// .env (formato KEY=VALUE)
const env = Synx.toEnv(config);            // APP_NAME=TotalWario
const prefixed = Synx.toEnv(config, 'APP'); // APP_APP_NAME=TotalWario
```

---

## 📋 Exportación de esquema

> Añadido en v3.1.3.

Extraer restricciones SYNX como objeto JSON Schema:

```typescript
const schema = Synx.schema(`
!active
app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
`);
```

Resultado:

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

## 👁 Observador de archivos

> Añadido en v3.1.3.

Vigile un archivo `.synx` y obtenga la configuración actualizada en cada cambio:

```typescript
const handle = Synx.watch('config.synx', (config, error) => {
  if (error) {
    console.error('Error al recargar configuración:', error.message);
    return;
  }
  console.log('Configuración actualizada:', config.server.port);
}, { strict: true });

// Detener observación
handle.close();
```

---

## 🐳 Guía de despliegue

> Añadido en v3.1.3.

### Docker + Docker Compose

SYNX sirve como **fuente única de verdad** para toda la configuración de servicios. Los servicios que necesitan su propio formato (Nginx, Redis, etc.) reciben configuraciones generadas al inicio.

**Patrón:**

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   config.synx   │────▶│  script inicio  │────▶│  nginx.conf     │
│  (un archivo)   │     │  o CLI convert  │     │  .env           │
│  :env :default  │     │                 │     │  redis.conf     │
│  :template      │     │                 │     │  ajustes app    │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

**Paso 1 — Escriba su configuración:**

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

**Paso 2 — Generar .env para Docker Compose:**

```bash
synx convert config.synx --format env > .env
```

**Paso 3 — Usar en docker-compose.yml:**

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

### Generación de configuración Nginx

Use una plantilla + script de inicio para generar `nginx.conf` desde SYNX:

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

### Conexión Redis

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

### Conexión PostgreSQL

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

K8s monta secretos como archivos en `/run/secrets/`. Use `:watch` para leerlos:

```synx
!active

db_password:watch /run/secrets/db-password
api_key:watch /run/secrets/api-key
```

Docker Secrets funciona de manera idéntica — montados en `/run/secrets/`.

### HashiCorp Vault

Use Vault Agent para escribir secretos en archivos, luego léalos con `:watch`:

```synx
!active

db_creds:watch:password /vault/secrets/database
api_key:watch:key /vault/secrets/api-key
```

O inyecte via variables de entorno usando `env_template` de Vault Agent:

```synx
!active

db_password:env VAULT_DB_PASSWORD
api_key:env VAULT_API_KEY
```

### Helm / Kubernetes

Convertir SYNX a YAML para valores Helm:

```bash
synx convert config.synx --format yaml > helm/values.yaml
helm upgrade my-release ./chart -f helm/values.yaml
```

### Terraform

Terraform acepta archivos de variables JSON:

```bash
synx convert config.synx --format json > terraform.tfvars.json
terraform apply -var-file=terraform.tfvars.json
```

### Validación en pipeline CI/CD

Añada a su pipeline CI para verificar configuraciones antes del despliegue:

```yaml
# Ejemplo GitHub Actions
- name: Validar configuración SYNX
  run: npx @aperturesyndicate/synx validate config.synx --strict
```

---

## �💻 Ejemplos de Código

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

**Manipulación en tiempo de ejecución (set / add / remove):**

```typescript
import { Synx } from '@aperturesyndicate/synx';

const config = Synx.loadSync('./game.synx');

// Establecer un valor
Synx.set(config, 'max_players', 100);
Synx.set(config, 'server.host', 'localhost');

// Obtener un valor
const port = Synx.get(config, 'server.port'); // 8080

// Agregar a una lista
Synx.add(config, 'maps', 'Arena of Doom');

// Eliminar de una lista
Synx.remove(config, 'maps', 'Arena of Doom');

// Eliminar una clave completa
Synx.remove(config, 'deprecated_key');

// Verificar bloqueo
if (!Synx.isLocked(config)) {
  Synx.set(config, 'motd', '¡Bienvenido!');
}
```

> **Nota:** Si el archivo `.synx` tiene `!lock`, todas las llamadas `set`/`add`/`remove` lanzarán un error.

**Métodos de acceso (API JS/TS):**

- `Synx.get(obj, keyPath)` — leer un valor por ruta con puntos.
- `Synx.set(obj, keyPath, value)` — establecer un valor por ruta con puntos.
- `Synx.add(obj, keyPath, item)` — agregar un elemento a un arreglo.
- `Synx.remove(obj, keyPath, item?)` — quitar elemento de arreglo o borrar una clave.
- `Synx.isLocked(obj)` — comprobar si el config está bloqueado por `!lock`.

### Python

Actualmente `synx_native` exporta: `parse`, `parse_active`, `parse_to_json`.

Equivalentes en Python para `get`/`set`/`add`/`remove`:

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

# Uso de helpers de acceso en Python
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

## 🛠 Soporte de Editores

### Visual Studio Code

Soporte completo del lenguaje: resaltado de sintaxis, IntelliSense (20 marcadores), diagnósticos en tiempo real (15 verificaciones), ir a definición, formateo, vista previa de colores, sugerencias inline de `:calc`, vista previa JSON en vivo.

### Visual Studio 2022

Extensión MEF: resaltado de sintaxis, IntelliSense, marcado de errores, plegado de código, comandos de conversión.

---

## 🏗 Arquitectura

```
synx-format/
├── crates/synx-core/          # Núcleo Rust — parser + motor
├── bindings/
│   ├── node/                  # NAPI-RS → módulo nativo npm
│   └── python/                # PyO3 → módulo nativo PyPI
├── packages/
│   ├── synx-js/               # Parser + motor TypeScript puro
│   ├── synx-vscode/           # Extensión VS Code
│   └── synx-visualstudio/     # Extensión Visual Studio 2022
├── publish-npm.bat
├── publish-pypi.bat
└── publish-crates.bat
```

---

## 🔗 Enlaces

| Recurso | URL |
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
