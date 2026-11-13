#!/usr/bin/env bash
# Scratch: package commands of the CLI against links, odd names and paths. Everything lives in a temp dir.
S="$1"
T="$(mktemp -d)"
cd "$T" || exit 2
W="$(cygpath -w "$T")"
fresh() {
  rm -rf "$T/P" "$T/V" "$T/L" 2>/dev/null
  cmd //c "rmdir /s /q $W\\P" >/dev/null 2>&1
  mkdir -p "$T/P" "$T/V" "$T/L/src"
  echo keep > "$T/V/keep.txt"
  printf 'name @s/pkg\nversion 1.0.0\ndescription d\nauthor a\nlicense MIT\nmain src/main.synx\n' > "$T/L/synx-pkg.synx"
  echo 'greeting hello' > "$T/L/src/main.synx"
}
junction() { cmd //c "mklink /J $(cygpath -w "$1") $(cygpath -w "$2")" >/dev/null 2>&1; }
victim_ok() { [ -f "$T/V/keep.txt" ] && [ "$(ls -A "$T/V" | wc -l)" = "1" ]; }
report() { if victim_ok; then echo "ok    $1 — $2"; else echo "BREAK $1 — victim dir now: $(ls -A "$T/V" | tr '\n' ' ') — $2"; fi; }
run() { (cd "$T/P" && "$S" "$@" 2>&1 | tail -1 | cut -c1-110); }

fresh; junction "$T/P/synx_packages" "$T/V"
report "synx_packages is a junction, install --path" "$(run install --path "$T/L")"

fresh; mkdir -p "$T/P/synx_packages"; junction "$T/P/synx_packages/@s" "$T/V"
report "@scope is a junction, install --path" "$(run install --path "$T/L")"

fresh; mkdir -p "$T/P/synx_packages/@s"; junction "$T/P/synx_packages/@s/pkg" "$T/V"
report "package dir is a junction, uninstall" "$(run uninstall @s/pkg)"

fresh; mkdir -p "$T/P/synx_packages/@s"; junction "$T/P/synx_packages/@s/pkg" "$T/V"
report "package dir is a junction, install over it" "$(run install --path "$T/L")"

fresh; mkdir -p "$T/P/synx_packages"; junction "$T/P/synx_packages/.staging" "$T/V"
report ".staging is a junction, install --path" "$(run install --path "$T/L")"

fresh; junction "$T/L/src/link" "$T/V"
out="$(run install --path "$T/L")"
if find "$T/P/synx_packages" -name keep.txt 2>/dev/null | grep -q .; then echo "BREAK source tree junction followed: keep.txt was copied into the project — $out"; else echo "ok    junction inside the package source is not followed — $out"; fi

for n in '../x' '@s/..' '@s/pkg.' 'CON' '@s/nul' '@s/a b' 'C:\x' '@s/pkg/../../x' '..' '.' '@s/.staging' '@s/com1.txt' '@s/pkg ' '@s/аб' '@s/%TEMP%'; do
  fresh; mkdir -p "$T/P/keepme"; echo x > "$T/P/keepme/f"
  printf 'name %s\nversion 1.0.0\ndescription d\nauthor a\nlicense MIT\nmain src/main.synx\n' "$n" > "$T/L/synx-pkg.synx"
  out="$(run install --path "$T/L")"
  extra="$(cd "$T" && ls -A | grep -v -E '^(P|V|L)$' | tr '\n' ' ')"
  if victim_ok && [ -f "$T/P/keepme/f" ] && [ -z "$extra" ]; then echo "ok    manifest name $(printf '%q' "$n") — $out"; else echo "BREAK manifest name $n — extra: $extra — $out"; fi
  out="$(run uninstall "$n")"
  if victim_ok && [ -f "$T/P/keepme/f" ]; then echo "ok    uninstall $(printf '%q' "$n") — $out"; else echo "BREAK uninstall $n — $out"; fi
done

fresh; echo secret > "$T/outside.txt"; junction "$T/L/src/link" "$T/V"
printf 'name @s/pkg\nversion 1.0.0\ndescription d\nauthor a\nlicense MIT\nmain src/main.synx\nfiles\n  - src/main.synx\n  - ../outside.txt\n  - %s\n  - src/link/keep.txt\n  - src/../../outside.txt\n' "$(cygpath -w "$T/outside.txt")" > "$T/L/synx-pkg.synx"
out="$(cd "$T/L" && "$S" pack . 2>&1 | tail -1 | cut -c1-140)"
arc="$(ls "$T/L"/*.tar.gz 2>/dev/null | head -1)"
if [ -n "$arc" ]; then listing="$(cd "$T/L" && tar -tzf "$(basename "$arc")" 2>/dev/null | tr '\n' ' ')"; else listing="(no archive)"; fi
case "$listing" in *outside*|*keep.txt*) echo "BREAK pack took files from outside: $listing — $out";; *) echo "ok    pack with files pointing outside — $out — archive: $listing";; esac

# Control characters in a package manifest must not reach the terminal: ESC [2K ESC [1G rewrites the
# line the user reads, OSC 0 sets the window title, OSC 52 writes the clipboard.
fresh; mkdir -p "$T/P/synx_packages/@x/y"
printf 'name @x/y\nversion 1.0.0\033[2K\033[1G  @trusted/core@9.9.9\ndescription ok\033]0;title-from-a-package\007\nauthor me\033]52;c;ZWNobw==\007\nlicense MIT\nmain src/main.synx\n' > "$T/P/synx_packages/@x/y/synx-pkg.synx"
for c in "list" "info @x/y"; do
  n="$(cd "$T/P" && "$S" $c 2>&1 | tr -cd '\033\007' | wc -c | tr -d ' ')"
  if [ "$n" = "0" ]; then echo "ok    $c prints no control characters from a manifest"; else echo "BREAK $c prints $n raw ESC/BEL bytes taken from synx-pkg.synx"; fi
done

# Credential files inside the package sources stay out of the archive: the same names the MCP
# sandbox refuses (any *.env), and the usual homes of tokens and passwords.
fresh; mkdir -p "$T/L/src/sub"
for f in prod.env local.env sub/staging.env .envrc _netrc .pgpass .my.cnf .git-credentials credentials .dockercfg kubeconfig id_rsa_github; do echo "TOKEN=x" > "$T/L/src/$f"; done
out="$(cd "$T/L" && "$S" pack . 2>&1)"
arc="$(ls "$T/L"/*.tar.gz 2>/dev/null | head -1)"
if [ -n "$arc" ]; then listing="$(cd "$T/L" && tar -tzf "$(basename "$arc")" 2>/dev/null | grep -v -E '^(synx-pkg\.synx|src/|src/main\.synx|src/sub/)$' | tr '\n' ' ')"; else listing=""; fi
if [ -z "$listing" ]; then echo "ok    pack leaves credential files out"; else echo "BREAK pack put credential files into the archive: $listing"; fi

cd /; cmd //c "rmdir /s /q $W" >/dev/null 2>&1; rm -rf "$T" 2>/dev/null
