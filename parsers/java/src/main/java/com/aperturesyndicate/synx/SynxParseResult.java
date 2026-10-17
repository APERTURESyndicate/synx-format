package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class SynxParseResult {
    public SynxValue root = SynxValue.ofObject();
    public SynxMode mode = SynxMode.STATIC;
    public boolean locked;
    public boolean tool;
    public boolean schema;
    public boolean llm;
    /**
     * {@code path -> { key -> Meta }}, both levels in declaration order. The root object is
     * {@code ""}, a nested object its keys joined with {@code .}, a list item adds {@code #<index>}
     * ({@code servers.#0}), and {@code .} or {@code \} inside a key is escaped with {@code \}.
     */
    public Map<String, Map<String, SynxMeta>> metadata = new LinkedHashMap<>();
    public List<SynxIncludeDirective> includes = new ArrayList<>();
    public List<SynxUseDirective> uses = new ArrayList<>();
    /** A resource limit dropped input: the byte cap, the line cap or a multiline block cap. */
    public boolean truncated;

    public SynxParseResult() {}
}
