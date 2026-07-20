# MBC6 exerciser ROM

Tests the MBC6 register file and the MX29F008 flash command set per Pan Docs
"MBC6" — see the test map comments in the sources. Two syntax twins assemble
the same test program:

- `mbc6test.asm` — RGBDS source → `mbc6test.gb`
- `mbc6test-wla.s` — wla-dx source → `mbc6test-wla.gb`

Both prebuilt ROMs are committed so `cargo test -p slopgb-core --test mbc6`
never skips. Pass/fail uses the mooneye breakpoint protocol (`LD B,B`;
Fibonacci registers = pass, `B=$42` with the failing test number in `C`).

## Rebuild

RGBDS (tested with v1.0.1):

```sh
rgbasm -o mbc6test.o mbc6test.asm
rgblink -o mbc6test.gb -p 0xFF mbc6test.o
rgbfix -v -p 0xFF -m 0x20 -r 0x03 -t "MBC6TEST" mbc6test.gb
```

wla-dx (tested with v10.8a):

```sh
wla-gb -o mbc6test-wla.o mbc6test-wla.s
wlalink linkfile-wla mbc6test-wla.gb
```
