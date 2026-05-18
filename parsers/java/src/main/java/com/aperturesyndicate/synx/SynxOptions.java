package com.aperturesyndicate.synx;

import java.util.HashMap;
import java.util.Map;

/** Options for {@code !active} mode resolution. Default-constructed value is fine for static parsing. */
public final class SynxOptions {
    public Map<String, String> env;     // nullable
    public String region;                // nullable
    public String lang;                  // nullable
    public String basePath;              // nullable — defaults to "." at resolve time
    public Integer maxIncludeDepth;      // nullable — defaults to 16
    public String packagesPath;          // nullable — defaults to "./synx_packages"
    public boolean strict;
    public Map<String, SynxMarkerFn> markerFns = new HashMap<>();
    /** Internal counter — do not set manually. */
    public int includeDepth;

    public SynxOptions() {}
}
