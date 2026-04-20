import java.util.Properties

plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.android)
    kotlin("plugin.serialization")
    id("maven-publish")
    id("signing")
}

android {
    namespace = "xyz.pollinet.sdk"
    compileSdk = 36

    defaultConfig {
        minSdk = 28
        
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("consumer-rules.pro")

        // Configure native library directories
        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
    
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
    
    kotlinOptions {
        jvmTarget = "11"
    }

    sourceSets {
        getByName("main") {
            // Point to the Rust-generated JNI libraries
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }
    
    lint {
        // Don't abort build on lint errors (can fix later)
        abortOnError = false
        // Treat warnings as errors (optional, can remove if too strict)
        warningsAsErrors = false
    }
    
    publishing {
        singleVariant("release") {
            withSourcesJar()
            withJavadocJar()
        }
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.appcompat)
    implementation(libs.material)
    
    // Coroutines for async operations
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.8.1")
    
    // JSON parsing
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
    
    // WorkManager for battery-efficient background tasks (Phase 4)
    implementation("androidx.work:work-runtime-ktx:2.9.0")
    
    // UniFFI runtime (we'll use JNA for now as it's simpler than full UniFFI)
    implementation("net.java.dev.jna:jna:5.14.0@aar")
    
    testImplementation(libs.junit)
    androidTestImplementation(libs.androidx.junit)
    androidTestImplementation(libs.androidx.espresso.core)
}

// Resolve ANDROID_NDK_HOME at configuration time:
// 1. Use the env var if already set (CI / Android Studio)
// 2. Otherwise derive it from sdk.dir in local.properties (any ancestor project)
val resolvedNdkHome: String? = run {
    val envNdk = System.getenv("ANDROID_NDK_HOME")
    if (!envNdk.isNullOrBlank()) return@run envNdk

    val candidateLocalProps = listOf(
        file("../../local.properties"),      // pollinet repo root
        file("../local.properties"),         // pollinet-sdk root
        rootProject.file("local.properties") // consumer project root (e.g. Pollistem)
    )
    var sdkDir: String? = null
    for (f in candidateLocalProps) {
        if (f.exists()) {
            val props = Properties()
            f.inputStream().use { props.load(it) }
            sdkDir = props.getProperty("sdk.dir")
            if (sdkDir != null) break
        }
    }
    if (sdkDir == null) return@run null

    val ndkParent = File("$sdkDir/ndk")
    if (!ndkParent.isDirectory) return@run null
    // Pick the newest installed NDK version
    ndkParent.listFiles()
        ?.filter { it.isDirectory }
        ?.maxByOrNull { it.name }
        ?.absolutePath
}

// Task to build Rust library using cargo-ndk.
// Skipped automatically when:
//   - SKIP_RUST_BUILD=true is set (composite/incremental builds where .so files are pre-built), OR
//   - libpollinet.so already exists for all three ABIs (avoids heavy Rust recompile on every build)
// Run `./gradlew buildRustLib` explicitly to force a rebuild.
tasks.register<Exec>("buildRustLib") {
    description = "Build Rust library for Android using cargo-ndk"
    group = "build"

    workingDir = file("../../")

    val cargoHome = System.getenv("CARGO_HOME") ?: "${System.getProperty("user.home")}/.cargo"
    val cargoPath = "$cargoHome/bin/cargo"

    if (resolvedNdkHome != null) {
        environment("ANDROID_NDK_HOME", resolvedNdkHome)
    }

    commandLine(
        cargoPath, "ndk",
        "-t", "arm64-v8a",
        "-t", "armeabi-v7a",
        "-t", "x86_64",
        "-o", "pollinet-sdk/pollinet-sdk/src/main/jniLibs",
        "build",
        "--release",
        "--no-default-features",
        "--features", "android"
    )

    // Skip if all pre-built .so files are already in place, or SKIP_RUST_BUILD=true.
    // This keeps composite/incremental builds fast — run `./gradlew buildRustLib`
    // explicitly whenever the Rust source changes.
    onlyIf {
        val skipEnv = System.getenv("SKIP_RUST_BUILD") == "true"
        val arm64So   = file("pollinet-sdk/pollinet-sdk/src/main/jniLibs/arm64-v8a/libpollinet.so")
        val armv7So   = file("pollinet-sdk/pollinet-sdk/src/main/jniLibs/armeabi-v7a/libpollinet.so")
        val x86_64So  = file("pollinet-sdk/pollinet-sdk/src/main/jniLibs/x86_64/libpollinet.so")
        val alreadyBuilt = arm64So.exists() && armv7So.exists() && x86_64So.exists()
        !skipEnv && !alreadyBuilt
    }
}

// Make preBuild depend on buildRustLib
tasks.named("preBuild") {
    dependsOn("buildRustLib")
}

// Version - read from VERSION file or Cargo.toml or set directly
val sdkVersion = file("../../VERSION").takeIf { it.exists() }?.readText()?.trim()
    ?: file("../../Cargo.toml").takeIf { it.exists() }?.readText()?.let {
        Regex("version = \"([^\"]+)\"").find(it)?.groupValues?.get(1)
    }
    ?: project.findProperty("sdk.version") as String?
    ?: "0.1.0"

version = sdkVersion

// Publishing configuration
publishing {
    publications {
        create<MavenPublication>("release") {
            groupId = "xyz.pollinet"
            artifactId = "pollinet-sdk"
            version = sdkVersion
            
            afterEvaluate {
                from(components["release"])
            }
            
            pom {
                name.set("Pollinet SDK")
                description.set("Offline Solana transaction propagation over BLE mesh networks")
                url.set("https://github.com/pollinet/pollinet")
                
                licenses {
                    license {
                        name.set("Apache-2.0")
                        url.set("https://www.apache.org/licenses/LICENSE-2.0.txt")
                    }
                }
                
                developers {
                    developer {
                        id.set("pollinet")
                        name.set("Pollinet Team")
                        email.set("team@pollinet.xyz")
                    }
                }
                
                scm {
                    connection.set("scm:git:git://github.com/pollinet/pollinet.git")
                    developerConnection.set("scm:git:ssh://github.com/pollinet/pollinet.git")
                    url.set("https://github.com/pollinet/pollinet")
                }
            }
        }
    }
    
    repositories {
        // Maven Central (Sonatype OSSRH) - Recommended for production
        maven {
            name = "OSSRH"
            url = uri("https://s01.oss.sonatype.org/service/local/staging/deploy/maven2/")
            credentials {
                username = project.findProperty("ossrhUsername") as String?
                password = project.findProperty("ossrhPassword") as String?
            }
        }
        
        // GitHub Packages - Alternative/backup option
        maven {
            name = "GitHubPackages"
            url = uri("https://maven.pkg.github.com/pollinet/pollinet")
            credentials {
                username = project.findProperty("gpr.user") as String? ?: System.getenv("GITHUB_ACTOR")
                password = project.findProperty("gpr.token") as String? ?: System.getenv("GITHUB_TOKEN")
            }
        }
    }
}

// Signing configuration — only applied when keys are explicitly provided.
// JitPack does not have a GPG keyring, so signing is skipped there.
// Maven Central publishing requires signing; pass keys via gradle.properties or env vars.
val signingKeyId  = project.findProperty("signing.keyId")   as String?
    ?: project.findProperty("signingKeyId")                  as String?
val signingKey      = project.findProperty("signingKey")      as String?
val signingPassword = project.findProperty("signing.password") as String?
    ?: project.findProperty("signingPassword")               as String?

if (signingKeyId != null && signingKey != null && signingPassword != null) {
    signing {
        useInMemoryPgpKeys(signingKeyId, signingKey, signingPassword)
        sign(publishing.publications["release"])
    }
}
// No else — if keys are absent (JitPack, forks, fresh checkouts) the AAR is
// published unsigned. Signing is only required for Sonatype/Maven Central.
