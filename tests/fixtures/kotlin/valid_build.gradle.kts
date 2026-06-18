import org.gradle.api.tasks.testing.Test

plugins {
    `kotlin-dsl`
    kotlin("jvm") version "1.9.22"
    id("com.diffplug.spotless") version "6.25.0"
    application
}

group = "com.example.demo"
version = "1.2.3"

repositories {
    mavenCentral()
    gradlePluginPortal()
    maven {
        url = uri("https://example.com/repo")
    }
}

dependencies {
    implementation("org.jetbrains.kotlin:kotlin-stdlib:1.9.22")
    implementation(libs.guava)
    implementation(libs.bundles.networking)
    testImplementation("org.junit.jupiter:junit-jupiter:5.10.1")
    api(project(":core"))
}

tasks.register<Test>("integrationTest") {
    description = "Runs integration tests."
    useJUnitPlatform()
}

tasks.named<Test>("test") {
    maxParallelForks = 4
}
