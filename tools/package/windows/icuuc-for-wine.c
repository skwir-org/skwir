/* A stand-in for Windows' own icuuc.dll, put in the Wine prefix that
   tools/package/windows-check.sh tries the package in: Windows has had
   it since 10 1703, Wine has none, and gpui's jump list measures a string
   with it. Never shipped. */
#include <stdint.h>

__declspec(dllexport) int32_t u_strlen(const uint16_t *s) {
    int32_t n = 0;
    while (s[n]) {
        n++;
    }
    return n;
}
