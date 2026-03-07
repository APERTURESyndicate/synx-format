"""LLM SYNX test cases: 125 parsing + 125 generation = 250 total."""

# ─────────────────────────────────────────────────────────────────────────────
# PARSING TEST CASES: SYNX strings with expected JSON output
# ─────────────────────────────────────────────────────────────────────────────

PARSE_TESTS = [
    # Simple key-value pairs
    {
        "id": "parse_001",
        "name": "Simple key-value",
        "synx": "name John\nage 30",
        "expected": {"name": "John", "age": 30},
    },
    {
        "id": "parse_002",
        "name": "Multiple strings",
        "synx": "first Alice\nlast Smith\ncity Boston",
        "expected": {"first": "Alice", "last": "Smith", "city": "Boston"},
    },
    {
        "id": "parse_003",
        "name": "Mix of types",
        "synx": "name Bob\nage 25\nactive true\nscore 9.5",
        "expected": {"name": "Bob", "age": 25, "active": True, "score": 9.5},
    },
    
    # Nested objects
    {
        "id": "parse_004",
        "name": "Nested object",
        "synx": "user\n  name Carol\n  age 28",
        "expected": {"user": {"name": "Carol", "age": 28}},
    },
    {
        "id": "parse_005",
        "name": "Multiple nested",
        "synx": "person\n  name David\n  admin true\nstatus active",
        "expected": {"person": {"name": "David", "admin": True}, "status": "active"},
    },
    {
        "id": "parse_006",
        "name": "Deeply nested",
        "synx": "config\n  database\n    host localhost\n    port 5432",
        "expected": {"config": {"database": {"host": "localhost", "port": 5432}}},
    },
    
    # Arrays
    {
        "id": "parse_007",
        "name": "Simple array",
        "synx": "colors [red, green, blue]",
        "expected": {"colors": ["red", "green", "blue"]},
    },
    {
        "id": "parse_008",
        "name": "Numeric array",
        "synx": "scores [100, 85, 92, 78]",
        "expected": {"scores": [100, 85, 92, 78]},
    },
    {
        "id": "parse_009",
        "name": "Mixed array",
        "synx": "mixed [true, 42, example, 3.14]",
        "expected": {"mixed": [True, 42, "example", 3.14]},
    },
    {
        "id": "parse_010",
        "name": "Array of objects",
        "synx": "users\n  []\n  name Alice\n  name Bob\n  name Carol",
        "expected": {"users": [{"name": "Alice"}, {"name": "Bob"}, {"name": "Carol"}]},
    },
    
    # Numbers and booleans
    {
        "id": "parse_011",
        "name": "Integer values",
        "synx": "count 42\ntotal 1000",
        "expected": {"count": 42, "total": 1000},
    },
    {
        "id": "parse_012",
        "name": "Float values",
        "synx": "pi 3.14159\ntemperature -5.3",
        "expected": {"pi": 3.14159, "temperature": -5.3},
    },
    {
        "id": "parse_013",
        "name": "Boolean values",
        "synx": "enabled true\ndisabled false\navailable yes",
        "expected": {"enabled": True, "disabled": False, "available": True},
    },
    
    # Comments and whitespace
    {
        "id": "parse_014",
        "name": "With comments",
        "synx": "name John\n// This is a comment\nage 30",
        "expected": {"name": "John", "age": 30},
    },
    {
        "id": "parse_015",
        "name": "Block comments",
        "synx": "x 10\n/* comment block */\ny 20",
        "expected": {"x": 10, "y": 20},
    },
    
    # Special values and null
    {
        "id": "parse_016",
        "name": "Null values",
        "synx": "value ~\nempty null",
        "expected": {"value": None, "empty": None},
    },
    {
        "id": "parse_017",
        "name": "Empty array",
        "synx": "items []",
        "expected": {"items": []},
    },
    
    # Complex nested with arrays
    {
        "id": "parse_018",
        "name": "Config with arrays",
        "synx": "database\n  host localhost\n  ports [5432, 5433]\n  credentials\n    user admin\n    pass secret",
        "expected": {
            "database": {
                "host": "localhost",
                "ports": [5432, 5433],
                "credentials": {"user": "admin", "pass": "secret"}
            }
        },
    },
    {
        "id": "parse_019",
        "name": "Array of mixed objects",
        "synx": "items\n  []\n  name Item1\n  price 10.5\n  name Item2\n  price 20.0",
        "expected": {
            "items": [
                {"name": "Item1", "price": 10.5},
                {"name": "Item2", "price": 20.0}
            ]
        },
    },
    
    # String values with spaces
    {
        "id": "parse_020",
        "name": "Strings with spaces",
        "synx": "title Hello World\nsubtitle A test case",
        "expected": {"title": "Hello World", "subtitle": "A test case"},
    },
]

