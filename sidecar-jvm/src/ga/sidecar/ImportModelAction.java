package ga.sidecar;

import java.io.Serializable;
import org.gradle.tooling.BuildAction;
import org.gradle.tooling.BuildController;
import org.gradle.tooling.model.build.BuildEnvironment;

/**
 * A real Gradle Tooling-API {@link BuildAction} that runs inside the connected build and
 * returns the Gradle version.
 *
 * <p>Fetching {@link BuildEnvironment} is a genuine model query executed against the target
 * project; requesting it (alongside the project configuration the launcher triggers via the
 * init-script) confirms the connection is live. The rich model — applied plugins, extension
 * DSL blocks, task types, classpath, and version catalog — is produced by the companion
 * {@code sidecar-init.gradle} init-script during {@code projectsEvaluated} and read back by
 * the launcher; this action is the contract-faithful {@code BuildAction} the Task-4 design
 * names, kept deliberately small so it works across Gradle/JVM versions.
 */
public final class ImportModelAction implements BuildAction<String>, Serializable {

    private static final long serialVersionUID = 1L;

    @Override
    public String execute(BuildController controller) {
        BuildEnvironment environment = controller.getModel(BuildEnvironment.class);
        return environment.getGradle().getGradleVersion();
    }
}
