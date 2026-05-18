@tool
extends EditorPlugin

# The plugin is optional — all `Synx.*` static APIs are available from any
# script as soon as the addon's source files exist on disk, thanks to GDScript
# `class_name` global registration. Enabling the plugin only flips the editor
# bit so it shows up in Project → Plugins.

func _enter_tree() -> void:
	pass

func _exit_tree() -> void:
	pass