# ─────────────────────────────────────────────────────────────────────────────
# GENERATION TEST CASES: English descriptions with expected SYNX output
# ─────────────────────────────────────────────────────────────────────────────

GENERATE_TESTS = [
    {
        "id": "gen_001",
        "name": "Simple user",
        "description": "Create a SYNX config with user object. The user has name 'John' and age 30.",
        "expected_contains": ["name", "John", "age", "30"],
    },
    {
        "id": "gen_002",
        "name": "Server config",
        "description": "Create a SYNX database config. Host is 'localhost', port is 5432, username is 'admin'.",
        "expected_contains": ["host", "localhost", "port", "5432", "username", "admin"],
    },
    {
        "id": "gen_003",
        "name": "Array of items",
        "description": "Create a SYNX config with items array containing three items: apple, banana, cherry.",
        "expected_contains": ["items", "apple", "banana", "cherry", "["],
    },
    {
        "id": "gen_004",
        "name": "Nested config",
        "description": "Create a SYNX config with app settings. Under 'app', add 'name' (MyApp) and 'version' (1.0).",
        "expected_contains": ["app", "name", "MyApp", "version", "1.0"],
    },
    {
        "id": "gen_005",
        "name": "Boolean values",
        "description": "Create a SYNX config with enabled set to true, debug set to false, and production set to true.",
        "expected_contains": ["enabled", "true", "debug", "false", "production", "true"],
    },
    {
        "id": "gen_006",
        "name": "Numbers and types",
        "description": "Create a SYNX config: count 42, factor 3.14, ratio 0.5, total 1000.",
        "expected_contains": ["count", "42", "factor", "3.14", "ratio", "0.5", "total", "1000"],
    },
    {
        "id": "gen_007",
        "name": "Deeply nested",
        "description": "Create a SYNX config tree: config → database → connection. Inside connection: host 'db.local', port 3306.",
        "expected_contains": ["config", "database", "connection", "host", "db.local", "port", "3306"],
    },
    {
        "id": "gen_008",
        "name": "Array of objects",
        "description": "Create a SYNX config with users array. Add two users: Alice (age 25) and Bob (age 30).",
        "expected_contains": ["users", "Alice", "age", "25", "Bob", "30", "[]"],
    },
]


TARGET_PARSE_TESTS = 125
TARGET_GENERATE_TESTS = 125


def _append_parse_tests_to_target(target_count: int) -> None:
    """Append deterministic parse tests until PARSE_TESTS reaches target_count."""
    cities = [
        "Berlin",
        "Tokyo",
        "Madrid",
        "Oslo",
        "Lisbon",
        "Prague",
        "Warsaw",
        "Dublin",
        "Helsinki",
        "Seoul",
    ]
    roles = ["admin", "editor", "viewer", "owner"]
    tiers = ["free", "pro", "business", "enterprise"]
    envs = ["dev", "qa", "stage", "prod"]

    for i in range(len(PARSE_TESTS) + 1, target_count + 1):
        mode = i % 5
        city = cities[(i - 1) % len(cities)]
        role = roles[(i - 1) % len(roles)]
        tier = tiers[(i - 1) % len(tiers)]
        env = envs[(i - 1) % len(envs)]

        if mode == 0:
            age = 18 + (i % 47)
            PARSE_TESTS.append(
                {
                    "id": f"parse_{i:03d}",
                    "name": f"KV profile {i}",
                    "synx": (
                        f"name User{i}\n"
                        f"age {age}\n"
                        f"city {city}\n"
                        f"role {role}\n"
                        f"active true"
                    ),
                    "expected": {
                        "name": f"User{i}",
                        "age": age,
                        "city": city,
                        "role": role,
                        "active": True,
                    },
                }
            )
        elif mode == 1:
            port = 4000 + i
            PARSE_TESTS.append(
                {
                    "id": f"parse_{i:03d}",
                    "name": f"Nested service {i}",
                    "synx": (
                        f"service\n"
                        f"  name svc{i}\n"
                        f"  env {env}\n"
                        f"  enabled true\n"
                        f"  connection\n"
                        f"    host api{i}.local\n"
                        f"    port {port}"
                    ),
                    "expected": {
                        "service": {
                            "name": f"svc{i}",
                            "env": env,
                            "enabled": True,
                            "connection": {"host": f"api{i}.local", "port": port},
                        }
                    },
                }
            )
        elif mode == 2:
            PARSE_TESTS.append(
                {
                    "id": f"parse_{i:03d}",
                    "name": f"Array values {i}",
                    "synx": (
                        f"tags [alpha{i}, beta{i}, gamma{i}]\n"
                        f"weights [{i}, {i + 1}, {i + 2}]\n"
                        f"flags [true, false, true]"
                    ),
                    "expected": {
                        "tags": [f"alpha{i}", f"beta{i}", f"gamma{i}"],
                        "weights": [i, i + 1, i + 2],
                        "flags": [True, False, True],
                    },
                }
            )
        elif mode == 3:
            temp = round(-((i % 13) + 1) * 1.25, 2)
            PARSE_TESTS.append(
                {
                    "id": f"parse_{i:03d}",
                    "name": f"Mixed nullable {i}",
                    "synx": (
                        f"sensor\n"
                        f"  id s{i}\n"
                        f"  reading {temp}\n"
                        f"  ok false\n"
                        f"meta\n"
                        f"  tier {tier}\n"
                        f"note ~\n"
                        f"// end"
                    ),
                    "expected": {
                        "sensor": {"id": f"s{i}", "reading": temp, "ok": False},
                        "meta": {"tier": tier},
                        "note": None,
                    },
                }
            )
        else:
            quota = 100 + (i % 20)
            used = 30 + (i % 15)
            PARSE_TESTS.append(
                {
                    "id": f"parse_{i:03d}",
                    "name": f"Business config {i}",
                    "synx": (
                        f"account\n"
                        f"  id acc{i}\n"
                        f"  plan {tier}\n"
                        f"  region {env}\n"
                        f"limits\n"
                        f"  quota {quota}\n"
                        f"  used {used}\n"
                        f"  features [api, export, alerts]"
                    ),
                    "expected": {
                        "account": {"id": f"acc{i}", "plan": tier, "region": env},
                        "limits": {
                            "quota": quota,
                            "used": used,
                            "features": ["api", "export", "alerts"],
                        },
                    },
                }
            )


