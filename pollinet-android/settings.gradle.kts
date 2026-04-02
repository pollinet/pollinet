pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "pollinet-android"

// Pull in the SDK as a composite build so implementation("xyz.pollinet:pollinet-sdk")
// is transparently satisfied from local source during development.
includeBuild("../pollinet-sdk") {
    dependencySubstitution {
        substitute(module("xyz.pollinet:pollinet-sdk")).using(project(":pollinet-sdk"))
    }
}

include(":app")
