@tool
class_name SynxSafeCalc
extends RefCounted

# Safe arithmetic evaluator — parity with synx-core::calc::safe_calc.
# Grammar:
#   expr   → term (('+' | '-') term)*
#   term   → factor (('*' | '/' | '%') factor)*
#   factor → NUMBER | '(' expr ')'
#
# All variable substitution must happen before reaching this evaluator.
# Returns { "ok": bool, "value": float, "error": String }.

const TOK_NUMBER := 0
const TOK_OP := 1
const TOK_LPAREN := 2
const TOK_RPAREN := 3

static func evaluate(expr: String) -> Dictionary:
	var trimmed := expr.strip_edges()
	if trimmed.is_empty():
		return {"ok": true, "value": 0.0, "error": ""}
	var tok_result := _tokenize(trimmed)
	if not bool(tok_result["ok"]):
		return tok_result
	var tokens: Array = tok_result["tokens"]
	if tokens.is_empty():
		return {"ok": true, "value": 0.0, "error": ""}
	var state := {"tokens": tokens, "pos": 0}
	var val_res := _expr(state)
	if not bool(val_res["ok"]):
		return val_res
	if int(state["pos"]) < tokens.size():
		return {"ok": false, "value": 0.0, "error": "SYNX :calc — unexpected token at position %d" % int(state["pos"])}
	return val_res


static func _tokenize(expr: String) -> Dictionary:
	var tokens: Array = []
	var i := 0
	var n := expr.length()
	while i < n:
		var ch := expr.unicode_at(i)
		if ch == 32 or ch == 9:
			i += 1
			continue

		# Number — digit or '.' followed by digit, or '-' in prefix position.
		var is_digit := ch >= 48 and ch <= 57
		var is_dot_then_digit := ch == 46 and i + 1 < n and expr.unicode_at(i + 1) >= 48 and expr.unicode_at(i + 1) <= 57
		var prefix_minus := false
		if ch == 45:
			# Unary minus only when last token is operator/lparen/start.
			if tokens.is_empty():
				prefix_minus = true
			else:
				var last: Dictionary = tokens[tokens.size() - 1]
				var k := int(last["kind"])
				if k == TOK_OP or k == TOK_LPAREN:
					prefix_minus = true

		if is_digit or is_dot_then_digit or prefix_minus:
			var start := i
			if prefix_minus:
				i += 1
			while i < n:
				var c := expr.unicode_at(i)
				if (c >= 48 and c <= 57) or c == 46:
					i += 1
				else:
					break
			var num_str := expr.substr(start, i - start)
			if not num_str.is_valid_float():
				return {"ok": false, "value": 0.0, "error": "SYNX :calc — invalid number: '%s'" % num_str}
			tokens.append({"kind": TOK_NUMBER, "value": num_str.to_float()})
			continue

		if ch == 43 or ch == 45 or ch == 42 or ch == 47 or ch == 37:
			tokens.append({"kind": TOK_OP, "op": ch})
			i += 1
			continue

		if ch == 40:
			tokens.append({"kind": TOK_LPAREN})
			i += 1
			continue
		if ch == 41:
			tokens.append({"kind": TOK_RPAREN})
			i += 1
			continue

		return {"ok": false, "value": 0.0, "error": "SYNX :calc — unexpected character: '%s' in expression" % char(ch)}

	return {"ok": true, "tokens": tokens}


static func _expr(state: Dictionary) -> Dictionary:
	var left_res := _term(state)
	if not bool(left_res["ok"]):
		return left_res
	var left := float(left_res["value"])
	var tokens: Array = state["tokens"]
	while int(state["pos"]) < tokens.size():
		var t: Dictionary = tokens[int(state["pos"])]
		if int(t["kind"]) != TOK_OP:
			break
		var op := int(t["op"])
		if op == 43: # +
			state["pos"] = int(state["pos"]) + 1
			var r := _term(state)
			if not bool(r["ok"]):
				return r
			left += float(r["value"])
		elif op == 45: # -
			state["pos"] = int(state["pos"]) + 1
			var r2 := _term(state)
			if not bool(r2["ok"]):
				return r2
			left -= float(r2["value"])
		else:
			break
	return {"ok": true, "value": left, "error": ""}


static func _term(state: Dictionary) -> Dictionary:
	var left_res := _factor(state)
	if not bool(left_res["ok"]):
		return left_res
	var left := float(left_res["value"])
	var tokens: Array = state["tokens"]
	while int(state["pos"]) < tokens.size():
		var t: Dictionary = tokens[int(state["pos"])]
		if int(t["kind"]) != TOK_OP:
			break
		var op := int(t["op"])
		if op == 42: # *
			state["pos"] = int(state["pos"]) + 1
			var r := _factor(state)
			if not bool(r["ok"]):
				return r
			left *= float(r["value"])
		elif op == 47: # /
			state["pos"] = int(state["pos"]) + 1
			var r2 := _factor(state)
			if not bool(r2["ok"]):
				return r2
			var rv := float(r2["value"])
			if rv == 0.0:
				return {"ok": false, "value": 0.0, "error": "SYNX :calc — division by zero"}
			left /= rv
		elif op == 37: # %
			state["pos"] = int(state["pos"]) + 1
			var r3 := _factor(state)
			if not bool(r3["ok"]):
				return r3
			var rv3 := float(r3["value"])
			if rv3 == 0.0:
				return {"ok": false, "value": 0.0, "error": "SYNX :calc — division by zero"}
			left = fposmod(left, rv3) if left >= 0 else fmod(left, rv3)
		else:
			break
	return {"ok": true, "value": left, "error": ""}


static func _factor(state: Dictionary) -> Dictionary:
	var tokens: Array = state["tokens"]
	if int(state["pos"]) >= tokens.size():
		return {"ok": false, "value": 0.0, "error": "SYNX :calc — unexpected end of expression"}
	var t: Dictionary = tokens[int(state["pos"])]
	var kind := int(t["kind"])
	if kind == TOK_NUMBER:
		state["pos"] = int(state["pos"]) + 1
		return {"ok": true, "value": float(t["value"]), "error": ""}
	if kind == TOK_LPAREN:
		state["pos"] = int(state["pos"]) + 1
		var inner := _expr(state)
		if not bool(inner["ok"]):
			return inner
		if int(state["pos"]) >= tokens.size():
			return {"ok": false, "value": 0.0, "error": "SYNX :calc — missing closing parenthesis"}
		var t2: Dictionary = tokens[int(state["pos"])]
		if int(t2["kind"]) != TOK_RPAREN:
			return {"ok": false, "value": 0.0, "error": "SYNX :calc — missing closing parenthesis"}
		state["pos"] = int(state["pos"]) + 1
		return inner
	return {"ok": false, "value": 0.0, "error": "SYNX :calc — unexpected token"}
