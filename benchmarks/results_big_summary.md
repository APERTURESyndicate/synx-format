## Big verification run 2026-04-01T14:06:27
CPU cores: 20 (target ~80% jobs=16)

### Release verification (scripts/verify-release-quality.ps1)
- Pass (see: benchmarks/results_big_* files + terminal output)

### synx-js Jest
- Pass (81 tests)

### Node benchmark (50k iters each)
ΓòöΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòù
Γòæ         SYNX Benchmark ΓÇö Node.js  (50 000 iterations)           Γòæ
ΓòáΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòú
Γòæ  Input sizes:  JSON 3301 b   YAML 2467 b   XML 3579 b   SYNX 2527 b   Γòæ
ΓòÜΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓò¥

  Parser                               Time/call   Total (ms)   vs JSON   vs Rust
  ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ
  JSON.parse (built-in)                 5.910 ┬╡s     295.5 ms     1.00x     0.06x ΓåÉfastest
  YAML  (js-yaml)                       67.26 ┬╡s    3362.8 ms    11.38x     0.74x
  XML   (fast-xml-parser)              161.59 ┬╡s    8079.5 ms    27.34x     1.77x
  SYNX  (synx-js / pure JS)             19.99 ┬╡s     999.5 ms     3.38x     0.22x
  SYNX  (synx-native / Rust)            91.08 ┬╡s    4553.9 ms    15.41x     1.00x
  SYNX  (native parseToJson)            44.82 ┬╡s    2241.1 ms     7.58x     0.49x

  Note: run with --expose-gc for more stable GC-controlled results.
  e.g.  node --expose-gc bench_node.js

  Results saved ΓåÆ A:\APERTURESyndicate\ASC\synx-format\benchmarks\results_node.json

### Python benchmark (10k iters each)
ΓòöΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòù
Γòæ         SYNX Benchmark ΓÇö Python  (10,000 iterations)           Γòæ
ΓòáΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòú
Γòæ  Input sizes:  JSON 3301 b   YAML 2467 b   XML 3579 b   SYNX 2375 b   Γòæ
ΓòÜΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓòÉΓò¥

  Parser                                  Time/call    Total (ms)   vs json
  ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ
  json.loads (built-in)                    12.94 ┬╡s       129.4 ms     1.00x ΓåÉfastest
  yaml.safe_load (PyYAML)                4024.91 ┬╡s     40249.1 ms   310.96x
  xml.etree (parse)                        48.24 ┬╡s       482.4 ms     3.73x
  synx_native.parse (Rust)                 46.49 ┬╡s       464.9 ms     3.59x
  synx_native.parseToJson                  43.62 ┬╡s       436.2 ms     3.37x

  Python 3.11.9 ┬╖ Windows AMD64

  Results saved ΓåÆ A:\APERTURESyndicate\ASC\synx-format\benchmarks\results_python.json

### Rust criterion benchmark
  3 (3.00%) low mild
  5 (5.00%) high mild
  3 (3.00%) high severe
Benchmarking synx_parse/full/110-keys
Benchmarking synx_parse/full/110-keys: Warming up for 3.0000 s
Benchmarking synx_parse/full/110-keys: Collecting 100 samples in estimated 5.0778 s (141k iterations)
Benchmarking synx_parse/full/110-keys: Analyzing
synx_parse/full/110-keys
                        time:   [35.680 ┬╡s 35.869 ┬╡s 36.169 ┬╡s]
Found 6 outliers among 100 measurements (6.00%)
  4 (4.00%) high mild
  2 (2.00%) high severe

Benchmarking synx_api/Synx::parse
Benchmarking synx_api/Synx::parse: Warming up for 3.0000 s
Benchmarking synx_api/Synx::parse: Collecting 100 samples in estimated 5.0868 s (141k iterations)
Benchmarking synx_api/Synx::parse: Analyzing
synx_api/Synx::parse    time:   [35.858 ┬╡s 35.936 ┬╡s 36.022 ┬╡s]
Found 5 outliers among 100 measurements (5.00%)
  3 (3.00%) low mild
  1 (1.00%) high mild
  1 (1.00%) high severe
Benchmarking synx_api/parse_to_json
Benchmarking synx_api/parse_to_json: Warming up for 3.0000 s
Benchmarking synx_api/parse_to_json: Collecting 100 samples in estimated 5.0908 s (116k iterations)
Benchmarking synx_api/parse_to_json: Analyzing
synx_api/parse_to_json  time:   [43.317 ┬╡s 43.524 ┬╡s 43.844 ┬╡s]
Found 6 outliers among 100 measurements (6.00%)
  2 (2.00%) low mild
  2 (2.00%) high mild
  2 (2.00%) high severe

Benchmarking throughput/SYNX_parse_bytes/s
Benchmarking throughput/SYNX_parse_bytes/s: Warming up for 3.0000 s
Benchmarking throughput/SYNX_parse_bytes/s: Collecting 100 samples in estimated 5.0784 s (141k iterations)
Benchmarking throughput/SYNX_parse_bytes/s: Analyzing
throughput/SYNX_parse_bytes/s
                        time:   [35.671 ┬╡s 35.755 ┬╡s 35.840 ┬╡s]
                        thrpt:  [67.242 MiB/s 67.402 MiB/s 67.560 MiB/s]
Found 9 outliers among 100 measurements (9.00%)
  3 (3.00%) low mild
  4 (4.00%) high mild
  2 (2.00%) high severe

Benchmarking synx_binary/compile/110-keys
Benchmarking synx_binary/compile/110-keys: Warming up for 3.0000 s
Benchmarking synx_binary/compile/110-keys: Collecting 100 samples in estimated 5.2018 s (50k iterations)
Benchmarking synx_binary/compile/110-keys: Analyzing
synx_binary/compile/110-keys
                        time:   [104.53 ┬╡s 105.11 ┬╡s 105.67 ┬╡s]
Found 6 outliers among 100 measurements (6.00%)
  2 (2.00%) low mild
  2 (2.00%) high mild
  2 (2.00%) high severe
Benchmarking synx_binary/decompile/110-keys
Benchmarking synx_binary/decompile/110-keys: Warming up for 3.0000 s
Benchmarking synx_binary/decompile/110-keys: Collecting 100 samples in estimated 5.0207 s (136k iterations)
Benchmarking synx_binary/decompile/110-keys: Analyzing
synx_binary/decompile/110-keys
                        time:   [36.982 ┬╡s 37.098 ┬╡s 37.243 ┬╡s]
Found 16 outliers among 100 measurements (16.00%)
  3 (3.00%) low mild
  6 (6.00%) high mild
  7 (7.00%) high severe
Benchmarking synx_binary/size_text_bytes
Benchmarking synx_binary/size_text_bytes: Warming up for 3.0000 s
Benchmarking synx_binary/size_text_bytes: Collecting 100 samples in estimated 5.0000 s (22B iterations)
Benchmarking synx_binary/size_text_bytes: Analyzing
synx_binary/size_text_bytes
                        time:   [222.87 ps 223.75 ps 224.69 ps]
Benchmarking synx_binary/size_binary_bytes
Benchmarking synx_binary/size_binary_bytes: Warming up for 3.0000 s
Benchmarking synx_binary/size_binary_bytes: Collecting 100 samples in estimated 5.0000 s (22B iterations)
Benchmarking synx_binary/size_binary_bytes: Analyzing
synx_binary/size_binary_bytes
                        time:   [427.73 ps 428.46 ps 429.25 ps]
Found 3 outliers among 100 measurements (3.00%)
  1 (1.00%) high mild
  2 (2.00%) high severe

