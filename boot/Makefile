# Build the slopgb CGB boot ROM (2304 bytes) with RGBDS.
TARGET = slopgb_cgb_boot.bin

$(TARGET): slopgb_cgb_boot.asm hardware.inc logo.2bpp cgb_palettes.inc
	rgbasm -o boot.o slopgb_cgb_boot.asm
	rgblink -p 0x00 -o $(TARGET) boot.o
	@truncate -s 2304 $(TARGET)   # keep only the CGB-class region ($0000-$08FF)
	@sz=$$(stat -c%s $(TARGET)); \
	 if [ "$$sz" != "2304" ]; then echo "ERROR: $(TARGET) is $$sz bytes, expected 2304"; exit 1; fi; \
	 echo "built $(TARGET): $$sz bytes"

# The SLOPGB wordmark tiles. slopgb_logo.png is the source art (regenerate it
# with `cargo run -p slopgb-core --example gen_boot_logo`, which emits a PBM —
# see the example header); rgbgfx packs it into the 2bpp the boot ROM INCBINs.
logo.2bpp: slopgb_logo.png
	rgbgfx -o logo.2bpp slopgb_logo.png

clean:
	rm -f boot.o $(TARGET)

.PHONY: clean
