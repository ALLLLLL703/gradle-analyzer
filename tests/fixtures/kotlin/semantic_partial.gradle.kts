plugins {
    kotlin("jvm")
}

// An out-of-nucleus control-flow region that must degrade to OPAQUE and be skipped.
if (System.getenv("CI") != null) {
    println("ci")
}

dependencies {
    implementation()
    implementation("org.example:lib:1.0"
