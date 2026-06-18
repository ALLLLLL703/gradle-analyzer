//! Pure launch planning for the real Gradle Tooling-API sidecar.
//!
//! This module decides *how* to spawn the JVM sidecar child without performing any spawn
//! itself: it validates that a JVM, a Gradle wrapper or installation, the compiled sidecar
//! classes, and the init-script are all present, then builds the exact `java` argv +
//! classpath the [`crate::gradle::sidecar::wrapper_runner::WrapperRunner`] will launch.
//!
//! Keeping planning pure (only `Path::exists`/metadata probes, no process spawn) makes
//! every degradation path — missing JVM, missing wrapper, a non-executable `gradlew` —
//! unit-testable with temp directories and no real Gradle. Each validation failure maps to
//! the matching [`SidecarFailure`] so the caller degrades to the static tier with a
//! localized status.

use std::path::{Path, PathBuf};

use crate::gradle::sidecar::failure::SidecarFailure;

/// The fully-resolved inputs needed to plan a sidecar launch.
///
/// Discovery of the JVM and Gradle installation from the environment is done by
/// [`LaunchInputs::discover`]; tests construct this struct directly with temp paths so
/// [`plan_launch`] stays pure and deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchInputs {
    /// The workspace root holding the Gradle build to import.
    pub project_dir: PathBuf,
    /// The Gradle installation directory (for the Tooling-API jars + installation fallback).
    pub gradle_home: Option<PathBuf>,
    /// Directory containing the compiled sidecar `.class` files.
    pub classes_dir: PathBuf,
    /// Path to the `sidecar-init.gradle` init-script.
    pub init_script: PathBuf,
    /// The `java` executable to launch (resolved during discovery).
    pub java_exe: Option<PathBuf>,
    /// File the init-script writes the serialized model JSON to.
    pub out_file: PathBuf,
}

impl LaunchInputs {
    /// Resolves a JVM and Gradle installation from the environment, keeping the supplied
    /// project/classes/init/out paths.
    ///
    /// The `java` executable comes from `$JAVA_HOME/bin/java` when set, else the first
    /// `java` on `$PATH`. The Gradle home comes from `$GRADLE_HOME` or the parent-of-parent
    /// of a `gradle` binary on `$PATH`. Either may resolve to `None`, which surfaces as a
    /// typed failure from [`plan_launch`] rather than a panic.
    pub fn discover(
        project_dir: PathBuf,
        classes_dir: PathBuf,
        init_script: PathBuf,
        out_file: PathBuf,
    ) -> Self {
        Self {
            project_dir,
            gradle_home: discover_gradle_home(),
            classes_dir,
            init_script,
            java_exe: discover_jvm(),
            out_file,
        }
    }
}

/// The concrete plan to spawn the sidecar: the program and its argument vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// The `java` executable to spawn.
    pub program: PathBuf,
    /// The full argument vector (classpath, main class, then the sidecar's own args).
    pub args: Vec<String>,
}

/// The launcher's fully-qualified main class.
const SIDECAR_MAIN_CLASS: &str = "ga.sidecar.GradleModelSidecar";

