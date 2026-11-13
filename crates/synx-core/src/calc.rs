//! SYNX Safe Calculator — evaluates arithmetic expressions without eval().
//! Supports: +, -, *, /, %, parentheses, integers, floats (including negatives).

#[derive(Debug, Clone)]
enum Token {
    Number(f64),
    Op(u8),     // b'+', b'-', b'*', b'/', b'%'
    LParen,
    RParen,
}

fn tokenize(expr: &str) -> Result<Vec<Token>, String> {
    let bytes = expr.as_bytes();
    let len = bytes.len();
    let mut tokens: Vec<Token> = Vec::with_capacity(16);
    let mut i = 0;

    while i < len {
        let ch = bytes[i];

        if ch == b' ' || ch == b'\t' {
            i += 1;
            continue;
        }

        // Number (including negative after operator/start/lparen)
        if (ch >= b'0' && ch <= b'9')
            || (ch == b'.'
                && i + 1 < len
                && bytes[i + 1] >= b'0'
                && bytes[i + 1] <= b'9')
            || (ch == b'-'
                && (tokens.is_empty()
                    || matches!(tokens.last(), Some(Token::Op(_)) | Some(Token::LParen))))
        {
            let start = i;
            if ch == b'-' {
                i += 1;
            }
            while i < len && ((bytes[i] >= b'0' && bytes[i] <= b'9') || bytes[i] == b'.') {
                i += 1;
            }
            let num_str = &expr[start..i];
            let val: f64 = num_str
                .parse()
                .map_err(|_| format!("SYNX :calc — invalid number: '{}'", num_str))?;
            tokens.push(Token::Number(val));
            continue;
        }

        if ch == b'+' || ch == b'-' || ch == b'*' || ch == b'/' || ch == b'%' {
            tokens.push(Token::Op(ch));
            i += 1;
            continue;
        }

        if ch == b'(' {
            tokens.push(Token::LParen);
            i += 1;
            continue;
        }
        if ch == b')' {
            tokens.push(Token::RParen);
            i += 1;
            continue;
        }

        // The whole character, not its first byte read as Latin-1 (`м` → `Ð`).
        let shown = expr[i..].chars().next().unwrap_or('\u{fffd}');
        return Err(format!(
            "SYNX :calc — unexpected character: '{}' in expression",
            shown
        ));
    }

    Ok(tokens)
}

/// A token as written, for error messages (identical in every implementation).
fn token_text(t: &Token) -> String {
    match t {
        Token::Number(n) => n.to_string(),
        Token::Op(b) => (*b as char).to_string(),
        Token::LParen => "(".to_string(),
        Token::RParen => ")".to_string(),
    }
}

/// One open `expr` of the grammar below: the top level, or the inside of a
/// pair of parentheses.
///
/// Grammar:
///   expr   → term (('+' | '-') term)*
///   term   → factor (('*' | '/' | '%') factor)*
///   factor → NUMBER | '(' expr ')'
struct Frame {
    /// Sum so far and the `+` / `-` waiting for its right-hand term.
    expr_left: f64,
    expr_op: Option<u8>,
    /// Product so far and the `*` / `/` / `%` waiting for its right-hand factor.
    term_left: f64,
    term_op: Option<u8>,
}

impl Frame {
    fn new() -> Self {
        Frame { expr_left: 0.0, expr_op: None, term_left: 0.0, term_op: None }
    }
}

/// Evaluate a token stream.
///
/// safety: parentheses used to recurse through `expr → term → factor`, one
/// group of stack frames per `(`. A stack overflow cannot be caught, so
/// `x:calc ((((…` took the whole process down — 2 040 parentheses on a 512 KiB
/// thread, 20 000 through `:ref:calc` on any thread. Open parentheses now live
/// in a `Vec`; the order of evaluation, and so every result and error, is the
/// one the recursive parser produced.
fn evaluate(tokens: &[Token]) -> Result<f64, String> {
    let mut stack: Vec<Frame> = vec![Frame::new()];
    let mut pos = 0usize;
    loop {
        // A factor is expected here.
        let mut value = match tokens.get(pos) {
            None => return Err("SYNX :calc — unexpected end of expression".into()),
            Some(Token::Number(v)) => {
                pos += 1;
                *v
            }
            Some(Token::LParen) => {
                pos += 1;
                stack.push(Frame::new());
                continue;
            }
            Some(other) => {
                return Err(format!("SYNX :calc — unexpected token: '{}'", token_text(other)));
            }
        };

        // Fold the factor into its term, the term into its expr, and close
        // every pair of parentheses that ends here.
        loop {
            let frame = stack.last_mut().expect("the top-level frame is never popped");
            match frame.term_op.take() {
                None => frame.term_left = value,
                Some(b'*') => frame.term_left *= value,
                Some(op) => {
                    if value == 0.0 {
                        return Err("SYNX :calc — division by zero".into());
                    }
                    if op == b'/' {
                        frame.term_left /= value;
                    } else {
                        frame.term_left %= value;
                    }
                }
            }
            if let Some(Token::Op(op @ (b'*' | b'/' | b'%'))) = tokens.get(pos) {
                frame.term_op = Some(*op);
                pos += 1;
                break;
            }

            match frame.expr_op.take() {
                None => frame.expr_left = frame.term_left,
                Some(b'+') => frame.expr_left += frame.term_left,
                Some(_) => frame.expr_left -= frame.term_left,
            }
            if let Some(Token::Op(op @ (b'+' | b'-'))) = tokens.get(pos) {
                frame.expr_op = Some(*op);
                pos += 1;
                break;
            }

            // This expr is complete.
            let result = frame.expr_left;
            if stack.len() == 1 {
                if pos < tokens.len() {
                    return Err(format!("SYNX :calc — unexpected token at position {}", pos));
                }
                return Ok(result);
            }
            stack.pop();
            match tokens.get(pos) {
                Some(Token::RParen) => pos += 1,
                _ => return Err("SYNX :calc — missing closing parenthesis".into()),
            }
            value = result;
        }
    }
}

