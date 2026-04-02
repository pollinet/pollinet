plugins {
    alias(libs.plugins.android.library) apply false
    alias(libs.plugins.kotlin.android) apply false
    kotlin("plugin.serialization") version "2.1.0" apply false
}
