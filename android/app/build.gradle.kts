plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.bandbridge.relay"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.bandbridge.relay"
        minSdk = 26
        targetSdk = 33
        versionCode = 7
        versionName = "2.4.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // The debug key signed 1.0.0, 2.0.0, 2.1.0 and 2.2.0, one of which
            // is on the reader. Android installs an update only over an app
            // signed with the same key, so the release is signed with it too
            // (D17, B9, C7, A5).
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    testImplementation("junit:junit:4.13.2")
    // Android ships org.json, but the JVM tests get only stubs of it that throw.
    testImplementation("org.json:json:20240303")
}
