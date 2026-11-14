import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// Release signing: ~/.stafy-signing/keystore.properties (override with STAFY_KEYSTORE_PROPERTIES).
val keystoreProperties = Properties().apply {
    val path = System.getenv("STAFY_KEYSTORE_PROPERTIES")
        ?: "${System.getProperty("user.home")}/.stafy-signing/keystore.properties"
    val f = file(path)
    if (f.exists()) f.inputStream().use { load(it) }
}

// YouTube downloads (bundled yt-dlp). Store builds: -Pstafy.youtube=false (Play forbids YouTube downloaders).
val youtube = (findProperty("stafy.youtube") as? String)?.toBoolean() ?: true

android {
    sourceSets["main"].java.srcDir(if (youtube) "src/youtube/java" else "src/noyoutube/java")
    // yt-dlp's Python and ffmpeg run as executables from the native lib dir, so they must be extracted.
    packaging {
        jniLibs.useLegacyPackaging = youtube
    }
    compileSdk = 37
    namespace = "app.stafy.player"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "app.stafy.player"
        minSdk = 26
        targetSdk = 37
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    signingConfigs {
        if (keystoreProperties.containsKey("storeFile")) {
            create("release") {
                storeFile = file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("storePassword")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            signingConfigs.findByName("release")?.let { signingConfig = it }
            optimization {
               enable = false // R8 renamed classes Tauri calls over JNI (blank WebView)
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_17
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    // Media session for Android Auto, Bluetooth (AVRCP), notification and lock screen.
    implementation("androidx.media3:media3-session:1.8.0")
    implementation("androidx.media3:media3-common:1.8.0")
    implementation("androidx.media:media:1.7.0")
    implementation("androidx.core:core-ktx:1.16.0")
    if (youtube) {
        implementation("io.github.junkfood02.youtubedl-android:library:0.18.1")
        implementation("io.github.junkfood02.youtubedl-android:ffmpeg:0.18.1")
    }
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))
