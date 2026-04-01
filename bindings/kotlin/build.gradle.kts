plugins {
    kotlin("jvm") version "2.0.21"
    `maven-publish`
}

group = "com.aperturesyndicate"
version = "3.6.0"

repositories {
    mavenCentral()
}

dependencies {
    implementation("net.java.dev.jna:jna:5.15.0")
    testImplementation(kotlin("test"))
    testImplementation(kotlin("test-junit5"))
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

kotlin {
    jvmToolchain(17)
}

tasks.test {
    useJUnitPlatform()
    val defaultLib = rootProject.projectDir.resolve("../../target/release").absolutePath
    environment(
        "SYNX_LIB_DIR",
        System.getenv("SYNX_LIB_DIR") ?: defaultLib
    )
}

publishing {
    publications {
        register<MavenPublication>("maven") {
            groupId = project.group.toString()
            artifactId = "synx-kotlin"
            version = project.version.toString()
            from(components["java"])
        }
    }
}
