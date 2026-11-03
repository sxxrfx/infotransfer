#include "c-api.h"
#include <dlfcn.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

void it_speech_quiet_logs(void) {
  /* The upstream frontend can log source words. GUI errors use typed Rust status. */
  int sink = open("/dev/null", O_WRONLY);
  if (sink >= 0) { dup2(sink, STDERR_FILENO); close(sink); }
}

typedef struct {
  void *library;
  const SherpaOnnxOfflineTts *tts;
  void (*destroy)(const SherpaOnnxOfflineTts *);
  int32_t (*sample_rate)(const SherpaOnnxOfflineTts *);
  int32_t (*speakers)(const SherpaOnnxOfflineTts *);
  const SherpaOnnxGeneratedAudio *(*generate)(
      const SherpaOnnxOfflineTts *, const char *,
      const SherpaOnnxGenerationConfig *,
      SherpaOnnxGeneratedAudioProgressCallbackWithArg, void *);
  void (*destroy_audio)(const SherpaOnnxGeneratedAudio *);
} ItSpeech;

ItSpeech *it_speech_create_unchecked(const char *library, const char *model,
                          const char *voices, const char *tokens,
                          int threads, int kind, char *error,
                          size_t error_size) {
  ItSpeech *engine = calloc(1, sizeof(*engine));
  if (!engine) return NULL;
  engine->library = dlopen(library, RTLD_NOW | RTLD_LOCAL);
  if (!engine->library) {
    snprintf(error, error_size, "Cannot load speech runtime: %s", dlerror());
    free(engine);
    return NULL;
  }
  const SherpaOnnxOfflineTts *(*create)(const SherpaOnnxOfflineTtsConfig *) =
      dlsym(engine->library, "SherpaOnnxCreateOfflineTts");
  engine->destroy = dlsym(engine->library, "SherpaOnnxDestroyOfflineTts");
  engine->sample_rate = dlsym(engine->library, "SherpaOnnxOfflineTtsSampleRate");
  engine->speakers = dlsym(engine->library, "SherpaOnnxOfflineTtsNumSpeakers");
  engine->generate = dlsym(engine->library, "SherpaOnnxOfflineTtsGenerateWithConfig");
  engine->destroy_audio = dlsym(engine->library, "SherpaOnnxDestroyOfflineTtsGeneratedAudio");
  if (!create || !engine->destroy || !engine->sample_rate || !engine->speakers ||
      !engine->generate || !engine->destroy_audio) {
    snprintf(error, error_size, "Speech runtime has an incompatible API");
    dlclose(engine->library);
    free(engine);
    return NULL;
  }
  SherpaOnnxOfflineTtsConfig config = {0};
  if (kind == 1) {
    config.model.kitten.model = model;
    config.model.kitten.voices = voices;
    config.model.kitten.tokens = tokens;
    config.model.kitten.length_scale = 1;
  } else {
    config.model.kokoro.model = model;
    config.model.kokoro.voices = voices;
    config.model.kokoro.tokens = tokens;
    config.model.kokoro.length_scale = 1;
  }
  config.model.num_threads = threads;
  config.model.provider = "cpu";
  config.max_num_sentences = 1;
  config.silence_scale = 1.0f;
  engine->tts = create(&config);
  if (!engine->tts) {
    snprintf(error, error_size, "Cannot initialize the speech model. Check the installed assets.");
    dlclose(engine->library);
    free(engine);
    return NULL;
  }
  return engine;
}

void it_speech_destroy(ItSpeech *engine) {
  if (!engine) return;
  engine->destroy(engine->tts);
  dlclose(engine->library);
  free(engine);
}

int it_speech_sample_rate(ItSpeech *engine) { return engine->sample_rate(engine->tts); }
int it_speech_speakers(ItSpeech *engine) { return engine->speakers(engine->tts); }

typedef struct {
  SherpaOnnxGeneratedAudioProgressCallbackWithArg callback;
  void *context;
  int calls;
} ItCallback;

static int32_t it_receive(const float *samples, int32_t n, float progress, void *arg) {
  ItCallback *state = arg;
  state->calls += 1;
  return state->callback(samples, n, progress, state->context);
}

int it_speech_generate_unchecked(ItSpeech *engine, const char *text, const int32_t *phonemes, int32_t phoneme_count, int voice, float speed,
                       SherpaOnnxGeneratedAudioProgressCallbackWithArg callback,
                       void *context, int *sample_count) {
  SherpaOnnxGenerationConfig config = {0};
  config.sid = voice;
  config.speed = speed;
  config.silence_scale = 1.0f;
  if (!phonemes || phoneme_count <= 0) return 0;
  config.phoneme_codepoints = phonemes;
  config.phoneme_codepoints_lens = &phoneme_count;
  config.phoneme_codepoints_num_sentences = 1;
  ItCallback state = {callback, context, 0};
  const SherpaOnnxGeneratedAudio *audio =
      engine->generate(engine->tts, text, &config, it_receive, &state);
  if (!audio) return 0;
  /* Kitten 2.0.1 returns completed PCM without invoking its callback. */
  if (state.calls == 0 && audio->n > 0) {
    callback(audio->samples, audio->n, 1.0f, context);
  }
  *sample_count = audio->n;
  engine->destroy_audio(audio);
  return 1;
}