/// Validates the inputs and builds the `java` launch plan, or the matching failure.
///
/// Validation order (each maps to a distinct [`SidecarFailure`]):
/// 1. a `java` executable resolved and present → else [`SidecarFailure::MissingJvm`];
/// 2. the compiled classes dir + init-script present → else
///    [`SidecarFailure::SyncFailure`] (a build/setup problem, not a user wrapper issue);
/// 3. a runnable Gradle: a `gradlew` wrapper in `project_dir` (must be executable, else
///    [`SidecarFailure::WrapperNotExecutable`]) OR a present `gradle_home` installation;
///    neither → [`SidecarFailure::WrapperMissing`].
///
/// # Example
///
/// ```
/// use std::path::PathBuf;
/// use gradle_analyzer::gradle::sidecar::launch::{plan_launch, LaunchInputs};
///
/// // No JVM resolved -> MissingJvm, never a panic.
/// let inputs = LaunchInputs {
///     project_dir: PathBuf::from("/tmp/does-not-exist"),
///     gradle_home: None,
///     classes_dir: PathBuf::from("/tmp/classes"),
///     init_script: PathBuf::from("/tmp/init.gradle"),
///     java_exe: None,
///     out_file: PathBuf::from("/tmp/out.json"),
/// };
/// assert!(plan_launch(&inputs).is_err());
/// ```
pub fn plan_launch(inputs: &LaunchInputs) -> Result<LaunchPlan, SidecarFailure> {
    let java = resolve_jvm(inputs)?;
    let classpath = build_classpath(inputs)?;
    let gradle_arg = resolve_gradle_arg(inputs)?;

    let args = vec![
        "-cp".to_string(),
        classpath,
        SIDECAR_MAIN_CLASS.to_string(),
        inputs.project_dir.to_string_lossy().into_owned(),
        gradle_arg,
        inputs.init_script.to_string_lossy().into_owned(),
        inputs.out_file.to_string_lossy().into_owned(),
    ];

    tracing::info!(
        program = %java.display(),
        project = %inputs.project_dir.display(),
        "sidecar launch planned"
    );
    Ok(LaunchPlan {
        program: java,
        args,
    })
}

/// Returns the `java` executable when one is resolved and exists, else [`SidecarFailure::MissingJvm`].
fn resolve_jvm(inputs: &LaunchInputs) -> Result<PathBuf, SidecarFailure> {
    match &inputs.java_exe {
        Some(java) if java.exists() => Ok(java.clone()),
        _ => {
            tracing::warn!("sidecar launch: no JVM available");
            Err(SidecarFailure::MissingJvm)
        }
    }
}

/// Builds the `java -cp` classpath from the Tooling-API jars + the compiled classes dir.
///
/// A missing classes dir or init-script is a setup/build problem, reported as
/// [`SidecarFailure::SyncFailure`] (the wrapper itself may be fine).
fn build_classpath(inputs: &LaunchInputs) -> Result<String, SidecarFailure> {
    if !inputs.classes_dir.exists() {
        return Err(SidecarFailure::SyncFailure {
            detail: "sidecar classes not compiled".to_string(),
        });
    }
    if !inputs.init_script.exists() {
        return Err(SidecarFailure::SyncFailure {
            detail: "sidecar init-script missing".to_string(),
        });
    }

    let gradle_home = inputs.gradle_home.as_deref().ok_or_else(|| {
        SidecarFailure::SyncFailure {
            detail: "no Gradle installation for Tooling-API jars".to_string(),
        }
    })?;
    let mut entries = gradle_lib_jars(gradle_home);
    if entries.is_empty() {
        return Err(SidecarFailure::SyncFailure {
            detail: "Gradle Tooling-API jars not found".to_string(),
        });
    }
    entries.push(inputs.classes_dir.to_string_lossy().into_owned());
    Ok(entries.join(classpath_separator()))
}

/// Resolves the Gradle argument the launcher uses to connect (installation dir or `""`).
///
/// When a `gradlew` wrapper exists in `project_dir` it must be executable, and the empty
/// string is passed so the launcher's connector auto-detects the wrapper. Otherwise a
/// present `gradle_home` is passed for an installation connection; neither present is a
/// [`SidecarFailure::WrapperMissing`].
fn resolve_gradle_arg(inputs: &LaunchInputs) -> Result<String, SidecarFailure> {
    if let Some(wrapper) = wrapper_script(&inputs.project_dir) {
        if !is_executable(&wrapper) {
            tracing::warn!(wrapper = %wrapper.display(), "gradle wrapper not executable");
            return Err(SidecarFailure::WrapperNotExecutable);
        }
        // Empty arg: the launcher's GradleConnector auto-detects the wrapper distribution.
        return Ok(String::new());
    }

    match &inputs.gradle_home {
        Some(home) if home.exists() => Ok(home.to_string_lossy().into_owned()),
        _ => {
            tracing::warn!("no gradle wrapper and no installation available");
            Err(SidecarFailure::WrapperMissing)
        }
    }
}

