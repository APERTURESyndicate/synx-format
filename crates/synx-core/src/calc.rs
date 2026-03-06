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

        return Err(format!(
            "SYNX :calc — unexpected character: '{}' in expression",
            ch as char
        ));
    }

    Ok(tokens)
}

/// Recursive descent parser.
/// Grammar:
///   expr   → term (('+' | '-') term)*
///   term   → factor (('*' | '/' | '%') factor)*
///   factor → NUMBER | '(' expr ')'
struct ExprParser {
    tokens: Vec<Token>,
    pos: usize,
}

impl ExprParser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn parse(&mut self) -> Result<f64, String> {
        let result = self.expr()?;
        if self.pos < self.tokens.len() {
            return Err(format!(
                "SYNX :calc — unexpected token at position {}",
                self.pos
            ));
        }
        Ok(result)
    }

    fn expr(&mut self) -> Result<f64, String> {
        let mut left = self.term()?;
        while self.pos < self.tokens.len() {
            match &self.tokens[self.pos] {
                Token::Op(b'+') => {
                    self.pos += 1;
                    left += self.term()?;
                }
                Token::Op(b'-') => {
                    self.pos += 1;
                    left -= self.term()?;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut left = self.factor()?;
        while self.pos < self.tokens.len() {
            match &self.tokens[self.pos] {
                Token::Op(b'*') => {
                    self.pos += 1;
                    left *= self.factor()?;
                }
                Token::Op(b'/') => {
                    self.pos += 1;
                    let right = self.factor()?;
                    if right == 0.0 {
                        return Err("SYNX :calc — division by zero".into());
                    }
                    left /= right;
                }
                Token::Op(b'%') => {
                    self.pos += 1;
                    left %= self.factor()?;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn factor(&mut self) -> Result<f64, String> {
        if self.pos >= self.tokens.len() {
            return Err("SYNX :calc — unexpected end of expression".into());
        }

        match &self.tokens[self.pos] {
            Token::Number(val) => {
                let val = *val;
                self.pos += 1;
                Ok(val)
            }
            Token::LParen => {
                self.pos += 1;
                let val = self.expr()?;
                if self.pos >= self.tokens.len() {
                    return Err("SYNX :calc — missing closing parenthesis".into());
                }
                match &self.tokens[self.pos] {
                    Token::RParen => {
                        self.pos += 1;
                        Ok(val)
                    }
                    _ => Err("SYNX :calc — missing closing parenthesis".into()),
                }
            }
            other => Err(format!("SYNX :calc — unexpected token: {:?}", other)),
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
    ExprParser::new(tokens).parse()
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
}
