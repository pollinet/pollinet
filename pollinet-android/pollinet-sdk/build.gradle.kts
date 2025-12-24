plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.android)
    kotlin("plugin.serialization")
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

// Task to build Rust library using cargo-ndk
tasks.register<Exec>("buildRustLib") {
    description = "Build Rust library for Android using cargo-ndk"
    group = "build"
    
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
}

// Make preBuild depend on buildRustLib
tasks.named("preBuild") {
    dependsOn("buildRustLib")
}

