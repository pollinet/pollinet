// Top-level build file for the PolliNet example/demo application.
// The SDK itself lives in ../pollinet-sdk and is included via composite build
// in settings.gradle.kts.
plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.kotlin.android) apply false
    alias(libs.plugins.kotlin.compose) apply false
    kotlin("plugin.serialization") version "2.1.0" apply false
}