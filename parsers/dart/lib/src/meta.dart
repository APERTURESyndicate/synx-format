// SYNX metadata + parse-result types. Mirrors Rust value.rs.

import 'value.dart';

/// Constraints from `[min:3, max:30, required, type:int, pattern:..., enum:a|b, readonly]`.
class SynxConstraints {
  double? min;
  double? max;
  String? typeName;
  bool required = false;
  bool readonly = false;
  String? pattern;
  List<String>? enumValues;

  SynxConstraints();

  bool get hasAny =>
      min != null ||
      max != null ||
      typeName != null ||
      required ||
      readonly ||
      pattern != null ||
      enumValues != null;
}

/// Marker / args / type-hint / constraints bundle attached to one active-mode field.
class SynxMeta {
  List<String> markers = [];

  /// One arg per marker (same length as `markers`).
  List<String> args = [];
  String? typeHint;
  SynxConstraints? constraints;

  SynxMeta();

  bool hasMarker(String name) => markers.contains(name);

  /// Index of marker `name` in the chain, or -1.
  int markerIndex(String name) {
    for (var i = 0; i < markers.length; i++) {
      if (markers[i] == name) return i;
    }
    return -1;
  }
}

typedef SynxMetaMap = Map<String, SynxMeta>;
typedef SynxMetadataTree = Map<String, SynxMetaMap>;

class SynxIncludeDirective {
  final String path;
  final String alias;
  const SynxIncludeDirective(this.path, this.alias);
}

class SynxUseDirective {
  final String pkg;
  final String alias;
  const SynxUseDirective(this.pkg, this.alias);
}

class SynxParseResult {
  SynxValue root;
  SynxMode mode;
  bool locked;
  bool tool;
  bool schema;
  bool llm;
  SynxMetadataTree metadata;
  List<SynxIncludeDirective> includes;
  List<SynxUseDirective> uses;

  SynxParseResult({
    SynxValue? root,
    this.mode = SynxMode.static_,
    this.locked = false,
    this.tool = false,
    this.schema = false,
    this.llm = false,
    SynxMetadataTree? metadata,
    List<SynxIncludeDirective>? includes,
    List<SynxUseDirective>? uses,
  })  : root = root ?? synxObject(),
        metadata = metadata ?? {},
        includes = includes ?? [],
        uses = uses ?? [];
}
