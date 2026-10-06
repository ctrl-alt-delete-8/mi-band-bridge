# Local changes to tiny-ECDH-c

Source: https://github.com/kokke/tiny-ECDH-c (Unlicense / public domain).

1. Added `#include <stdio.h>` as the first line of `ecdh.c`.
   `ecdsa_sign` calls `printf` with no declaration. Clang 1.95 makes an
   implicit function declaration an error, so the file does not compile as
   shipped. BandBridge does not use ECDSA. The include is the smallest fix.

No other change.
