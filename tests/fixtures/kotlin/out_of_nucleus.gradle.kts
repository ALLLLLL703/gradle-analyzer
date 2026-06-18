plugins {
    kotlin("jvm")
}

// A top-level control-flow / declaration block that is OUT of the supported nucleus.
// It must degrade to a single OPAQUE node WITHOUT errors, and parsing must continue.
if (System.getenv("CI") != null) {
    println("running in CI")
} else {
    println("local build")
}

fun computeVersion(base: String): String {
    return base + "-local"
}

// Surrounding nucleus blocks after the opaque region must still parse cleanly.
repositories {
    mavenCentral()
}

dependencies {
    implementation("org.example:lib:1.0")
}
