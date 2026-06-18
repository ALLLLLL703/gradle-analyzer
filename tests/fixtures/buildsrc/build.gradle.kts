plugins {
    `kotlin-dsl`
}

tasks.register("buildSrcHello") {
    doLast {
        println("from buildSrc")
    }
}
