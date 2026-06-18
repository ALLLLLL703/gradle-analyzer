// A messy-but-valid build script: comments, blank lines, trailing commas, string templates.

plugins {
    kotlin("jvm") version "1.9.22"

    /* block comment between entries */
    application
}


group = "com.example"      // trailing line comment
version = "0.0.1-SNAPSHOT"

val greeting = "Hello, ${System.getProperty("user.name")}!"

repositories {
    mavenCentral()
}

dependencies {
    implementation("a:b:1.0")
    implementation("c:d:2.0",)
    testImplementation(
        "e:f:3.0",
        "g:h:4.0",
    )
}

extra["buildNumber"] = "42"
