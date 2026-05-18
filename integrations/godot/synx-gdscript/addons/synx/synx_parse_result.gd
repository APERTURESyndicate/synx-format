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

# metadata: Dictionary[String /* dot-path */, Dictionary[String /* key */, SynxMeta]]
var metadata: Dictionary = {}

# includes: Array[SynxIncludeDirective]
var includes: Array = []

# uses: Array[SynxUseDirective]
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
