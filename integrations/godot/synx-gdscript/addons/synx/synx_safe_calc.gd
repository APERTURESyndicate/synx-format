@tool
class_name SynxSafeCalc
extends RefCounted

# Safe arithmetic evaluator — synx-core `calc::safe_calc`, same grammar,
# same order of evaluation and the same error texts:
#   expr   → term (('+' | '-') term)*
#   term   → factor (('*' | '/' | '%') factor)*
#   factor → NUMBER | '(' expr ')'
#
# The recursive descent of the reference runs here on an explicit stack, so
# a deeply parenthesised expression cannot overflow the GDScript call stack.
# Variables must be substituted before the call.
# Returns { "ok": bool, "value": float, "error": String }.

const TOK_NUMBER := 0
const TOK_OP := 1
const TOK_LPAREN := 2
const TOK_RPAREN := 3


static func evaluate(expr: String) -> Dictionary:
	var trimmed := SynxText.trim(expr)
	if trimmed.is_empty():
		return {"ok": true, "value": 0.0, "error": ""}
	var kinds := PackedInt64Array()
	var nums := PackedFloat64Array()
	var err := _tokenize(trimmed, kinds, nums)
	if not err.is_empty():
		return {"ok": false, "value": 0.0, "error": err}
	if kinds.is_empty():
		return {"ok": true, "value": 0.0, "error": ""}
	return _parse(kinds, nums)


# Tokens: kinds[i] is TOK_*; nums[i] the number, or the operator's code.
static func _tokenize(expr: String, kinds: PackedInt64Array, nums: PackedFloat64Array) -> String:
	var n := expr.length()
	var i := 0
	while i < n:
		var ch := expr.unicode_at(i)
		if ch == 32 or ch == 9:
			i += 1
			continue
		var last := kinds[kinds.size() - 1] if kinds.size() > 0 else -1
		if (ch >= 48 and ch <= 57) \
				or (ch == 46 and i + 1 < n and expr.unicode_at(i + 1) >= 48 and expr.unicode_at(i + 1) <= 57) \
				or (ch == 45 and (last == -1 or last == TOK_OP or last == TOK_LPAREN)):
			var start := i
			if ch == 45:
				i += 1
			while i < n:
				var d := expr.unicode_at(i)
				if (d >= 48 and d <= 57) or d == 46:
					i += 1
				else:
					break
			var num_str := expr.substr(start, i - start)
			var v: Variant = SynxNum.parse_f64(num_str)
			if v == null:
				return "SYNX :calc — invalid number: '%s'" % num_str
			kinds.append(TOK_NUMBER)
			nums.append(v)
			continue
		if ch == 43 or ch == 45 or ch == 42 or ch == 47 or ch == 37:
			kinds.append(TOK_OP)
			nums.append(ch)
			i += 1
			continue
		if ch == 40:
			kinds.append(TOK_LPAREN)
			nums.append(0.0)
			i += 1
			continue
		if ch == 41:
			kinds.append(TOK_RPAREN)
			nums.append(0.0)
			i += 1
			continue
		return "SYNX :calc — unexpected character: '%s' in expression" % expr.substr(i, 1)
	return ""


static func _token_text(kind: int, num: float) -> String:
	match kind:
		TOK_NUMBER:
			return SynxNum.display(num)
		TOK_OP:
			return String.chr(int(num))
		TOK_LPAREN:
			return "("
	return ")"


static func _fail(msg: String) -> Dictionary:
	return {"ok": false, "value": 0.0, "error": msg}


# One context per open parenthesis (index 0 is the whole expression): the
# running sum of `expr`, its pending + / -, the running product of `term`
# and its pending * / % or /.
static func _parse(kinds: PackedInt64Array, nums: PackedFloat64Array) -> Dictionary:
	var count := kinds.size()
	var pos := 0
	var e_left := PackedFloat64Array([0.0])
	var e_op := PackedInt64Array([0])
	var t_left := PackedFloat64Array([0.0])
	var t_op := PackedInt64Array([0])
	var value := 0.0
	var need_factor := true
	while true:
		if need_factor:
			if pos >= count:
				return _fail("SYNX :calc — unexpected end of expression")
			var k := kinds[pos]
			if k == TOK_NUMBER:
				value = nums[pos]
				pos += 1
				need_factor = false
			elif k == TOK_LPAREN:
				pos += 1
				e_left.append(0.0)
				e_op.append(0)
				t_left.append(0.0)
				t_op.append(0)
			else:
				return _fail("SYNX :calc — unexpected token: '%s'" % _token_text(k, nums[pos]))
			continue
		var top := e_left.size() - 1
		# term: apply the pending operator to the factor just read
		var top_op := t_op[top]
		if top_op == 0:
			t_left[top] = value
		elif top_op == 42:
			t_left[top] = t_left[top] * value
		else:
			if value == 0.0:
				return _fail("SYNX :calc — division by zero")
			if top_op == 47:
				t_left[top] = t_left[top] / value
			else:
				t_left[top] = fmod(t_left[top], value)
		if pos < count and kinds[pos] == TOK_OP:
			var opc := int(nums[pos])
			if opc == 42 or opc == 47 or opc == 37:
				t_op[top] = opc
				pos += 1
				need_factor = true
				continue
		var term_value := t_left[top]
		t_op[top] = 0
		# expr: apply the pending + / - to the term just finished
		var eo := e_op[top]
		if eo == 0:
			e_left[top] = term_value
		elif eo == 43:
			e_left[top] = e_left[top] + term_value
		else:
			e_left[top] = e_left[top] - term_value
		if pos < count and kinds[pos] == TOK_OP:
			var opc2 := int(nums[pos])
			if opc2 == 43 or opc2 == 45:
				e_op[top] = opc2
				pos += 1
				need_factor = true
				continue
		var result := e_left[top]
		if top == 0:
			if pos < count:
				return _fail("SYNX :calc — unexpected token at position %d" % pos)
			return {"ok": true, "value": result, "error": ""}
		e_left.resize(top)
		e_op.resize(top)
		t_left.resize(top)
		t_op.resize(top)
		if pos >= count or kinds[pos] != TOK_RPAREN:
			return _fail("SYNX :calc — missing closing parenthesis")
		pos += 1
		value = result
	return _fail("SYNX :calc — unexpected end of expression")
