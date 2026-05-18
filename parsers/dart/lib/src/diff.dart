// Structural diff. Mirrors crates/synx-core/src/diff.rs.

import 'value.dart';

class SynxDiffChange {
  final SynxValue from;
  final SynxValue to;
  const SynxDiffChange(this.from, this.to);
}

class SynxDiffEntry {
  final String key;
  final SynxDiffChange change;
  const SynxDiffEntry(this.key, this.change);
}

class SynxDiffResult {
  final SynxObject added;
  final SynxObject removed;
  final List<SynxDiffEntry> changed;
  final List<String> unchanged;
  const SynxDiffResult(this.added, this.removed, this.changed, this.unchanged);
}

SynxDiffResult diff(SynxObject a, SynxObject b) {
  final added = SynxObject();
  final removed = SynxObject();
  final changed = <SynxDiffEntry>[];
  final unchanged = <String>[];

  for (final entry in a.entries) {
    final bv = b[entry.key];
    if (bv == null && !b.contains(entry.key)) {
      removed.set(entry.key, entry.value);
    } else if (entry.value == bv) {
      unchanged.add(entry.key);
    } else {
      changed.add(SynxDiffEntry(entry.key, SynxDiffChange(entry.value, bv!)));
    }
  }
  for (final entry in b.entries) {
    if (a[entry.key] == null && !a.contains(entry.key)) {
      added.set(entry.key, entry.value);
    }
  }
  unchanged.sort();
  return SynxDiffResult(added, removed, changed, unchanged);
}

SynxValue diffToValue(SynxDiffResult d) {
  final root = SynxObject();
  root.set('added', synxObject(d.added));
  root.set('removed', synxObject(d.removed));

  final changed = SynxObject();
  for (final e in d.changed) {
    final inner = SynxObject();
    inner.set('from', e.change.from);
    inner.set('to', e.change.to);
    changed.set(e.key, synxObject(inner));
  }
  root.set('changed', synxObject(changed));

  final arr = d.unchanged.map((s) => synxString(s)).toList();
  root.set('unchanged', synxArray(arr));
  return synxObject(root);
}
