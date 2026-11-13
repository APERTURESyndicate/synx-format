// Port worker for the Go implementation: one request per line on stdin,
//   id \t op \t base64(text) \t base64(KEY=VALUE lines) \t base directory
// and one answer per line on stdout,
//   id \t base64(canonical JSON, or "ERROR: …") \t base64(default string form of the tree)
package main

import (
	"bufio"
	"encoding/base64"
	"fmt"
	"os"
	"strings"

	synx "github.com/aperturesyndicate/synx"
)

func run(op, text string, env map[string]string, base string) (out, shown string) {
	defer func() {
		if r := recover(); r != nil {
			out = fmt.Sprintf("ERROR: panic: %v", r)
		}
	}()
	if op == "decompile" {
		// `text` is the .synxb container itself.
		t, err := synx.DecompileToText([]byte(text))
		if err != nil {
			return "ERROR: " + err.Error(), ""
		}
		return t, ""
	}
	res := synx.Parse(text)
	if op == "active" {
		synx.Resolve(&res, synx.Options{Env: env, BasePath: base})
	}
	return synx.ToJSON(res.Root), fmt.Sprintf("%v %+v %s", res.Root, res.Root, res.Root)
}

func main() {
	sc := bufio.NewScanner(os.Stdin)
	sc.Buffer(make([]byte, 1<<20), 1<<28)
	w := bufio.NewWriter(os.Stdout)
	defer w.Flush()
	for sc.Scan() {
		f := strings.Split(sc.Text(), "\t")
		if len(f) < 5 {
			continue
		}
		text, _ := base64.StdEncoding.DecodeString(f[2])
		envRaw, _ := base64.StdEncoding.DecodeString(f[3])
		env := map[string]string{}
		for _, l := range strings.Split(string(envRaw), "\n") {
			if i := strings.Index(l, "="); i > 0 {
				env[l[:i]] = l[i+1:]
			}
		}
		out, shown := run(f[1], string(text), env, f[4])
		fmt.Fprintf(w, "%s\t%s\t%s\n", f[0], base64.StdEncoding.EncodeToString([]byte(out)), base64.StdEncoding.EncodeToString([]byte(shown)))
	}
}