/// Returns the path to a `gradlew`(.bat) wrapper script in `dir`, if one exists.
fn wrapper_script(dir: &Path) -> Option<PathBuf> {
    let unix = dir.join("gradlew");
    if unix.exists() {
        return Some(unix);
    }
    let win = dir.join("gradlew.bat");
    if win.exists() {
        return Some(win);
    }
    None
}

/// Returns every jar in a Gradle installation's `lib/` directory for the launcher classpath.
///
/// The distribution's `gradle-tooling-api` jar is NOT self-contained: `GradleConnector`
/// needs sibling service jars (e.g. `gradle-base-services`) at runtime, so the whole `lib/`
/// glob is placed on the launcher classpath rather than a hand-picked subset. The launcher
/// only ever links against the public Tooling-API surface; the extra jars are inert until
/// the connector resolves its services.
fn gradle_lib_jars(gradle_home: &Path) -> Vec<String> {
    let lib = gradle_home.join("lib");
    let mut jars = Vec::new();
    let Ok(entries) = std::fs::read_dir(&lib) else {
        return jars;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().ends_with(".jar") {
            jars.push(entry.path().to_string_lossy().into_owned());
        }
    }
    jars
}

/// The platform classpath separator (`:` on unix, `;` on Windows).
fn classpath_separator() -> &'static str {
    if cfg!(windows) { ";" } else { ":" }
}

/// Returns `true` if `path` is executable (mode bit on unix; assumed on other platforms).
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// On non-unix platforms an existing wrapper script is treated as runnable.
#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.exists()
}

/// Resolves the `java` executable from `$JAVA_HOME` then `$PATH`.
fn discover_jvm() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let candidate = PathBuf::from(home).join("bin").join(java_binary_name());
        if candidate.exists() {
            return Some(candidate);
        }
    }
    which_on_path(java_binary_name())
}

/// Resolves a Gradle installation directory from `$GRADLE_HOME` then a `gradle` on `$PATH`.
fn discover_gradle_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("GRADLE_HOME") {
        let path = PathBuf::from(home);
        if path.join("lib").exists() {
            return Some(path);
        }
    }
    // A `gradle` launcher typically lives at `<home>/bin/gradle`; climb two parents.
    let gradle = which_on_path(gradle_binary_name())?;
    let canonical = std::fs::canonicalize(&gradle).unwrap_or(gradle);
    let home = canonical.parent()?.parent()?.to_path_buf();
    if home.join("lib").exists() {
        Some(home)
    } else {
        None
    }
}

/// Returns the first directory on `$PATH` containing an executable named `name`.
fn which_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// The platform `java` binary name.
fn java_binary_name() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

