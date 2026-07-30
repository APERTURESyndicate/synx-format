# SYNX specification

The normative specification files live in the monorepo (not duplicated here for npm):

- English: [`docs/spec/SPECIFICATION_EN.md`](https://github.com/APERTURESyndicate/synx-format/blob/main/docs/spec/SPECIFICATION_EN.md)
- Russian: [`docs/spec/SPECIFICATION_RU.md`](https://github.com/APERTURESyndicate/synx-format/blob/main/docs/spec/SPECIFICATION_RU.md)
- Normative (SYNX 3.6, frozen): [`docs/spec/SYNX-3.6-NORMATIVE.md`](https://github.com/APERTURESyndicate/synx-format/blob/main/docs/spec/SYNX-3.6-NORMATIVE.md)
- Normative additions (SYNX 3.7): [`docs/spec/SYNX-3.7-NORMATIVE.md`](https://github.com/APERTURESyndicate/synx-format/blob/main/docs/spec/SYNX-3.7-NORMATIVE.md) — adds the `|+` indent-preserving multiline opener; all of 3.6 carries through unchanged.
- Normative (SYNXL 1): [`docs/spec/SYNXL-1-NORMATIVE.md`](https://github.com/APERTURESyndicate/synx-format/blob/main/docs/spec/SYNXL-1-NORMATIVE.md) — the `.synxl` record-stream format. Separate format, separate version axis; embeds SYNX 3.7 for block fields.

---

## SYNXL in this package

This package implements SYNXL format version 1 in pure TypeScript (`src/synxl.ts`). A SYNXL document is **not** a SYNX document — its root is a sequence of records, not an object — so `Synx.parse` will not read one. The entry points below are the SYNXL surface.

```synxl
!synxl 1
!fields id[type:int, required] ; score[type:float] ; messages[block]

1 ; 0.91
  messages
    - role user
      content Hello
```

### Reading

```typescript
import { Synx } from '@aperturesyndicate/synx-format';

const doc = Synx.parseSynxl(text);
doc.version;                     // 1
doc.records[0].values.id;        // 1
doc.records[0].values.messages;  // [{ role: 'user', content: 'Hello' }]
doc.records[0].index;            // 0-based position
doc.records[0].line;             // 1-based source line
doc.fieldLists;                  // every `!fields` line seen, in document order
doc.diagnostics;                 // SynxlDiagnostic[] — recoverable observations
```

### Streaming

```typescript
// Synchronous, over text already in memory
for (const record of Synx.streamSynxl(text)) console.log(record.values.id);

// Asynchronous, over a file or any async iterable of chunks
for await (const record of Synx.streamSynxlFile('chat.synxl')) console.log(record.values.id);
for await (const record of Synx.streamSynxlAsync(chunks)) console.log(record.values.id);
```

### Files, projections, writing

```typescript
Synx.loadSynxlSync('chat.synxl');          // SynxlDocument
await Synx.loadSynxl('chat.synxl');        // SynxlDocument

Synx.synxlToJSON(text);                    // canonical JSON array
Synx.synxlToNDJSON(text);                  // one canonical object per line

Synx.writeSynxl([{ id: 1, messages: [{ role: 'user', content: 'Hi' }] }]);
Synx.saveSynxlSync('out.synxl', records);
await Synx.saveSynxl('out.synxl', records);
```

`writeSynxl` accepts either a `SynxlDocument` or a plain array of objects. Quoting is applied automatically, and a value that cannot be written inline — one containing a newline, or one needing quotes while containing both quote characters — is promoted to a `[block]` field.

### Options and errors

```typescript
Synx.parseSynxl(text, { validate: true });   // enforce declared [constraints]; off by default
Synx.parseSynxl(text, { maxRecordBytes: 1 << 20, reportTruncatedTail: true });
```

Hard errors throw `SynxlError`, a subclass of the package's `SynxError`, carrying a machine-readable `condition` (`MissingOrMalformedPrologue`, `DuplicateFieldName`, `ZeroArityFieldList`, …) and the 1-based `line`. Everything recoverable — a wrong inline field count, a failed cast, an unknown block key — is reported as a diagnostic instead, on both the record and the document.

Named exports mirror the statics: `parseSynxl`, `streamSynxl`, `streamSynxlAsync`, `synxlToJSON`, `synxlToNDJSON`, `writeSynxl`, plus `SYNXL_VERSION` and the `MAX_SYNXL_*` resource limits.
