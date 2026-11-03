/* SPDX-License-Identifier: Apache-2.0
 * Standalone pronunciation utility. The linked eSpeak NG distribution is GPL-3.0-or-later.
 * stdin: little-endian u32 byte length, then one dialect byte ('a' American, 'b' British)
 * and the UTF-8 text. stdout: u32 length and the IPA. This program never generates or
 * plays audio.
 */
#include <espeak-ng/speak_lib.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int write_size(uint32_t n) {
  unsigned char bytes[] = {n & 255, (n >> 8) & 255, (n >> 16) & 255, (n >> 24) & 255};
  return fwrite(bytes, 1, 4, stdout) == 4;
}
int main(int argc, char **argv) {
  if (argc != 2 || espeak_Initialize(AUDIO_OUTPUT_SYNCHRONOUS, 0, argv[1], espeakINITIALIZE_DONT_EXIT) < 0 ||
      espeak_SetVoiceByName("en-us") != EE_OK) return 1;
  char dialect = 'a';
  if (!write_size(0) || fflush(stdout)) return 1;
  unsigned char bytes[4];
  while (fread(bytes, 1, 4, stdin) == 4) {
    uint32_t n = (uint32_t)bytes[0] | (uint32_t)bytes[1] << 8 | (uint32_t)bytes[2] << 16 | (uint32_t)bytes[3] << 24;
    if (n < 2 || n > 8193) return 2;
    char wanted;
    if (fread(&wanted, 1, 1, stdin) != 1 || (wanted != 'a' && wanted != 'b')) return 2;
    if (wanted != dialect) {
      /* "en" is eSpeak's British English (Received Pronunciation). */
      if (espeak_SetVoiceByName(wanted == 'b' ? "en" : "en-us") != EE_OK) return 2;
      dialect = wanted;
    }
    n -= 1;
    char text[8193];
    if (fread(text, 1, n, stdin) != n || memchr(text, 0, n)) return 2;
    text[n] = 0;
    const void *cursor = text;
    char result[65536]; size_t length = 0;
    do {
      const char *phones = espeak_TextToPhonemes(&cursor, espeakCHARS_UTF8, espeakPHONEMES_IPA);
      if (!phones) return 3;
      size_t count = strlen(phones);
      if (length + count + 2 >= sizeof(result)) return 3;
      if (length && count) result[length++] = ' ';
      memcpy(result + length, phones, count); length += count;
    } while (cursor);
    if (!length || !write_size((uint32_t)length) || fwrite(result, 1, length, stdout) != length || fflush(stdout)) return 3;
  }
  espeak_Terminate();
  return ferror(stdin) ? 1 : 0;
}
