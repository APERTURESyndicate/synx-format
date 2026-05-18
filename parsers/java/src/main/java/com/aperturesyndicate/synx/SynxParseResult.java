package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

public final class SynxParseResult {
    public SynxValue root = SynxValue.ofObject();
    public SynxMode mode = SynxMode.STATIC;
    public boolean locked;
    public boolean tool;
    public boolean schema;
    public boolean llm;
    /** {@code path -> { key -> Meta }}. Empty path "" is the root level. */
    public Map<String, Map<String, SynxMeta>> metadata = new HashMap<>();
    public List<SynxIncludeDirective> includes = new ArrayList<>();
    public List<SynxUseDirective> uses = new ArrayList<>();

    public SynxParseResult() {}
}
