// C ABI exported by the Rust core (src-tauri/src/ffi.rs).
#pragma once
#include <stdbool.h>

bool stafy_init(const char *data_dir, const char *cache_dir);
char *stafy_call(const char *method, const char *args_json);
void stafy_free(char *s);
typedef void (*stafy_state_cb)(const char *state_json, bool track_changed);
bool stafy_set_listener(stafy_state_cb cb);

// In-process yt-dlp for iOS (ytdlp-apple), registered at launch: src-tauri/src/ytrun.rs.
typedef void (*stafy_yt_line)(void *context, const char *line, int is_stderr);
typedef char *(*stafy_yt_run)(const char *request_json, stafy_yt_line line, void *context);
typedef void (*stafy_yt_free)(char *s);
void stafy_set_ytdlp(stafy_yt_run run, stafy_yt_free free);
