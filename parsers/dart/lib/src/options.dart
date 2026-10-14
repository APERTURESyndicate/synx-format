// Resolver options + user-marker function type.

import 'value.dart';

/// User-supplied custom marker.
///
/// Signature: (key, args, currentValue) → resolvedValue. Builtin markers
/// always win over a custom marker with the same name.
typedef SynxMarkerFn = SynxValue Function(
    String key, List<String> args, SynxValue value);

class SynxOptions {
  /// Variables `:env` reads. null means the process environment; a non-null
  /// map (even an empty one) is the complete set of variables.
  Map<String, String>? env;
  String? region;
  String? lang;

  /// Base directory for `:include` / `:use` lookups. Defaults to `.` at resolve time.
  String? basePath;

  /// Defaults to 16 when null.
  int? maxIncludeDepth;

  /// Defaults to `./synx_packages` when null.
  String? packagesPath;

  bool strict = false;
  Map<String, SynxMarkerFn> markerFns = {};

  /// Internal include-recursion counter — do not set manually.
  int includeDepth = 0;

  SynxOptions();
}
