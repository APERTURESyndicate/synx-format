// Oracle for the ports' [pattern] engines: what synx-core's `regex` says.
// stdin: one request per line, `pattern<TAB>value` with both hex-encoded UTF-8.
// stdout: `invalid`, `match` or `nomatch` per line.
use std::io::{BufRead, Write};

fn unhex(s: &str) -> String {
    let b: Vec<u8> = (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect();
    String::from_utf8(b).unwrap()
}

fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::BufWriter::new(std::io::stdout());
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let mut it = line.split('\t');
        let p = unhex(it.next().unwrap_or(""));
        let v = unhex(it.next().unwrap_or(""));
        let ans = match regex::Regex::new(&p) {
            Err(_) => "invalid",
            Ok(re) => if re.is_match(&v) { "match" } else { "nomatch" },
        };
        writeln!(out, "{}", ans).unwrap();
    }
}
