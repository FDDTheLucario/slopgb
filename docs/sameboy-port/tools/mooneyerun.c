/* Minimal SameBoy harness for the MOONEYE protocol: run a ROM headless until
 * it executes `LD B,B` (the suite's completion signal) and print B,C,D,E,H,L.
 * Pass <=> 3,5,8,13,21,34 — so this ground-truths whether SameBoy passes any
 * mooneye-protocol ROM (the wilbertpol and age legs, 62 of the census rows
 * `classify_*.py` cannot reach: those read glyphs or pixels, and these ROMs
 * report in registers).
 *
 * The completion signal is the register signature itself, polled per step: the
 * debugger's software breakpoint stays disabled (a trapped `LD B,B` freezes the
 * run, the same reason hramdump.c disables it), and reading the opcode at PC
 * with `GB_safe_read_memory` segfaults on the CGB models. So this reports PASS
 * on the signature and otherwise runs to the timeout and prints the registers
 * it ended on — enough to gate a row, which only asks whether SameBoy passes.
 *
 * Usage: mooneyerun [--dmg|--cgb|--agb|--mgb|--sgb|--sgb2] <rom> [boot.bin]
 * Build:  see the README (link against build/obj/Core/*.c.o).
 */
#include "Core/gb.h"
#include "Core/debugger.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* SGB-sized: the SGB models render 256x224 into this buffer. */
static uint32_t pixels[256 * 224];

/* SameBoy calls this per pixel; leaving it unset segfaults the CGB models
 * (the tester installs one too — Tester/main.c). */
static uint32_t rgb_encode(GB_gameboy_t *gb, uint8_t r, uint8_t g, uint8_t b)
{
    (void)gb;
    return ((uint32_t)r << 16) | ((uint32_t)g << 8) | b;
}

int main(int argc, char **argv)
{
    GB_model_t model = GB_MODEL_DMG_B;
    const char *boot_default = "build/bin/tester/dmg_boot.bin";
    const char *rom = NULL;
    const char *boot = NULL;
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--dmg") == 0) { model = GB_MODEL_DMG_B; boot_default = "build/bin/tester/dmg_boot.bin"; continue; }
        if (strcmp(argv[i], "--mgb") == 0) { model = GB_MODEL_MGB;   boot_default = "build/bin/tester/dmg_boot.bin"; continue; }
        if (strcmp(argv[i], "--cgb") == 0) { model = GB_MODEL_CGB_E; boot_default = "build/bin/tester/cgb_boot.bin"; continue; }
        if (strcmp(argv[i], "--agb") == 0) { model = GB_MODEL_AGB;   boot_default = "build/bin/tester/agb_boot.bin"; continue; }
        if (strcmp(argv[i], "--sgb") == 0) { model = GB_MODEL_SGB;   boot_default = "build/bin/tester/sgb_boot.bin"; continue; }
        if (strcmp(argv[i], "--sgb2") == 0) { model = GB_MODEL_SGB2; boot_default = "build/bin/tester/sgb2_boot.bin"; continue; }
        if (!rom) { rom = argv[i]; continue; }
        boot = argv[i];
    }
    if (!rom) {
        fprintf(stderr, "usage: mooneyerun [--dmg|--mgb|--cgb|--agb|--sgb|--sgb2] <rom> [boot]\n");
        return 2;
    }

    GB_gameboy_t gb;
    GB_random_set_enabled(false);
    GB_init(&gb, model);
    GB_load_boot_rom(&gb, boot ?: boot_default);
    GB_debugger_set_disabled(&gb, true);
    GB_set_pixels_output(&gb, pixels);
    GB_set_rgb_encode_callback(&gb, rgb_encode);
    if (GB_load_rom(&gb, rom)) { perror("load rom"); return 1; }

    /* 20 emulated seconds: every mooneye-protocol ROM in the collection
     * signals inside a second or two, and the sweep runs 60+ of them. */
    unsigned long budget = 4194304UL * 20;
    unsigned long spent = 0;
    bool pass = false;
    GB_registers_t *r = GB_get_registers(&gb);
    while (spent < budget) {
        spent += GB_run(&gb);
        if (r->b == 3 && r->c == 5 && r->d == 8 && r->e == 13 && r->h == 21 && r->l == 34) {
            pass = true;
            break;
        }
    }
    printf("%s B=%02X C=%02X D=%02X E=%02X H=%02X L=%02X %s\n",
           rom, r->b, r->c, r->d, r->e, r->h, r->l, pass ? "PASS" : "NOPASS");
    GB_free(&gb);
    return 0;
}
