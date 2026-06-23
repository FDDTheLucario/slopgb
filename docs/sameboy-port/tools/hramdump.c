/* Minimal SameBoy harness: load a gbmicrotest ROM, run it headless (no button
 * input, no menu mashing), then dump HRAM $FF80/$FF81/$FF82 — the gbmicrotest
 * result block. Ground-truths whether SameBoy passes a gbmicrotest ROM.
 *
 * Usage: hramdump [--dmg|--cgb] <rom> [boot.bin]
 */
#include "Core/gb.h"
#include "Core/debugger.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint32_t pixels[160 * 144];

int main(int argc, char **argv)
{
    bool dmg = true;
    const char *rom = NULL;
    const char *boot = NULL;
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--dmg") == 0) { dmg = true; continue; }
        if (strcmp(argv[i], "--cgb") == 0) { dmg = false; continue; }
        if (!rom) { rom = argv[i]; continue; }
        boot = argv[i];
    }
    if (!rom) { fprintf(stderr, "usage: hramdump [--dmg|--cgb] <rom> [boot]\n"); return 2; }

    GB_gameboy_t gb;
    GB_random_set_enabled(false);
    int br;
    if (dmg) {
        GB_init(&gb, GB_MODEL_DMG_B);
        br = GB_load_boot_rom(&gb, boot ?: "build/bin/tester/dmg_boot.bin");
    } else {
        GB_init(&gb, GB_MODEL_CGB_E);
        br = GB_load_boot_rom(&gb, boot ?: "build/bin/tester/cgb_boot.bin");
    }
    fprintf(stderr, "[diag] boot_rom_load=%d\n", br);
    GB_debugger_set_disabled(&gb, true);
    GB_set_pixels_output(&gb, pixels);
    if (GB_load_rom(&gb, rom)) { perror("load rom"); return 1; }

    size_t hsize = 0; uint16_t hbank = 0;
    uint8_t *hram = GB_get_direct_access(&gb, GB_DIRECT_ACCESS_HRAM, &hsize, &hbank);

    /* Run a generous budget (boot animation ~60-90 frames + test) or until the
     * FF82 verdict is written. */
    unsigned long budget = 70224UL * 400;
    unsigned long spent = 0;
    unsigned long calls = 0;
    while (spent < budget) {
        spent += GB_run(&gb);
        calls++;
        if (hram[2] == 0x01 || hram[2] == 0xFF) {   /* HRAM[2] == $FF82 verdict */
            for (int k = 0; k < 4000; k++) spent += GB_run(&gb);
            break;
        }
    }

    uint8_t ff80 = hram[0], ff81 = hram[1], ff82 = hram[2];
    GB_registers_t *r = GB_get_registers(&gb);
    fprintf(stderr, "[diag] hsize=%zu calls=%lu spent=%lu pc=%04X sp=%04X a=%02X\n",
            hsize, calls, spent, r->pc, r->sp, r->a);
    printf("%s FF80=%02X FF81=%02X FF82=%02X %s\n",
           rom, ff80, ff81, ff82,
           ff82 == 0x01 ? "PASS" : ff82 == 0xFF ? "FAIL" : "NOVERDICT");
    GB_free(&gb);
    return 0;
}
