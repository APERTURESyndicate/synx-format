package com.aperturesyndicate.synx;

import java.math.BigDecimal;
import java.math.MathContext;
import java.math.RoundingMode;

/**
 * Number text and number parsing exactly as {@code crates/synx-core} does them: Rust's
 * {@code str::parse} grammars, f64 {@code Display}, the {@code ryu} layout of the JSON writer,
 * {@code f as i64}, and the engine's {@code format_number}.
 */
final class SynxNumbers {

    private SynxNumbers() {}

    /** {@code i64::MAX as f64} (2^63). */
    static final double I64_MAX_F = 9223372036854775807.0;

    private static final boolean SHORTEST_TO_STRING = Runtime.version().feature() >= 19;

    // ─── parsing ────────────────────────────────────────────────────────────

    /**
     * Rust {@code str::parse::<f64>}: optional sign, digits with an optional fraction and exponent,
     * or {@code inf} / {@code infinity} / {@code nan} in any case. No whitespace, no hex, no
     * {@code d}/{@code f} suffix (all of which {@link Double#parseDouble} would take).
     * Returns {@code null} when the text is not a float.
     */
    static Double parseF64(String s) {
        int n = s.length();
        if (n == 0) return null;
        int i = 0;
        boolean neg = false;
        char c0 = s.charAt(0);
        if (c0 == '+' || c0 == '-') {
            neg = c0 == '-';
            i = 1;
        }
        if (i >= n) return null;
        String body = s.substring(i);
        if (body.equalsIgnoreCase("inf") || body.equalsIgnoreCase("infinity")) {
            return neg ? Double.NEGATIVE_INFINITY : Double.POSITIVE_INFINITY;
        }
        if (body.equalsIgnoreCase("nan")) return Double.NaN;
        int digits = 0;
        int j = i;
        while (j < n && isDigit(s.charAt(j))) { j++; digits++; }
        if (j < n && s.charAt(j) == '.') {
            j++;
            while (j < n && isDigit(s.charAt(j))) { j++; digits++; }
        }
        if (digits == 0) return null;
        if (j < n && (s.charAt(j) == 'e' || s.charAt(j) == 'E')) {
            j++;
            if (j < n && (s.charAt(j) == '+' || s.charAt(j) == '-')) j++;
            int exp = 0;
            while (j < n && isDigit(s.charAt(j))) { j++; exp++; }
            if (exp == 0) return null;
        }
        if (j != n) return null;
        return Double.parseDouble(s);
    }

    /** Rust {@code str::parse::<i64>}: optional sign and ASCII digits only. */
    static Long parseI64(String s) {
        int n = s.length();
        int i = 0;
        if (n > 0 && (s.charAt(0) == '+' || s.charAt(0) == '-')) i = 1;
        if (i >= n) return null;
        for (int j = i; j < n; j++) if (!isDigit(s.charAt(j))) return null;
        try {
            return Long.parseLong(s);
        } catch (NumberFormatException e) {
            return null;
        }
    }

    /**
     * Rust {@code str::parse::<u64>}: one optional {@code +}, ASCII digits. The result is an
     * unsigned 64-bit value held in a long ({@link Long#compareUnsigned} to compare).
     */
    static Long parseU64(String s) {
        int n = s.length();
        int i = 0;
        if (n > 0 && s.charAt(0) == '+') i = 1;
        if (i >= n) return null;
        for (int j = i; j < n; j++) if (!isDigit(s.charAt(j))) return null;
        try {
            return Long.parseUnsignedLong(s.substring(i));
        } catch (NumberFormatException e) {
            return null;
        }
    }

    static boolean isDigit(char c) { return c >= '0' && c <= '9'; }

    // ─── conversions ────────────────────────────────────────────────────────

    /** Rust {@code f as i64}: truncation toward zero, saturating, NaN → 0. */
    static long toI64(double f) {
        if (Double.isNaN(f)) return 0;
        if (f >= I64_MAX_F) return Long.MAX_VALUE;
        if (f <= -I64_MAX_F) return Long.MIN_VALUE;
        return (long) f;
    }

    /** Rust {@code f.fract() == 0.0}. */
    static boolean isWhole(double f) {
        return !Double.isInfinite(f) && !Double.isNaN(f) && f == Math.rint(f);
    }

