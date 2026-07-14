# Build the slopgb clean-room boot ROMs with RGBDS.
CGB_TARGET = slopgb_cgb_boot.bin
SGB_TARGET = slopgb_sgb_boot.bin

all: $(CGB_TARGET) $(SGB_TARGET)

# CGB boot ROM: 2304 bytes, CGB-class ($0000-$08FF).
$(CGB_TARGET): slopgb_cgb_boot.asm hardware.inc logo.2bpp cgb_palettes.inc
	rgbasm -o boot.o slopgb_cgb_boot.asm
	rgblink -p 0x00 -o $(CGB_TARGET) boot.o
	@truncate -s 2304 $(CGB_TARGET)   # keep only the CGB-class region ($0000-$08FF)
	@sz=$$(stat -c%s $(CGB_TARGET)); \
	 if [ "$$sz" != "2304" ]; then echo "ERROR: $(CGB_TARGET) is $$sz bytes, expected 2304"; exit 1; fi; \
	 echo "built $(CGB_TARGET): $$sz bytes"

# SGB boot ROM: 256 bytes, DMG-class ($0000-$00FF).
$(SGB_TARGET): slopgb_sgb_boot.asm hardware.inc
	rgbasm -o sgb_boot.o slopgb_sgb_boot.asm
	rgblink -p 0x00 -o $(SGB_TARGET) sgb_boot.o
	@truncate -s 256 $(SGB_TARGET)   # keep only the DMG-class region ($0000-$00FF)
	@sz=$$(stat -c%s $(SGB_TARGET)); \
	 if [ "$$sz" != "256" ]; then echo "ERROR: $(SGB_TARGET) is $$sz bytes, expected 256"; exit 1; fi; \
	 echo "built $(SGB_TARGET): $$sz bytes"

# The SLOPGB wordmark tiles. slopgb_logo.png is the source art (regenerate it
# with `cargo run -p slopgb-core --example gen_boot_logo`, which emits a PBM —
# see the example header); rgbgfx packs it into the 2bpp the boot ROM INCBINs.
logo.2bpp: slopgb_logo.png
	rgbgfx -o logo.2bpp slopgb_logo.png

clean:
	rm -f boot.o sgb_boot.o $(CGB_TARGET) $(SGB_TARGET)

.PHONY: all clean