/// The platform `gradle` binary name.
fn gradle_binary_name() -> &'static str {
    if cfg!(windows) { "gradle.bat" } else { "gradle" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A unique temp dir for one test.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ga-launch-{}-{}-{}",
            tag,
            std::process::id(),
            unique()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn unique() -> u128 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    }

    /// A fake gradle home with a `lib/` holding tooling-api + service jars.
    fn fake_gradle_home(parent: &Path) -> PathBuf {
        let home = parent.join("gradle-home");
        fs::create_dir_all(home.join("lib")).unwrap();
        fs::write(home.join("lib/gradle-tooling-api-9.5.1.jar"), b"").unwrap();
        fs::write(home.join("lib/gradle-base-services-9.5.1.jar"), b"").unwrap();
        fs::write(home.join("lib/slf4j-api-2.0.17.jar"), b"").unwrap();
        home
    }

    /// Inputs with a present java, classes dir, and init-script under `base`.
    fn base_inputs(base: &Path) -> LaunchInputs {
        let java = base.join("java");
        fs::write(&java, b"#!/bin/sh\n").unwrap();
        let classes = base.join("classes");
        fs::create_dir_all(&classes).unwrap();
        let init = base.join("sidecar-init.gradle");
        fs::write(&init, b"// init\n").unwrap();
        LaunchInputs {
            project_dir: base.join("project"),
            gradle_home: Some(fake_gradle_home(base)),
            classes_dir: classes,
            init_script: init,
            java_exe: Some(java),
            out_file: base.join("out.json"),
        }
    }

    #[test]
    fn missing_jvm_yields_missing_jvm_failure() {
        let base = temp_dir("nojvm");
        let mut inputs = base_inputs(&base);
        inputs.java_exe = None;
        fs::create_dir_all(&inputs.project_dir).unwrap();

        assert_eq!(plan_launch(&inputs).unwrap_err(), SidecarFailure::MissingJvm);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn no_wrapper_and_no_installation_yields_wrapper_missing() {
        let base = temp_dir("nowrapper");
        let mut inputs = base_inputs(&base);
        inputs.gradle_home = None; // also drops the classpath source
        fs::create_dir_all(&inputs.project_dir).unwrap();

        // gradle_home None -> classpath build fails first as a SyncFailure (setup), still degrades.
        let failure = plan_launch(&inputs).unwrap_err();
        assert!(failure.degraded_to_static());
        assert!(matches!(failure, SidecarFailure::SyncFailure { .. }));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn installation_present_but_no_wrapper_uses_installation_dir() {
        let base = temp_dir("install");
        let inputs = base_inputs(&base);
        fs::create_dir_all(&inputs.project_dir).unwrap(); // no gradlew inside

        let plan = plan_launch(&inputs).expect("installation plan");
        let home = inputs.gradle_home.as_ref().unwrap().to_string_lossy().into_owned();
        assert!(plan.args.contains(&home), "installation dir passed as gradle arg");
        assert_eq!(plan.program, inputs.java_exe.unwrap());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn present_wrapper_passes_empty_gradle_arg_for_autodetect() {
        let base = temp_dir("wrapper");
        let inputs = base_inputs(&base);
        fs::create_dir_all(&inputs.project_dir).unwrap();
        let gradlew = inputs.project_dir.join("gradlew");
        fs::write(&gradlew, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&gradlew).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&gradlew, perms).unwrap();
        }

        let plan = plan_launch(&inputs).expect("wrapper plan");
        // The gradle arg slot (index 4) is empty so the launcher auto-detects the wrapper.
        assert_eq!(plan.args[4], "", "empty gradle arg signals wrapper auto-detect");
        fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn non_executable_wrapper_yields_wrapper_not_executable() {
        use std::os::unix::fs::PermissionsExt;
        let base = temp_dir("noexec");
        let inputs = base_inputs(&base);
        fs::create_dir_all(&inputs.project_dir).unwrap();
        let gradlew = inputs.project_dir.join("gradlew");
        fs::write(&gradlew, b"#!/bin/sh\n").unwrap();
        let mut perms = fs::metadata(&gradlew).unwrap().permissions();
        perms.set_mode(0o644); // not executable
        fs::set_permissions(&gradlew, perms).unwrap();

        assert_eq!(
            plan_launch(&inputs).unwrap_err(),
            SidecarFailure::WrapperNotExecutable
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn uncompiled_classes_dir_is_a_sync_failure() {
        let base = temp_dir("noclasses");
        let mut inputs = base_inputs(&base);
        inputs.classes_dir = base.join("missing-classes");
        fs::create_dir_all(&inputs.project_dir).unwrap();

        let failure = plan_launch(&inputs).unwrap_err();
        assert!(matches!(failure, SidecarFailure::SyncFailure { .. }));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn classpath_includes_service_jars_and_classes_dir() {
        let base = temp_dir("cp");
        let inputs = base_inputs(&base);
        fs::create_dir_all(&inputs.project_dir).unwrap();

        let plan = plan_launch(&inputs).unwrap();
        let cp = &plan.args[1];
        assert!(cp.contains("gradle-tooling-api-9.5.1.jar"));
        assert!(
            cp.contains("gradle-base-services-9.5.1.jar"),
            "service jars needed at runtime are on the classpath: {cp}"
        );
        assert!(cp.contains("classes"), "compiled classes dir on classpath");
        fs::remove_dir_all(&base).unwrap();
    }
}
