@tool
class_name SynxDiff
extends RefCounted

# Structural diff between two SYNX top-level objects.
# Mirrors synx-core::diff::diff and diff_to_value.
#
# Returns:
#   {
#     "added":     Dictionary[String, SynxValue],
#     "removed":   Dictionary[String, SynxValue],
#     "changed":   Dictionary[String, { from: SynxValue, to: SynxValue }],
#     "unchanged": Array[String]   (sorted)
#   }

static func diff(a: Dictionary, b: Dictionary) -> Dictionary:
	var added: Dictionary = {}
	var removed: Dictionary = {}
	var changed: Dictionary = {}
	var unchanged: Array[String] = []

	for key in a.keys():
		var av: SynxValue = a[key]
		if not b.has(key):
			removed[key] = av
		else:
			var bv: SynxValue = b[key]
			if av.equals(bv):
				unchanged.append(String(key))
			else:
				changed[key] = {"from": av, "to": bv}

	for key in b.keys():
		if not a.has(key):
			added[key] = b[key]

	unchanged.sort()
	return {
		"added": added,
		"removed": removed,
		"changed": changed,
		"unchanged": unchanged,
	}


# Convert a diff result into a SynxValue tree suitable for `to_json`.
static func diff_to_value(d: Dictionary) -> SynxValue:
	var added: Dictionary = d["added"]
	var removed: Dictionary = d["removed"]
	var changed_in: Dictionary = d["changed"]
	var unchanged: Array = d["unchanged"]

	var added_v: Dictionary = {}
	for k in added.keys():
		added_v[k] = added[k]
	var removed_v: Dictionary = {}
	for k in removed.keys():
		removed_v[k] = removed[k]

	var changed_v: Dictionary = {}
	for k in changed_in.keys():
		var pair: Dictionary = changed_in[k]
		var entry: Dictionary = {}
		entry["from"] = pair["from"]
		entry["to"] = pair["to"]
		changed_v[k] = SynxValue.make_object(entry)

	var unchanged_arr: Array = []
	for s in unchanged:
		unchanged_arr.append(SynxValue.make_string(String(s)))

	var root: Dictionary = {}
	root["added"] = SynxValue.make_object(added_v)
	root["removed"] = SynxValue.make_object(removed_v)
	root["changed"] = SynxValue.make_object(changed_v)
	root["unchanged"] = SynxValue.make_array(unchanged_arr)
	return SynxValue.make_object(root)
