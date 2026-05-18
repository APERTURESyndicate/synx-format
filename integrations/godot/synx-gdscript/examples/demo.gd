extends Node

# Minimal demo: parse a static SYNX block, then an active block, and print results.
# Run this scene to see SYNX in action.

func _ready() -> void:
	_demo_static()
	_demo_active()
	_demo_tool()
	_demo_binary()


func _demo_static() -> void:
	var text := """
name Alice
age 30
server
  host 0.0.0.0
  port 8080
inventory
  - Sword
  - Shield
  - Potion
"""
	var d := Synx.parse_to_variant(text)
	print("\n── static ──")
	print(JSON.stringify(d, "  "))


func _demo_active() -> void:
	var text := """
!active
base_hp 100
boss_hp:calc base_hp * 5
host:env:default:0.0.0.0 HOST
port[min:1, max:65535]:env:default:3000 PORT
greeting Hello, {name}!
name Alice
"""
	var opts := SynxOptions.new()
	opts.env = { "PORT": "8080" }
	var d := Synx.parse_active_to_variant(text, opts)
	print("\n── active ──")
	print(JSON.stringify(d, "  "))


func _demo_tool() -> void:
	var text := """
!tool
web_search
  query Godot 4 docs
  lang en
"""
	var d := Synx.parse_tool(text)
	# Unwrap to plain dictionary for printing.
	var unwrapped: Dictionary = {}
	for k in d.keys():
		var v: SynxValue = d[k]
		unwrapped[k] = v.to_variant()
	print("\n── tool ──")
	print(JSON.stringify(unwrapped, "  "))


func _demo_binary() -> void:
	var text := "name Test\nport 8080\nactive true\n"
	var bin := Synx.compile(text, false)
	print("\n── binary ──")
	print("compiled %d bytes" % bin.size())
	var roundtrip := Synx.decompile(bin)
	if roundtrip["ok"]:
		print("decompiled:\n" + String(roundtrip["text"]))
	else:
		print("decompile error: " + String(roundtrip["error"]))
