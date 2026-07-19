# Clean-room SPC700 music engine build. Requires WLA-DX (wla-spc700 + wlalink) on PATH.
WLA  ?= wla-spc700
LINK ?= wlalink

all: driver.bin

engine.o: engine.asm
	$(WLA) -o engine.o engine.asm

driver.bin: engine.o linkfile
	$(LINK) -v linkfile driver.bin

clean:
	rm -f engine.o driver.bin driver.sym
