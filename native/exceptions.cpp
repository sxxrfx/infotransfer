#include "c-api.h"
#include <cstddef>
#include <cstdio>

extern "C" {
void *it_speech_create_unchecked(const char *, const char *, const char *,
                               const char *, int, int, char *, size_t);
int it_speech_generate_unchecked(void *, const char *, const int32_t *, int32_t, int, float,
    SherpaOnnxGeneratedAudioProgressCallbackWithArg, void *, int *);

void *it_speech_create(const char *library, const char *model, const char *voices,
                      const char *tokens, int threads,
                      int kind, char *error, size_t error_size) noexcept {
    try {
        return it_speech_create_unchecked(library, model, voices, tokens,
                                         threads, kind, error, error_size);
    } catch (...) {
        std::snprintf(error, error_size, "Speech runtime could not load the model");
        return nullptr;
    }
}

int it_speech_generate(void *engine, const char *text, const int32_t *phonemes, int32_t phoneme_count, int voice, float speed,
    SherpaOnnxGeneratedAudioProgressCallbackWithArg callback, void *context,
    int *sample_count) noexcept {
    try {
        return it_speech_generate_unchecked(engine, text, phonemes, phoneme_count, voice, speed, callback,
                                            context, sample_count);
    } catch (...) {
        // Exception messages can contain selected words. Return a content-free error.
        return 0;
    }
}
}
