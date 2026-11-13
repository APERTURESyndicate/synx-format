"""Unicode property names of regex-syntax 0.8.10 (the crate synx-core's `regex`
uses) for the ports' [pattern] engines: normalized alias -> canonical name.

Usage: python gen_names.py <regex-syntax dir> dart|json
"""
import os
import re
import sys

root = sys.argv[1]
mode = sys.argv[2]
tables = os.path.join(root, 'src', 'unicode_tables')

names_src = open(os.path.join(tables, 'property_names.rs'), encoding='utf-8').read()
prop_names = re.findall(r'\("([^"]*)", "([^"]*)"\)', names_src)

values_src = open(os.path.join(tables, 'property_values.rs'), encoding='utf-8').read()
values = {}
for block in re.finditer(r'\(\s*"([A-Za-z_]+)",\s*&\[(.*?)\],\s*\)', values_src, re.S):
    values[block.group(1)] = re.findall(r'\("([^"]*)", "([^"]*)"\)', block.group(2))

bool_src = open(os.path.join(tables, 'property_bool.rs'), encoding='utf-8').read()
by_name = re.search(r'pub const BY_NAME[^=]*=\s*&\[(.*?)\];', bool_src, re.S).group(1)
bools = re.findall(r'\("([^"]*)",', by_name)

gc = values['General_Category']
sc = values['Script']

if mode == 'json':
    import json
    print(json.dumps({'props': prop_names, 'gc': gc, 'sc': sc, 'bool': bools}))
    sys.exit(0)

def enc(pairs):
    return ';'.join(f'{a}={b}' for a, b in pairs)

print('// Unicode property names the `[pattern]` syntax accepts, as regex-syntax')
print('// 0.8.10 (Unicode 16.0) knows them: a normalized alias (UAX44-LM3: lower')
print('// case, no space, `_` or `-`, no `is` prefix) and its canonical name.')
print('// Generated from regex-syntax\'s unicode_tables; do not edit by hand.')
print()
print("const String propertyNames = '" + enc(prop_names) + "';")
print()
print("const String generalCategories = '" + enc(gc) + "';")
print()
print("const String scripts = '" + enc(sc) + "';")
print()
print("const String booleanProperties = '" + ';'.join(bools) + "';")