    /** The engine's number text: a whole float within i64 prints as an integer. */
    static String formatNumber(double n) {
        if (isWhole(n) && Math.abs(n) < I64_MAX_F) return Long.toString(toI64(n));
        return display(n);
    }

    /** A computed float as Rust stores it: an Int when whole and within i64, else a Float. */
    static SynxValue numberValue(double f) {
        if (isWhole(f) && Math.abs(f) < I64_MAX_F) return SynxValue.ofInt(toI64(f));
        return SynxValue.ofFloat(f);
    }

    // ─── text ───────────────────────────────────────────────────────────────

    /** Shortest round-trip digits of a finite, non-zero |d|: value = 0.DIGITS × 10^point. */
    record Shortest(String digits, int point) {}

    static Shortest shortest(double abs) {
        Shortest s = SHORTEST_TO_STRING ? fromToString(abs) : search(abs);
        if (s.digits.length() == 2) {
            // Double.toString never prints one significant digit; the reference does when one
            // round-trips (a subnormal such as 5e-324).
            Shortest one = oneDigit(abs, s);
            if (one != null) return one;
        }
        return s;
    }

    private static Shortest fromToString(double abs) {
        String s = Double.toString(abs);
        int e = s.indexOf('E');
        String mant = e < 0 ? s : s.substring(0, e);
        int exp = e < 0 ? 0 : Integer.parseInt(s.substring(e + 1));
        int dot = mant.indexOf('.');
        String intPart = dot < 0 ? mant : mant.substring(0, dot);
        String frac = dot < 0 ? "" : mant.substring(dot + 1);
        return normalize(intPart + frac, intPart.length() + exp);
    }

    private static Shortest normalize(String digits, int point) {
        int lead = 0;
        while (lead < digits.length() - 1 && digits.charAt(lead) == '0') lead++;
        int end = digits.length();
        while (end > lead + 1 && digits.charAt(end - 1) == '0') end--;
        return new Shortest(digits.substring(lead, end), point - lead);
    }

    /** Exact shortest search for runtimes whose Double.toString is not shortest (before JDK 19). */
    private static Shortest search(double abs) {
        BigDecimal exact = new BigDecimal(abs);
        for (int p = 1; p <= 17; p++) {
            BigDecimal pick = pick(exact, abs, p);
            if (pick != null) return fromBig(pick);
        }
        return fromToString(abs);
    }

    private static Shortest oneDigit(double abs, Shortest two) {
        BigDecimal pick = pick(new BigDecimal(abs), abs, 1);
        return pick == null ? null : fromBig(pick);
    }

    /** The p-digit decimal nearest the exact value that still reads back as {@code abs}. */
    private static BigDecimal pick(BigDecimal exact, double abs, int p) {
        BigDecimal lo = exact.round(new MathContext(p, RoundingMode.FLOOR));
        BigDecimal hi = exact.round(new MathContext(p, RoundingMode.CEILING));
        boolean loOk = lo.doubleValue() == abs;
        boolean hiOk = hi.doubleValue() == abs;
        if (loOk && hiOk) {
            int cmp = exact.subtract(lo).compareTo(hi.subtract(exact));
            if (cmp < 0) return lo;
            if (cmp > 0) return hi;
            return lo.unscaledValue().testBit(0) ? hi : lo;
        }
        if (loOk) return lo;
        if (hiOk) return hi;
        return null;
    }

    private static Shortest fromBig(BigDecimal b) {
        BigDecimal s = b.stripTrailingZeros();
        String digits = s.unscaledValue().toString();
        return new Shortest(digits, digits.length() - s.scale());
    }

    /** Rust f64 {@code Display}: plain decimal, never an exponent; {@code NaN}, {@code inf}. */
    static String display(double d) {
        if (Double.isNaN(d)) return "NaN";
        if (Double.isInfinite(d)) return d > 0 ? "inf" : "-inf";
        if (d == 0) return (1 / d < 0) ? "-0" : "0";
        Shortest s = shortest(Math.abs(d));
        StringBuilder sb = new StringBuilder(24);
        if (d < 0) sb.append('-');
        String digits = s.digits;
        int kk = s.point;
        if (kk <= 0) {
            sb.append("0.");
            for (int i = 0; i < -kk; i++) sb.append('0');
            sb.append(digits);
        } else if (kk >= digits.length()) {
            sb.append(digits);
            for (int i = digits.length(); i < kk; i++) sb.append('0');
        } else {
            sb.append(digits, 0, kk).append('.').append(digits, kk, digits.length());
        }
        return sb.toString();
    }