def _append_generate_tests_to_target(target_count: int) -> None:
    """Append deterministic generation tests until GENERATE_TESTS reaches target_count."""
    regions = [
        "us-east",
        "us-west",
        "eu-central",
        "ap-south",
        "sa-east",
        "eu-west",
        "ap-northeast",
        "ca-central",
        "me-central",
        "af-south",
        "eu-north",
        "ap-southeast",
        "us-central",
        "eu-south",
    ]

    current = len(GENERATE_TESTS)
    for n in range(current + 1, target_count + 1):
        region = regions[(n - 1) % len(regions)]
        replicas = 1 + (n % 4)
        port = 5000 + n
        name = f"app{n}"
        version = f"{(n % 3) + 1}.{n % 10}"
        timeout = 20 + (n % 40)
        retries = 1 + (n % 5)

        GENERATE_TESTS.append(
            {
                "id": f"gen_{n:03d}",
                "name": f"Deployment profile {n}",
                "description": (
                    "Create a SYNX deployment config with object 'app'. "
                    f"Set name to '{name}', version to '{version}', region to '{region}', "
                    f"replicas to {replicas}, and port to {port}. "
                    f"Set timeout to {timeout} and retries to {retries}. "
                    "Add boolean field 'enabled' set to true and array field 'features' with auth, api, metrics."
                ),
                "expected_contains": [
                    "app",
                    "name",
                    name,
                    "version",
                    version,
                    "region",
                    region,
                    "replicas",
                    str(replicas),
                    "port",
                    str(port),
                    "timeout",
                    str(timeout),
                    "retries",
                    str(retries),
                    "enabled",
                    "true",
                    "features",
                    "auth",
                    "api",
                    "metrics",
                ],
            }
        )


_append_parse_tests_to_target(TARGET_PARSE_TESTS)
_append_generate_tests_to_target(TARGET_GENERATE_TESTS)

assert len(PARSE_TESTS) == TARGET_PARSE_TESTS, (
    f"Expected {TARGET_PARSE_TESTS} parse tests, got {len(PARSE_TESTS)}"
)
assert len(GENERATE_TESTS) == TARGET_GENERATE_TESTS, (
    f"Expected {TARGET_GENERATE_TESTS} generation tests, got {len(GENERATE_TESTS)}"
)

def get_all_tests():
    """Return all test cases"""
    return {
        "parse": PARSE_TESTS,
        "generate": GENERATE_TESTS,
    }

def get_parse_tests():
    return PARSE_TESTS

def get_generate_tests():
    return GENERATE_TESTS

def get_test_by_id(test_id):
    """Find a test case by ID"""
    for test in PARSE_TESTS + GENERATE_TESTS:
        if test["id"] == test_id:
            return test
    return None

if __name__ == "__main__":
    all_tests = get_all_tests()
    print(f"Parse tests: {len(all_tests['parse'])}")
    print(f"Generate tests: {len(all_tests['generate'])}")
    print(f"Total: {len(all_tests['parse']) + len(all_tests['generate'])} tests")
