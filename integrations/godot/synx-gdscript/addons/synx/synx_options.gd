@tool
class_name SynxOptions
extends RefCounted

# Options for active-mode resolution. Mirrors synx-core::value::Options.

# Override environment variables; null → fall back to OS.get_environment.
var env: Variant = null

# Region code for :geo (e.g. "RU", "US"). Empty string → default "US".
var region: String = ""

# Language code for :i18n (e.g. "en", "ru"). Empty string → default "en".
var lang: String = ""

# Base directory for :include / :watch path resolution.
# Empty string → use ProjectSettings root ("res://") with FileAccess.
var base_path: String = ""

# Maximum :include / :import / :watch nesting depth.
var max_include_depth: int = 16

# Path to packages directory (relative to base_path). Empty → "./synx_packages".
var packages_path: String = ""

# Strict mode: throw GDScript push_error on *_ERR strings produced by markers.
var strict: bool = false

# Custom GDScript callables registered as markers — replacement for WASM tool markers.
# Format: Dictionary[String /* marker name */, Callable].
# Callable signature: func(value: SynxValue, args: PackedStringArray) -> SynxValue
var marker_callables: Dictionary = {}

# Internal: incremented when resolve() recurses into included files.
var _include_depth: int = 0

func clone() -> SynxOptions:
	var o := SynxOptions.new()
	o.env = env.duplicate() if typeof(env) == TYPE_DICTIONARY else env
	o.region = region
	o.lang = lang
	o.base_path = base_path
	o.max_include_depth = max_include_depth
	o.packages_path = packages_path
	o.strict = strict
	o.marker_callables = marker_callables.duplicate()
	o._include_depth = _include_depth
	return o
