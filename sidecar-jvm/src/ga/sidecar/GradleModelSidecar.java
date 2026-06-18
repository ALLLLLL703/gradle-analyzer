package ga.sidecar;

import java.io.BufferedReader;
import java.io.File;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import org.gradle.tooling.BuildActionExecuter;
import org.gradle.tooling.GradleConnector;
import org.gradle.tooling.ProjectConnection;

/**
 * The Gradle model sidecar launcher: speaks the line-delimited JSON protocol over stdio and
 * drives the Tooling API to import a project model.
 *
 * <p>Protocol (matching the Rust Task-4 contract): read a {@code ClientHello} line, write a
 * {@code ServerHello}, read the {@code ImportModel} request, run the Tooling-API
 * {@link ImportModelAction} with the {@code sidecar-init.gradle} init-script (which writes the
 * rich model JSON to the out-file), then write a {@code SidecarResponse} carrying that model.
 *
 * <p>The Gradle daemon's own stdout/stderr is routed to <em>stderr</em> so the stdout protocol
 * channel carries only framed JSON; the launcher captures the real stdout descriptor up front.
 *
 * <p>Args: {@code <projectDir> <gradleArgOrEmpty> <initScript> <outFile>}. An empty
 * {@code gradleArg} lets the connector auto-detect the project's wrapper; otherwise it names a
 * Gradle installation directory.
 */
public final class GradleModelSidecar {

    private GradleModelSidecar() {
    }

    public static void main(String[] args) {
        PrintStream protocol = Json.protocolChannel();
        // Redirect the default streams to stderr so Tooling-API noise never hits the
        // protocol channel (stdout).
        System.setOut(System.err);

        if (args.length < 4) {
            protocol.println(Json.errorResponse("syncFailure", "sidecar requires 4 args"));
            protocol.flush();
            return;
        }
        File projectDir = new File(args[0]);
        String gradleArg = args[1];
        String initScript = args[2];
        String outFile = args[3];

        try {
            BufferedReader stdin =
                    new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));

            // Handshake: consume the client hello, answer with the server hello.
            stdin.readLine();
            protocol.println(Json.serverHello());
            protocol.flush();

            // Consume the model-import request before doing the (slow) sync.
            stdin.readLine();

            String response = runImport(projectDir, gradleArg, initScript, outFile);
            protocol.println(response);
            protocol.flush();
        } catch (Exception e) {
            protocol.println(Json.errorResponse("syncFailure", String.valueOf(e.getMessage())));
            protocol.flush();
        }
    }

    /** Connects, runs the build action + init-script, and returns the response frame. */
    private static String runImport(
            File projectDir, String gradleArg, String initScript, String outFile) {
        GradleConnector connector = GradleConnector.newConnector().forProjectDirectory(projectDir);
        if (gradleArg.isEmpty()) {
            connector.useBuildDistribution();
        } else {
            connector.useInstallation(new File(gradleArg));
        }

        try (ProjectConnection connection = connector.connect()) {
            BuildActionExecuter<String> executer = connection.action(new ImportModelAction());
            executer.withArguments("--init-script", initScript, "-Pga_sidecar_out=" + outFile);
            executer.setStandardOutput(System.err);
            executer.setStandardError(System.err);
            String gradleVersion = executer.run();

            String model = readModel(outFile);
            if (model == null) {
                model = Json.minimalModel(gradleVersion);
            }
            return Json.modelResponse(model);
        }
    }

    /** Reads the init-script's model JSON, or {@code null} if absent/unreadable/blank. */
    private static String readModel(String outFile) {
        try {
            Path path = Path.of(outFile);
            if (!Files.exists(path)) {
                return null;
            }
            String content = Files.readString(path, StandardCharsets.UTF_8).trim();
            return content.isEmpty() ? null : content;
        } catch (Exception e) {
            return null;
        }
    }
}
