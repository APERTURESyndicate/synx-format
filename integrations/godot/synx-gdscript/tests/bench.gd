extends SceneTree

# Timings over a directory of .synx documents:
#
#   godot --headless --path . -s tests/bench.gd -- <dir> [runs] [all]
#
# Prints the best of `runs` (default 7) per document: parse (an `!active`
# document includes resolution) and the JSON projection; with `all`, also
# `.synxb` compile and decompile, stringify and format.


func _best(runs: int, f: Callable) -> float:
	var best := INF
	for r in runs:
		var t0 := Time.get_ticks_usec()
		f.call()
		best = minf(best, float(Time.get_ticks_usec() - t0))
	return best / 1000.0


func _init() -> void:
	var args := OS.get_cmdline_user_args()
	if args.is_empty():
		print("usage: bench.gd -- <dir> [runs] [all]")
		quit(2)
		return
	var dir_path: String = args[0]
	var runs := 7
	if args.size() > 1:
		runs = int(args[1])
	var all := args.size() > 2 and args[2] == "all"
	var files := DirAccess.get_files_at(dir_path)
	files.sort()
	var totals := PackedFloat64Array([0, 0, 0, 0, 0, 0])
	var count := 0
	for f in files:
		if not f.ends_with(".synx"):
			continue
		var text := FileAccess.get_file_as_string(dir_path.path_join(f))
		var t := PackedFloat64Array([0, 0, 0, 0, 0, 0])
		t[0] = _best(runs, func() -> void:
			var res := SynxParser.parse(text)
			if res.mode == SynxParseResult.Mode.ACTIVE:
				SynxEngine.resolve(res, SynxOptions.new()))
		var resolved := SynxParser.parse(text)
		if resolved.mode == SynxParseResult.Mode.ACTIVE:
			SynxEngine.resolve(resolved, SynxOptions.new())
		t[1] = _best(runs, func() -> void: SynxJson.to_json(resolved.root))
		var line := "%-22s %9d B   parse %9.3f ms   json %8.3f ms" % [f, text.to_utf8_buffer().size(), t[0], t[1]]
		if all:
			var parsed := SynxParser.parse(text)
			var bin := SynxBinary.compile(parsed, false)
			t[2] = _best(runs, func() -> void: SynxBinary.compile(parsed, false))
			t[3] = _best(runs, func() -> void: SynxBinary.decompile(bin))
			t[4] = _best(runs, func() -> void: SynxStringify.stringify(parsed.root))
			t[5] = _best(runs, func() -> void: SynxFormatter.format(text))
			line += "   compile %8.3f ms   decompile %8.3f ms   stringify %8.3f ms   format %8.3f ms   synxb %d B" % [t[2], t[3], t[4], t[5], bin.size()]
		for i in 6:
			totals[i] += t[i]
		count += 1
		print(line)
	var tl := "%-22s %9d files parse %9.3f ms   json %8.3f ms" % ["total", count, totals[0], totals[1]]
	if all:
		tl += "   compile %8.3f ms   decompile %8.3f ms   stringify %8.3f ms   format %8.3f ms" % [totals[2], totals[3], totals[4], totals[5]]
	print(tl)
	quit(0)
