package com.aperturesyndicate.synx;

import java.util.List;

/**
 * User-supplied custom marker. Receives the field key, the parsed argument list,
 * and the value currently on the field; returns the resolved value.
 *
 * <p>Builtin markers always win over a custom marker with the same name —
 * use a different name when extending the format.
 */
@FunctionalInterface
public interface SynxMarkerFn {
    SynxValue apply(String key, List<String> args, SynxValue value);
}
