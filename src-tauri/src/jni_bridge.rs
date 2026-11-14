//! JNI bindings for `app.stafy.player.media.StafyNative` (Kotlin), used by the Media3
//! MediaLibraryService that powers Android Auto, Bluetooth (AVRCP) and the notification.

use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jstring, JNI_FALSE, JNI_TRUE};
use jni::{JNIEnv, JavaVM};

fn s(env: &mut JNIEnv, js: &JString) -> String {
    env.get_string(js).map(|v| v.into()).unwrap_or_default()
}

#[no_mangle]
pub extern "system" fn Java_app_stafy_player_media_StafyNative_init(
    mut env: JNIEnv,
    _class: JClass,
    data_dir: JString,
    cache_dir: JString,
) -> jboolean {
    let (data, cache) = (s(&mut env, &data_dir), s(&mut env, &cache_dir));
    match crate::core::init(data.as_ref(), cache.as_ref()) {
        Ok(_) => JNI_TRUE,
        Err(e) => {
            log::error!("stafy init failed: {e:#}");
            JNI_FALSE
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_app_stafy_player_media_StafyNative_call(
    mut env: JNIEnv,
    _class: JClass,
    method: JString,
    args: JString,
) -> jstring {
    let (m, a) = (s(&mut env, &method), s(&mut env, &args));
    let started = std::time::Instant::now();
    let out = crate::rpc::call_json(&m, &a);
    if started.elapsed() > std::time::Duration::from_millis(200) {
        eprintln!("catlify: slow native call {m}: {:?}", started.elapsed());
    }
    env.new_string(out).map(|j| j.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// `listener` implements `StafyNative.Listener { fun onState(json: String, trackChanged: Boolean) }`.
#[no_mangle]
pub extern "system" fn Java_app_stafy_player_media_StafyNative_setListener(
    env: JNIEnv,
    _class: JClass,
    listener: JObject,
) -> jboolean {
    let Some(core) = crate::core::get() else { return JNI_FALSE };
    let (Ok(vm), Ok(listener)) = (env.get_java_vm(), env.new_global_ref(listener)) else { return JNI_FALSE };
    let vm: JavaVM = vm;
    let listener: GlobalRef = listener;
    core.observe(move |_, changed, json| {
        let Ok(mut env) = vm.attach_current_thread_permanently() else { return };
        let Ok(js) = env.new_string(json) else { return };
        let _ = env.call_method(
            listener.as_obj(),
            "onState",
            "(Ljava/lang/String;Z)V",
            &[JValue::Object(&js), JValue::Bool(changed as u8)],
        );
        let _ = env.exception_clear();
        let _ = env.delete_local_ref(js);
    });
    JNI_TRUE
}

/// `resolver` implements `StafyNative.StreamResolver { fun fetch(spec: String, stem: String): String }`
/// (bundled yt-dlp, see StafyYouTube.kt). Returns the file path, or "ERR:<message>".
#[no_mangle]
pub extern "system" fn Java_app_stafy_player_media_StafyNative_setStreamResolver(
    env: JNIEnv,
    _class: JClass,
    resolver: JObject,
) -> jboolean {
    let (Ok(vm), Ok(resolver)) = (env.get_java_vm(), env.new_global_ref(resolver)) else { return JNI_FALSE };
    let vm: JavaVM = vm;
    let resolver: GlobalRef = resolver;
    let (vm2, resolver2) = (env.get_java_vm(), resolver.clone());
    if let Ok(vm2) = vm2 {
        // Fast path: yt-dlp only looks up the link (JSON); Rust downloads the audio itself.
        crate::audio::engine::set_link_extractor(Box::new(move |spec| {
            let mut env = vm2.attach_current_thread_permanently()?;
            let jspec = env.new_string(spec)?;
            let out = env.call_method(resolver2.as_obj(), "extract", "(Ljava/lang/String;)Ljava/lang/String;", &[JValue::Object(&jspec)]);
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
                anyhow::bail!("YouTube lookup crashed");
            }
            let obj = out?.l()?;
            let json: String = env.get_string(&JString::from(obj))?.into();
            match json.strip_prefix("ERR:") {
                Some(e) => Err(anyhow::anyhow!("{e}")),
                None => Ok(json),
            }
        }));
    }
    crate::audio::engine::set_stream_resolver(Box::new(move |spec, stem| {
        let mut env = vm.attach_current_thread_permanently()?;
        let (jspec, jstem) = (env.new_string(spec)?, env.new_string(stem.to_string_lossy())?);
        let out = env.call_method(
            resolver.as_obj(),
            "fetch",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&jspec), JValue::Object(&jstem)],
        );
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
            anyhow::bail!("YouTube fetch crashed");
        }
        let obj = out?.l()?;
        let path: String = env.get_string(&JString::from(obj))?.into();
        match path.strip_prefix("ERR:") {
            Some(err) => Err(anyhow::anyhow!("{err}")),
            None if path.is_empty() => Err(anyhow::anyhow!("YouTube fetch failed")),
            None => Ok(std::path::PathBuf::from(path)),
        }
    }));
    JNI_TRUE
}
