/* Paradee through the ONNX Runtime C API. One graph call turns token ids into a
 * complete 24 kHz waveform; Rust owns chunking, pronunciation and playback. */
#include "onnxruntime_c_api.h"
#include <dlfcn.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef int32_t (*ItAudioCallback)(const float *samples, int32_t n, float progress, void *context);

void it_speech_quiet_logs(void) {
  /* Runtime warnings can name graph details; GUI errors use typed Rust status. */
  int sink = open("/dev/null", O_WRONLY);
  if (sink >= 0) { dup2(sink, STDERR_FILENO); close(sink); }
}

typedef struct {
  void *library;
  const OrtApi *api;
  OrtEnv *env;
  OrtSessionOptions *options;
  OrtSession *session;
  OrtMemoryInfo *memory;
} ItSpeech;

/* Copies a content-free message, releases the status and reports failure. */
static int failed(const OrtApi *api, OrtStatus *status, char *error, size_t error_size,
                  const char *message) {
  if (!status) return 0;
  if (error) snprintf(error, error_size, "%s", message);
  api->ReleaseStatus(status);
  return 1;
}

void it_speech_destroy(ItSpeech *engine) {
  if (!engine) return;
  if (engine->api) {
    if (engine->memory) engine->api->ReleaseMemoryInfo(engine->memory);
    if (engine->session) engine->api->ReleaseSession(engine->session);
    if (engine->options) engine->api->ReleaseSessionOptions(engine->options);
    if (engine->env) engine->api->ReleaseEnv(engine->env);
  }
  if (engine->library) dlclose(engine->library);
  free(engine);
}

ItSpeech *it_speech_create(const char *library, const char *model, int threads, char *error,
                           size_t error_size) {
  ItSpeech *engine = calloc(1, sizeof(*engine));
  if (!engine) return NULL;
  engine->library = dlopen(library, RTLD_NOW | RTLD_LOCAL);
  if (!engine->library) {
    snprintf(error, error_size, "Cannot load speech runtime: %s", dlerror());
    it_speech_destroy(engine);
    return NULL;
  }
  const OrtApiBase *(*base)(void) = dlsym(engine->library, "OrtGetApiBase");
  engine->api = base ? base()->GetApi(ORT_API_VERSION) : NULL;
  if (!engine->api) {
    snprintf(error, error_size, "Speech runtime has an incompatible API");
    it_speech_destroy(engine);
    return NULL;
  }
  const OrtApi *api = engine->api;
  size_t inputs = 0;
  if (failed(api, api->CreateEnv(ORT_LOGGING_LEVEL_ERROR, "infotransfer", &engine->env), error,
             error_size, "Cannot start the speech runtime") ||
      failed(api, api->CreateSessionOptions(&engine->options), error, error_size,
             "Cannot start the speech runtime") ||
      failed(api, api->SetIntraOpNumThreads(engine->options, threads), error, error_size,
             "Cannot start the speech runtime") ||
      failed(api, api->SetInterOpNumThreads(engine->options, 1), error, error_size,
             "Cannot start the speech runtime") ||
      failed(api, api->SetSessionGraphOptimizationLevel(engine->options, ORT_ENABLE_ALL), error,
             error_size, "Cannot start the speech runtime") ||
      failed(api, api->CreateSession(engine->env, model, engine->options, &engine->session), error,
             error_size, "Cannot initialize the speech model. Check the installed assets.") ||
      failed(api, api->SessionGetInputCount(engine->session, &inputs), error, error_size,
             "Cannot initialize the speech model. Check the installed assets.") ||
      failed(api, api->CreateCpuMemoryInfo(OrtArenaAllocator, OrtMemTypeDefault, &engine->memory),
             error, error_size, "Cannot start the speech runtime")) {
    it_speech_destroy(engine);
    return NULL;
  }
  if (inputs != 3) {
    snprintf(error, error_size, "Speech model has an unexpected interface");
    it_speech_destroy(engine);
    return NULL;
  }
  return engine;
}

/* Runs one chunk. The callback receives the whole waveform once; returns 0 on failure. */
int it_speech_generate(ItSpeech *engine, const int64_t *tokens, int32_t token_count, int voice,
                       float speed, ItAudioCallback callback, void *context, int *sample_count) {
  const OrtApi *api = engine->api;
  if (!tokens || token_count <= 0) return 0;
  int64_t token_shape[] = {1, token_count};
  int64_t scalar_shape[] = {1};
  int64_t voice_id = voice;
  OrtValue *values[3] = {NULL, NULL, NULL};
  OrtValue *waveform = NULL;
  int ok = 0;
  if (failed(api, api->CreateTensorWithDataAsOrtValue(
                      engine->memory, (void *)tokens, sizeof(int64_t) * (size_t)token_count,
                      token_shape, 2, ONNX_TENSOR_ELEMENT_DATA_TYPE_INT64, &values[0]),
             NULL, 0, NULL) ||
      failed(api, api->CreateTensorWithDataAsOrtValue(engine->memory, &speed, sizeof(float),
                                                      scalar_shape, 1,
                                                      ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT,
                                                      &values[1]),
             NULL, 0, NULL) ||
      failed(api, api->CreateTensorWithDataAsOrtValue(engine->memory, &voice_id, sizeof(int64_t),
                                                      scalar_shape, 1,
                                                      ONNX_TENSOR_ELEMENT_DATA_TYPE_INT64,
                                                      &values[2]),
             NULL, 0, NULL))
    goto done;
  const char *input_names[] = {"input_ids", "speed", "voice"};
  const char *output_names[] = {"waveform"};
  if (failed(api, api->Run(engine->session, NULL, input_names, (const OrtValue *const *)values, 3,
                           output_names, 1, &waveform),
             NULL, 0, NULL))
    goto done;
  OrtTensorTypeAndShapeInfo *info = NULL;
  size_t count = 0;
  float *samples = NULL;
  if (failed(api, api->GetTensorTypeAndShape(waveform, &info), NULL, 0, NULL)) goto done;
  OrtStatus *status = api->GetTensorShapeElementCount(info, &count);
  api->ReleaseTensorTypeAndShapeInfo(info);
  if (failed(api, status, NULL, 0, NULL) ||
      failed(api, api->GetTensorMutableData(waveform, (void **)&samples), NULL, 0, NULL))
    goto done;
  if (count == 0 || count > INT32_MAX) goto done;
  *sample_count = (int)count;
  callback(samples, (int32_t)count, 1.0f, context);
  ok = 1;
done:
  for (int i = 0; i < 3; i++)
    if (values[i]) api->ReleaseValue(values[i]);
  if (waveform) api->ReleaseValue(waveform);
  return ok;
}
