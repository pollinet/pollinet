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

// Check if native libraries already exist
val jniLibsDir = file("src/main/jniLibs")
val hasNativeLibs = jniLibsDir.exists() && 
    file("$jniLibsDir/arm64-v8a").exists() &&
    file("$jniLibsDir/armeabi-v7a").exists() &&
    file("$jniLibsDir/x86_64").exists()

// Task to build Rust library using cargo-ndk (only if libraries don't exist)
tasks.register<Exec>("buildRustLib") {
    description = "Build Rust library for Android using cargo-ndk"
    group = "build"
    enabled = !hasNativeLibs
    
    workingDir = file("../../")
    
    // Use absolute path to cargo
    val cargoHome = System.getenv("CARGO_HOME") ?: "${System.getProperty("user.home")}/.cargo"
    val cargoPath = "$cargoHome/bin/cargo"
    
    commandLine(
        cargoPath, "ndk",
        "-t", "arm64-v8a",
        "-t", "armeabi-v7a", 
        "-t", "x86_64",
        "-o", "pollinet-android/pollinet-sdk/src/main/jniLibs",
        "build",
        "--release",
        "--no-default-features",
        "--features", "android"
    )
    
    doFirst {
        if (hasNativeLibs) {
            logger.info("Native libraries already exist, skipping Rust build")
        }
    }
}

// Make preBuild depend on buildRustLib only if libraries don't exist
tasks.named("preBuild") {
    if (!hasNativeLibs) {
        dependsOn("buildRustLib")
    } else {
        logger.info("Using pre-built native libraries, skipping Rust build")
    }
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

// Signing configuration (required for Maven Central)
signing {
    val signingKeyId = project.findProperty("signing.keyId") as String? 
        ?: project.findProperty("signingKeyId") as String?
    val signingKey = project.findProperty("signingKey") as String?
    val signingPassword = project.findProperty("signing.password") as String?
        ?: project.findProperty("signingPassword") as String?
    
    // Try in-memory keys first (for CI/CD with exported key)
    if (signingKeyId != null && signingKey != null && signingPassword != null) {
        useInMemoryPgpKeys(signingKeyId, signingKey, signingPassword)
        sign(publishing.publications["release"])
    } else {
        // Use GPG command (for local development with GPG keyring)
        // This will use the default GPG keyring and gpg-agent
        useGpgCmd()
        sign(publishing.publications["release"])
    }
}
