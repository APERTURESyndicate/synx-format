"""Differential test of a port's [pattern] engine against the `regex` crate.
usage: SYNX_REGEX_ORACLE=<built src/main.rs> python rxdiff.py <port probe exe> [random count] [seed]
A probe reads `pattern<TAB>value` (hex UTF-8) per line and answers `invalid`, `match` or `nomatch`."""
import os
import random
import subprocess
import sys

ORACLE = os.environ.get('SYNX_REGEX_ORACLE', 'synx-pattern-tables')


def run(exe, pairs):
    data = ''.join(p.encode().hex() + '\t' + v.encode().hex() + '\n' for p, v in pairs)
    r = subprocess.run(exe if isinstance(exe, list) else [exe], input=data.encode(), capture_output=True, timeout=1200)
    out = r.stdout.decode().split()
    if len(out) != len(pairs):
        sys.exit(f'{exe}: {len(out)} answers for {len(pairs)}; stderr: {r.stderr.decode()[-800:]}')
    return out


FIXED = [
    ('{', 'y'), ('a{', 'a'), ('a{x}', 'a'), ('a{2}{3}', 'aaaaaa'), ('a**', 'b'), ('a*+', 'b'), ('^*', 'b'),
    ('(?i)*', 'b'), ('}', '}'), (']', ']'), ('a{,3}', 'a'), ('a{ 2 }', 'aa'), ('\\/', '/'), ('\\@', '@'),
    ('\\_', '_'), ("\\'", "'"), ('\\é', 'é'), ('\\0', 'x'), ('\\e', 'x'), ('\\a', '\x07'), ('[]', 'x'),
    ('[]a]', ']'), ('[a-]', '-'), ('[[:alpha:]]', 'a'), ('[a&&b]', 'a'), ('(?=a)', 'a'), ('\\1', 'a'),
    ('(?P<n>a)', 'a'), ('(?<n>a)', 'a'), ('(?x) a b', 'ab'), ('\\<a', 'a'), ('x{1001}', 'x'), ('a{2,1}', 'a'),
    ('(?-u:.)', 'a'), ('(?-u)\\w', 'é'), ('\\pL', 'é'), ('\\p{greek}', 'α'), ('\\p{Script=Greek}', 'α'),
    ('\\p{sc=greek}', 'α'), ('\\p{Letter}', 'a'), ('\\p{L}', '1'), ('(?i)\\p{Lu}', 'a'), ('\\b{start}a', 'a'),
    ('\\K', 'a'), ('[\\d-z]', 'a'), ('[a-\\d]', 'a'), ('\\x{110000}', 'a'), ('(?i)K', '\u212a'), ('()', 'a'),
    ('(|a)', 'b'), ('\\Q', 'a'), ('[[:word:]]', '_'), ('[^\\x00-\\x{10FFFF}]', 'a'), ('(?s).', '\n'), ('.', '\n'),
    ('\\pN', '5'), ('\\p{Any}', 'a'), ('\\p{Alphabetic}', 'a'), ('\\p{IsGreek}', 'α'), ('\\p{Cyrillic}', 'ж'),
    ('\\p{N', 'a'), ('$a', 'a'), ('a{2,}', 'aa'), ('[a-a]', 'a'), ('[z-a]', 'a'), ('[\\[]', '['), ('[[]', '['),
    ('\\z', 'a'), ('\\A', 'a'), ('\\Z', 'a'), ('\\ ', ' '), ('[:alpha:]', 'b'), ('[:alpha:]', ':'),
    ('[a-c-e]', '-'), ('[a-c-e]', 'd'), ('[a&&]', 'a'), ('[&&a]', 'a'), ('[a--b]', 'a'), ('[a~~b]', 'a'),
    ('[a&&b&&c]', 'a'), ('[\\b]', 'a'), ('[^]a]', 'b'), ('(?i-)a', 'a'), ('(?ii)a', 'a'), ('(?)a', 'a'),
    ('(?-)a', 'a'), ('(?i:a|b)c', 'Bc'), ('a(?i)b|c', 'C'), ('(?i)a|b', 'B'), ('(?U)a', 'a'), ('(?R)a', 'a'),
    ('(?x)[ a ]', ' '), ('(?x)a # c', 'a'), ('\\x41', 'A'), ('\\x{41}', 'A'), ('\\u0041', 'A'), ('\\u{41}', 'A'),
    ('\\U00000041', 'A'), ('\\U{41}', 'A'), ('\\x{D800}', 'a'), ('(?-u)\\xFF', 'a'), ('(?-u)[^a]', 'b'),
    ('(?-u)\\pL', 'a'), ('(?-u)é', 'é'), ('(?i)(?-u)é', 'É'), ('(?P<a>x)(?P<a>y)', 'xy'), ('(?P<1a>x)', 'x'),
    ('(?P<a.b>x)', 'x'), ('(?P<>x)', 'x'), ('a{4294967296}', 'a'), ('a{1}?', 'a'), ('a??', 'a'),
    ('\\b{start-half}a', 'a'), ('\\b{foo}a', 'a'), ('x*', ''), ('a|', 'b'), ('|', 'b'), ('((((a))))', 'a'),
    ('(?m)^b', 'a\nb'), ('(?m)a$', 'a\r\nb'), ('(?mR)a$', 'a\r\nb'), ('(?R).', '\r'), ('a$', 'a\n'),
    ('[\\n]', '\n'), ('[\\x{61}-\\x{63}]', 'b'), ('[\\pL&&\\p{Greek}]', 'α'), ('[[:^alpha:]]', '1'),
    ('[[:foo:]]', 'a'), ('[[:foo:]]', 'f'), ('\\p{L}{2}', 'ab'), ('[\\p{L}--[a]]', 'a'), ('\\p{Greek_And_Coptic}', 'α'),
    ('\\p{scx=Greek}', 'α'), ('\\p{gc=Lu}', 'A'), ('\\p{lu}', 'A'), ('\\p{ L u }', 'A'), ('\\p{uppercase}', 'A'),
    ('\\p{White Space}', ' '), ('\\p{Emoji}', '😀'), ('\\p{Han}', '漢'), ('\\p{age=6.0}', 'a'), ('\\p{Word}', 'a'),
    ('\\p{XID_Start}', 'a'), ('^\\p{L}+$', 'мир'), ('^\\d+$', '١٢٣'), ('^\\w+$', 'naïve_1'), ('\\bfoo\\b', 'a foo b'),
    ('\\bfoo\\b', 'food'), ('(?i)^abc$', 'ABC'), ('(?i)^abc$', 'x'), ('^(a|aa)+$', 'a' * 60 + '!'),
    ('(?i)ß', 'ẞ'), ('(?i)[k]', '\u212a'), ('(?i)[^k]', 'K'), ('(?i)\\x{212a}', 'k'), ('[\\s]', '\u3000'),
    ('\\S', ' '), ('\\W', 'é'), ('\\D', '٣'), ('^.$', '😀'), ('^..$', '😀'), ('\\B', ''), ('\\b', ''),
    ('a\\B', 'ab'), ('(?m)$', ''), ('^$', ''), ('(?x)\\#', '#'), ('(?x)a\\ b', 'a b'), ('[a-z&&[^aeiou]]+', 'xyz'),
    ('^[a-z0-9_-]+$', 'my-key_1'), ('^https?://', 'https://x'), ('^\\d{3}-\\d{4}$', '555-1234'),
    ('^[A-Z]{2,3}$', 'ABCD'), ('(?i)^[a-f0-9]{8}$', 'DEADBEEF'), ('^\\S+@\\S+$', 'a@b'), ('\\.', 'x'),
    ('^.{3,}$', 'ab'), ('^(?:\\d+\\.){3}\\d+$', '10.0.0.1'), ('(?s)^.*$', 'a\nb'), ('^.*$', 'a\nb'),
    ('[\\u{41}-\\u{5A}]', 'Q'), ('[^[:space:]]', ' '), ('(?i)[[:upper:]]', 'a'), ('(?i)\\p{Ll}', 'A'),
    ('\\P{L}', '1'), ('\\p{^L}', 'a'), ('\\p{L!=}', 'a'), ('\\P{sc!=Greek}', 'α'), ('[[:alpha:][:digit:]]', '5'),
    ('[\\w--\\d]', '5'), ('[a-z~~c-x]', 'b'), ('[a-z~~c-x]', 'd'), ('(a)|b)', 'a'), ('(a', 'a'), ('a)', 'a'),
    ('a{1,2}{3}', 'aaa'), ('(?:)', ''), ('(?:a|)+', ''), ('(?i)İ', 'i'), ('(?i)ı', 'I'), ('(?i)σ', 'ς'),
    ('\\b{2}', 'a'), ('\\x', 'a'), ('\\x4', 'a'), ('\\u{}', 'a'), ('[\\-]', '-'), ('[a\\-z]', 'b'),
    ('[--a]', '-'), ('[a-z--]', 'b'), ('[\\]]', ']'), ('[^-]', 'a'), ('(?x)[a b]', ' '), ('(?x)a{ 2 }', 'aa'),
    ('(?x)a #c\n b', 'ab'), ('[\\d&&[0-5]]', '7'), ('(?i)[a-z--k]', 'K'), ('(?i)\\pL', '1'), ('(?-u)\\d', '٣'),
    ('(?-u)\\b', 'é'), ('(?-u)[a]', 'a'), ('(?-u)[[:^alpha:]]', '1'), ('(?-u)\\D', 'a'), ('\\u{d800}', 'a'),
]