    /**
     * The layout of Rust's {@code ryu} (what the reference {@code to_json} emits): plain decimals
     * while the decimal exponent stays within [-5, 16), else {@code d.ddde±N}, and a mandatory
     * {@code .0} on whole numbers.
     */
    static String ryu(double d) {
        if (d == 0) return (1 / d < 0) ? "-0.0" : "0.0";
        Shortest s = shortest(Math.abs(d));
        String digits = s.digits;
        int length = digits.length();
        int kk = s.point;
        int k = kk - length;
        StringBuilder sb = new StringBuilder(24);
        if (d < 0) sb.append('-');
        if (k >= 0 && kk <= 16) {
            sb.append(digits);
            for (int i = 0; i < k; i++) sb.append('0');
            sb.append(".0");
        } else if (kk > 0 && kk <= 16) {
            sb.append(digits, 0, kk).append('.').append(digits, kk, length);
        } else if (kk > -5 && kk <= 0) {
            sb.append("0.");
            for (int i = 0; i < -kk; i++) sb.append('0');
            sb.append(digits);
        } else if (length == 1) {
            sb.append(digits).append('e').append(kk - 1);
        } else {
            sb.append(digits.charAt(0)).append('.').append(digits, 1, length).append('e').append(kk - 1);
        }
        return sb.toString();
    }

    /** Rust {@code {:.prec$}}: exact decimal expansion, ties to even, sign kept on a rounded zero. */
    static String fixed(double d, int prec) {
        String special = nonFinite(d);
        if (special != null) return special;
        String s = new BigDecimal(d).setScale(prec, RoundingMode.HALF_EVEN).toPlainString();
        if ((d < 0 || (d == 0 && 1 / d < 0)) && !s.startsWith("-")) s = "-" + s;
        return s;
    }

    /** Rust {@code {:.prec$e}}: {@code 1.23e3}, {@code 1.00e-3}, {@code 0.00e0}. */
    static String lowerExp(double d, int prec) {
        String special = nonFinite(d);
        if (special != null) return special;
        boolean neg = d < 0 || (d == 0 && 1 / d < 0);
        StringBuilder sb = new StringBuilder(prec + 8);
        if (neg) sb.append('-');
        String digits;
        int exp;
        if (d == 0) {
            digits = "0".repeat(prec + 1);
            exp = 0;
        } else {
            BigDecimal r = new BigDecimal(Math.abs(d)).round(new MathContext(prec + 1, RoundingMode.HALF_EVEN));
            String unscaled = r.unscaledValue().toString();
            exp = unscaled.length() - 1 - r.scale();
            if (unscaled.length() < prec + 1) {
                unscaled = unscaled + "0".repeat(prec + 1 - unscaled.length());
            } else if (unscaled.length() > prec + 1) {
                unscaled = unscaled.substring(0, prec + 1);
            }
            digits = unscaled;
        }
        sb.append(digits.charAt(0));
        if (prec > 0) sb.append('.').append(digits, 1, prec + 1);
        sb.append('e').append(exp);
        return sb.toString();
    }

    /** {@code %e}: shortest digits, signed exponent without leading zeros ({@code 1.2345e+3}). */
    static String jsExponential(double d) {
        String special = nonFinite(d);
        if (special != null) return special;
        Shortest s = shortest(Math.abs(d));
        StringBuilder sb = new StringBuilder(24);
        if (d < 0) sb.append('-');
        sb.append(s.digits.charAt(0));
        if (s.digits.length() > 1) sb.append('.').append(s.digits, 1, s.digits.length());
        int exp = s.point - 1;
        sb.append('e').append(exp < 0 ? '-' : '+').append(Math.abs(exp));
        return sb.toString();
    }

    private static String nonFinite(double d) {
        if (Double.isNaN(d)) return "NaN";
        if (Double.isInfinite(d)) return d > 0 ? "inf" : "-inf";
        return null;
    }
}
