@tool
class_name SynxRng
extends RefCounted

# Deterministic-friendly RNG façade — matches synx-core::rng surface.
# Uses Godot's built-in RandomNumberGenerator so seeds can be set for tests.

static var _rng: RandomNumberGenerator = null

static func _get() -> RandomNumberGenerator:
	if _rng == null:
		_rng = RandomNumberGenerator.new()
		_rng.randomize()
	return _rng

static func set_seed(s: int) -> void:
	var r := _get()
	r.seed = s

static func random_i64() -> int:
	# Godot RandomNumberGenerator.randi() is uint32; combine two for a 63-bit-safe int.
	var hi := int(_get().randi())
	var lo := int(_get().randi())
	# Mask to 62 bits to avoid sign issues — large enough for game-config use.
	return ((hi << 31) ^ lo) & 0x3FFF_FFFF_FFFF_FFFF

static func random_f64_01() -> float:
	return _get().randf()

static func random_bool() -> bool:
	return _get().randf() < 0.5

static func random_usize(bound: int) -> int:
	if bound <= 0:
		return 0
	return _get().randi() % bound

static func generate_uuid() -> String:
	# RFC 4122 v4-like — no native uuid in Godot, fabricate from rng bytes.
	var bytes := PackedByteArray()
	bytes.resize(16)
	for i in 16:
		bytes[i] = _get().randi() & 0xff
	# Version (4) and variant (10xx) bits.
	bytes[6] = (bytes[6] & 0x0f) | 0x40
	bytes[8] = (bytes[8] & 0x3f) | 0x80
	var hex_chars := "0123456789abcdef"
	var s := ""
	for i in 16:
		var b := bytes[i]
		s += hex_chars[(b >> 4) & 0xf]
		s += hex_chars[b & 0xf]
		if i == 3 or i == 5 or i == 7 or i == 9:
			s += "-"
	return s
