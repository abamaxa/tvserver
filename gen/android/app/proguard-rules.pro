# Add project specific ProGuard rules here.
-keep class com.abamaxa.lots_of_videos.NativeBridge { *; }

# Commons Compress instantiates ZIP extra fields with Class.newInstance() while
# the downloader SDK unpacks Python/FFmpeg on first use. Preserve those public
# constructors so R8 cannot turn the implementations into abstract classes.
-keep,allowobfuscation class org.apache.commons.compress.archivers.zip.** implements org.apache.commons.compress.archivers.zip.ZipExtraField {
    public <init>();
}
-keep class org.videolan.libvlc.** { *; }
-keep class org.videolan.medialibrary.** { *; }
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile
