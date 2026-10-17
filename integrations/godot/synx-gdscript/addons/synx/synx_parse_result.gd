@tool
class_name SynxParseResult
extends RefCounted

# Full parse result, matching synx-core::value::ParseResult.

enum Mode { STATIC, ACTIVE }

var root: SynxValue = SynxValue.make_object({})
var mode: int = Mode.STATIC
var locked: bool = false
var tool_directive: bool = false
var schema: bool = false
var llm: bool = false

# synx-core's dot-path view of the declarations:
#   Dictionary[String /* path */, Dictionary[String /* key */, SynxMeta]]
# Declarations are stored on the objects of `root` (`SynxValue.meta`); this
# property reads them out, and assigning it attaches a path map to the tree
# (what `.synxb` decompile does).
var metadata: Dictionary:
	get:
		return SynxMeta.collect_paths(root)
	set(value):
		SynxMeta.attach_paths(root, value)

# includes: Array[SynxParseResult.IncludeDirective]
var includes: Array = []

# uses: Array[SynxParseResult.UseDirective]
var uses: Array = []


class IncludeDirective extends RefCounted:
	var path: String
	var alias: String

	func _init(p: String = "", a: String = "") -> void:
		path = p
		alias = a


class UseDirective extends RefCounted:
	var package: String
	var alias: String

	func _init(p: String = "", a: String = "") -> void:
		package = p
		alias = a