/// Safely evaluate an arithmetic expression.
/// All variable references must be substituted with numbers before calling this.
pub fn safe_calc(expr: &str) -> Result<f64, String> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Ok(0.0);
    }
    let tokens = tokenize(trimmed)?;
    if tokens.is_empty() {
        return Ok(0.0);
    }
    evaluate(&tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_ops() {
        assert_eq!(safe_calc("2 + 3").unwrap(), 5.0);
        assert_eq!(safe_calc("10 - 4").unwrap(), 6.0);
        assert_eq!(safe_calc("3 * 7").unwrap(), 21.0);
        assert_eq!(safe_calc("20 / 4").unwrap(), 5.0);
        assert_eq!(safe_calc("10 % 3").unwrap(), 1.0);
    }

    #[test]
    fn test_precedence() {
        assert_eq!(safe_calc("2 + 3 * 4").unwrap(), 14.0);
        assert_eq!(safe_calc("(2 + 3) * 4").unwrap(), 20.0);
    }

    #[test]
    fn test_negative_numbers() {
        assert_eq!(safe_calc("-5 + 3").unwrap(), -2.0);
        assert_eq!(safe_calc("10 * -2").unwrap(), -20.0);
    }

    #[test]
    fn test_floats() {
        assert!((safe_calc("0.1 + 0.2").unwrap() - 0.3).abs() < 1e-10);
        assert_eq!(safe_calc("100 * 0.2").unwrap(), 20.0);
    }

    #[test]
    fn test_nested_parens() {
        assert_eq!(safe_calc("((2 + 3) * (4 - 1))").unwrap(), 15.0);
    }

    #[test]
    fn test_division_by_zero() {
        assert!(safe_calc("10 / 0").is_err());
    }

    #[test]
    fn test_empty() {
        assert_eq!(safe_calc("").unwrap(), 0.0);
        assert_eq!(safe_calc("  ").unwrap(), 0.0);
    }

    #[test]
    fn test_complex() {
        assert_eq!(safe_calc("100 * 5 + (20 / 4)").unwrap(), 505.0);
    }

    // Parentheses are kept on the heap: a million of them is an expression,
    // not a stack overflow.
    #[test]
    fn test_deep_parentheses_do_not_recurse() {
        let n = 1_000_000;
        let expr = format!("{}7{}", "(".repeat(n), ")".repeat(n));
        let handle = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || safe_calc(&expr))
            .unwrap();
        assert_eq!(handle.join().unwrap().unwrap(), 7.0);
        let open = "(".repeat(50_000);
        assert_eq!(safe_calc(&open).unwrap_err(), "SYNX :calc — unexpected end of expression");
    }

    // Every result and error of the recursive parser, in the same order.
    #[test]
    fn test_errors_match_the_recursive_parser() {
        let cases: &[(&str, &str)] = &[
            ("(", "SYNX :calc — unexpected end of expression"),
            ("(1", "SYNX :calc — missing closing parenthesis"),
            ("(1 2)", "SYNX :calc — missing closing parenthesis"),
            ("1 2", "SYNX :calc — unexpected token at position 1"),
            ("1)", "SYNX :calc — unexpected token at position 1"),
            ("()", "SYNX :calc — unexpected token: ')'"),
            ("1 + * 2", "SYNX :calc — unexpected token: '*'"),
            ("1 +", "SYNX :calc — unexpected end of expression"),
            ("1 / 0 + (", "SYNX :calc — division by zero"),
            ("( + 1 / 0", "SYNX :calc — unexpected token: '+'"),
            ("5 % 0", "SYNX :calc — division by zero"),
            ("1 / (2 - 2) )", "SYNX :calc — division by zero"),
            ("2 * (3 + 4", "SYNX :calc — missing closing parenthesis"),
            ("1.2.3", "SYNX :calc — invalid number: '1.2.3'"),
            ("- ", "SYNX :calc — invalid number: '-'"),
            ("-3 * -(2)", "SYNX :calc — invalid number: '-'"),
            ("2 $ 3", "SYNX :calc — unexpected character: '$' in expression"),
        ];
        for (expr, want) in cases {
            assert_eq!(safe_calc(expr).unwrap_err(), *want, "{}", expr);
        }
        let values: &[(&str, f64)] = &[
            ("1 + 2 * 3 - 4 / 2", 5.0),
            ("2 * (3 + 4) * (5 - 1)", 56.0),
            ("((1 + 2) * (3 + (4 * 5))) % 7", 6.0),
            ("10 - 4 - 3", 3.0),
            ("100 / 5 / 2", 10.0),
            ("7 % 4 * 2", 6.0),
            ("(((((1)))))", 1.0),
            (".5 + 1.", 1.5),
        ];
        for (expr, want) in values {
            assert_eq!(safe_calc(expr).unwrap(), *want, "{}", expr);
        }
    }
}
