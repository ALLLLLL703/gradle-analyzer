package ga.sidecar;

import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;

/**
 * Minimal, dependency-free helpers for the line-delimited JSON sidecar protocol.
 *
 * <p>The launcher only ever emits two fixed frame shapes (a {@code ServerHello} and a
 * {@code SidecarResponse}) and embeds the model JSON produced by the Groovy init-script
 * verbatim, so a full JSON library is unnecessary. Only string escaping is needed for the
 * gradle-version fallback and error messages.
 */
final class Json {

    private Json() {
    }

    /** The fixed server handshake frame matching the Rust {@code ServerMessage::Hello} shape. */
    static String serverHello() {
        return "{\"type\":\"hello\",\"chosenVersion\":1,"
                + "\"capabilities\":[\"modelImport\",\"cancellation\",\"sourceJars\"]}";
    }

    /** Wraps an already-serialized model body in the {@code ServerMessage::Response} envelope. */
    static String modelResponse(String modelJson) {
        return "{\"type\":\"response\",\"id\":1,"
                + "\"outcome\":{\"status\":\"model\",\"body\":" + modelJson + "}}";
    }

    /** Builds an error response frame the Rust client maps to a degraded SyncFailure. */
    static String errorResponse(String code, String message) {
        return "{\"type\":\"response\",\"id\":1,\"outcome\":{\"status\":\"error\",\"body\":{"
                + "\"code\":" + quote(code) + ",\"message\":" + quote(message) + "}}}";
    }

    /** A minimal model carrying only the gradle version (the init-script fallback). */
    static String minimalModel(String gradleVersion) {
        return "{\"gradleVersion\":" + quote(gradleVersion)
                + ",\"appliedPlugins\":[],\"extensions\":[],\"taskTypes\":[],"
                + "\"classpathJars\":[],\"sourceJars\":[],"
                + "\"versionCatalog\":{\"libraries\":{},\"bundles\":{},\"versions\":{},\"plugins\":{}}}";
    }

    /** Returns a JSON-quoted, escaped string literal for `value`. */
    static String quote(String value) {
        StringBuilder out = new StringBuilder(value.length() + 2);
        out.append('"');
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (c < 0x20) {
                        out.append(String.format("\\u%04x", (int) c));
                    } else {
                        out.append(c);
                    }
                }
            }
        }
        out.append('"');
        return out.toString();
    }

    /**
     * Returns a UTF-8 PrintStream bound to the real stdout file descriptor.
     *
     * <p>Captured before the Tooling API can redirect {@code System.out}, this is the clean
     * protocol channel; the daemon's own output is routed to stderr by the launcher.
     */
    static PrintStream protocolChannel() {
        return new PrintStream(new FileOutputStream(FileDescriptor.out), true, StandardCharsets.UTF_8);
    }
}
