#!/usr/bin/env bash
# build-sidecar.sh — compile the Gradle model sidecar Java sources against the Tooling-API jar.
#
# Resolves a Gradle installation (GRADLE_HOME, else the parent-of-parent of a `gradle` on
# PATH), finds gradle-tooling-api-*.jar in its lib/, and compiles ga/sidecar/*.java into a
# classes dir. Usage:
#
#   build-sidecar.sh [OUT_CLASSES_DIR]
#
# OUT_CLASSES_DIR defaults to sidecar-jvm/build/classes. Prints the classes dir on success.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out_dir="${1:-$script_dir/build/classes}"

# --- locate a Gradle installation for the Tooling-API jar ---
gradle_home="${GRADLE_HOME:-}"
if [[ -z "$gradle_home" ]]; then
  if command -v gradle >/dev/null 2>&1; then
    gradle_bin="$(command -v gradle)"
    gradle_bin="$(readlink -f "$gradle_bin")"
    gradle_home="$(dirname "$(dirname "$gradle_bin")")"
  fi
fi
if [[ -z "$gradle_home" || ! -d "$gradle_home/lib" ]]; then
  echo "build-sidecar: no Gradle installation found (set GRADLE_HOME)" >&2
  exit 2
fi

tooling_jar="$(find "$gradle_home/lib" -maxdepth 1 -name 'gradle-tooling-api-*.jar' \
  ! -name '*provider*' ! -name '*builders*' | head -n1)"
if [[ -z "$tooling_jar" ]]; then
  echo "build-sidecar: gradle-tooling-api jar not found under $gradle_home/lib" >&2
  exit 2
fi

# --- compile ---
# Compile against the FULL lib glob: BuildController's type-annotations (jspecify Nullable,
# org.gradle.api.Action, ...) live in sibling jars and javac must resolve them while reading
# the class. The RUNTIME classpath (see launch.rs) needs only the self-contained tooling-api
# jar + an slf4j binding.
compile_cp="$(find "$gradle_home/lib" -maxdepth 1 -name '*.jar' | tr '\n' ':')"
mkdir -p "$out_dir"
javac -cp "$compile_cp" -d "$out_dir" "$script_dir"/src/ga/sidecar/*.java

echo "$out_dir"
