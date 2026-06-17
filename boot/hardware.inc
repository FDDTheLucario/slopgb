; Minimal Game Boy I/O register address constants (factual hardware addresses,
; FF00-page). Only the registers this boot ROM uses.

DEF rP1    EQU $FF00   ; joypad
DEF rDIV   EQU $FF04
DEF rNR50  EQU $FF24   ; master volume
DEF rNR51  EQU $FF25   ; channel pan
DEF rNR52  EQU $FF26   ; sound on/off
DEF rNR10  EQU $FF10   ; ch1 sweep
DEF rNR11  EQU $FF11   ; ch1 length/duty
DEF rNR12  EQU $FF12   ; ch1 envelope
DEF rNR13  EQU $FF13   ; ch1 freq lo
DEF rNR14  EQU $FF14   ; ch1 freq hi + trigger
DEF rLCDC  EQU $FF40
DEF rSTAT  EQU $FF41
DEF rSCY   EQU $FF42
DEF rSCX   EQU $FF43
DEF rLY    EQU $FF44
DEF rBGP   EQU $FF47
DEF rOBP0  EQU $FF48
DEF rOBP1  EQU $FF49
DEF rWY    EQU $FF4A
DEF rWX    EQU $FF4B
DEF rVBK   EQU $FF4F   ; CGB VRAM bank
DEF rBGPI  EQU $FF68   ; CGB BG palette index
DEF rBGPD  EQU $FF69   ; CGB BG palette data
DEF rOBPI  EQU $FF6A   ; CGB OBJ palette index
DEF rOBPD  EQU $FF6B   ; CGB OBJ palette data
DEF rBOOT  EQU $FF50   ; boot ROM disable