ATOMS = ['a', 'b', 'é', 'Ж', '.', '\\d', '\\w', '\\s', '\\D', '\\W', '\\S', '[a-c]', '[^b]', '[[:alpha:]]', '\\pL',
         '\\p{Greek}', '\\b', '\\B', '^', '$', '(?i)', '(?-i)', '(?m)', '(?s)', '(?x)', ' ', '\\.', '-', ']', '}',
         '{', '[a-z&&[^x]]', '\\x41', '\\u{430}', '(', ')', '|', '[', '\\', '#', '\n', 'K', 'k', 'ß']
QUANTS = ['', '', '', '*', '+', '?', '{2}', '{1,3}', '{0,}', '*?', '{,2}', '**']
VALS = ['', 'a', 'ab', 'aaa', 'abc', 'ABC', 'é', 'Ж', 'жж', 'a b', 'a\nb', '1', '123', 'x.y', 'α', '-', ']', '}',
        'a_b', 'K', '\u212a', '\r\n', ' ', 'aé1', 'ba', 'bab', 'xyz', '!', 'ss', 'SS']


def rand_pattern(rng):
    n = rng.randint(1, 6)
    parts = []
    for _ in range(n):
        if rng.random() < 0.15:
            inner = ''.join(rng.choice(ATOMS) + rng.choice(QUANTS) for _ in range(rng.randint(1, 3)))
            parts.append(rng.choice(['(', '(?:', '(?i:', '(?P<n%d>' % rng.randint(0, 9)]) + inner + rng.choice([')', '|b)', ')']))
        else:
            parts.append(rng.choice(ATOMS))
        parts.append(rng.choice(QUANTS))
    return ''.join(parts)


if __name__ == '__main__':
    exe = sys.argv[1].split('|')
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 0
    rng = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 1)
    pairs = list(FIXED)
    for _ in range(count):
        p = rand_pattern(rng)
        for v in rng.sample(VALS, 4):
            pairs.append((p, v))
    want = run(ORACLE, pairs)
    got = run(exe, pairs)
    bad = [(p, v, w, g) for (p, v), w, g in zip(pairs, want, got) if w != g]
    sys.stdout.reconfigure(encoding='utf-8')
    print(f'{len(pairs) - len(bad)} of {len(pairs)} agree')
    seen = set()
    for p, v, w, g in bad:
        if p in seen:
            continue
        seen.add(p)
        print(f'  {p!r} on {v!r}: regex {w}, port {g}')
        if len(seen) >= 60:
            break
