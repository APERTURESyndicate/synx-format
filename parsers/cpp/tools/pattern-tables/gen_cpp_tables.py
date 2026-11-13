"""Build parsers/cpp/src/pattern_tables.inc: Unicode sets, simple case folding
and property-name aliases, all taken from regex-syntax 0.8.10.

usage: python gen_cpp_tables.py <regex-syntax dir> <tables exe> <out file>
"""
import json
import subprocess
import sys

here = sys.argv[0].rsplit('\\', 1)[0].rsplit('/', 1)[0]
rs_dir, exe, out_path = sys.argv[1], sys.argv[2], sys.argv[3]

# The binary properties the C++ engine carries; any other one is refused.
BOOLS = ['Alphabetic', 'Lowercase', 'Uppercase', 'White_Space', 'Cased', 'Hex_Digit', 'ASCII_Hex_Digit',
         'Join_Control', 'Ideographic', 'Emoji', 'Math', 'Dash']
# General categories with a table; the rest are unions of these (see pattern.cpp).
LEAF_GC = ['Uppercase_Letter', 'Lowercase_Letter', 'Titlecase_Letter', 'Modifier_Letter', 'Other_Letter',
           'Nonspacing_Mark', 'Spacing_Mark', 'Enclosing_Mark', 'Decimal_Number', 'Letter_Number', 'Other_Number',
           'Connector_Punctuation', 'Dash_Punctuation', 'Open_Punctuation', 'Close_Punctuation',
           'Initial_Punctuation', 'Final_Punctuation', 'Other_Punctuation', 'Math_Symbol', 'Currency_Symbol',
           'Modifier_Symbol', 'Other_Symbol', 'Space_Separator', 'Line_Separator', 'Paragraph_Separator',
           'Control', 'Format', 'Private_Use']

names = json.loads(subprocess.run([sys.executable, here + '/gen_names.py', rs_dir, 'json'],
                                  capture_output=True, check=True).stdout)
sc_canon = sorted(set(v for _, v in names['sc']))

lines = ['set\t\\w\tperl_word']
lines += [f'set\t\\p{{gc={g}}}\tgc:{g}' for g in LEAF_GC]
lines += [f'set\t\\p{{sc={s}}}\tsc:{s}' for s in sc_canon]
lines += [f'set\t\\p{{{b}}}\tbool:{b}' for b in BOOLS]
r = subprocess.run([exe], input=('\n'.join(lines) + '\n').encode(), capture_output=True)
if r.returncode != 0:
    sys.exit(r.stderr.decode())
body = r.stdout.decode()


def pairs(name, items):
    rows = '\n'.join(f'    {{"{a}", "{b}"}},' for a, b in items)
    return f'constexpr NamePair {name}[] = {{\n{rows}\n}};\n'


text = [
    '// Unicode data for the `[pattern]` engine, generated from regex-syntax 0.8.10',
    '// (Unicode 16.0), the crate synx-core\'s `regex` uses, by gen_cpp_tables.py.',
    '// Do not edit by hand. Sets: perl_word, gc:<leaf General_Category>,',
    '// sc:<Script>, bool:<binary property>; kSetRanges holds inclusive [lo, hi]',
    '// pairs, kFold an entry per scalar whose simple case folding orbit has other',
    '// members: the scalar, the offset of its orbit in kFoldTargets, its length.',
    '',
    body,
    '// Normalized alias (UAX44-LM3) -> canonical name.',
    pairs('kPropertyNames', names['props']),
    pairs('kGeneralCategories', names['gc']),
    pairs('kScripts', names['sc']),
]
open(out_path, 'w', encoding='utf-8', newline='\n').write('\n'.join(text))
print(out_path, len('\n'.join(text)), 'bytes')
