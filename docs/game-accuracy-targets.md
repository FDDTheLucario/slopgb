# slopgb game-accuracy targets

> Generation date: **unknown** (not recorded — do not infer a date from this file's mtime).

## What this list is for

slopgb already ships an automated, deterministic test-ROM battery:

- **mooneye-gb** acceptance suite (439/439 passing), and
- the broader **game-boy-test-roms** collection.

Those suites give a hard pass/fail oracle for documented CPU/PPU/timer/MBC edge
cases. What they **cannot** cover is the behavior of real, copyrighted
*commercial* games: there is no public golden-frame oracle for "does Pokemon
Crystal's clock advance correctly" or "does this raster split render on the
right scanline." This document **complements** the automated battery by listing
real commercial (and a few homebrew/demoscene) titles whose observable behavior
exercises tricky hardware corners. They are meant to be checked by a human
running the game and watching for the documented behavior described below.

## Legality

The CRC32/MD5/SHA1 values here are **public preservation metadata** sourced from
the **No-Intro** datfiles redistributed through the **libretro-database**
project. This file **does not contain, host, or link to any ROM image.** To use
the list you must supply your own **legally dumped cartridges**; hash your own
dump and match it against the value in the table to confirm you are testing the
exact same dump the note refers to. A mismatch means you have a different
revision/region and the behavioral notes may not apply.

---

## Game Boy / Game Boy Color essential titles

| Title | Platform | Revision | Why it tests accuracy | Emus that failed | CRC32 | MD5 | SHA1 |
|---|---|---|---|---|---|---|---|
| Pokemon Red & Blue | Game Boy | — | — | — | 9F7FDD53 | 3D45C1EE9ABD5738DF46D2BDDA8B57DC | EA9BCAE617FDF159B045185467AE58B2E4A48B9A |
| Pokemon Yellow: Special Pikachu Edition | Game Boy | — | — | — | 7D527D62 | D9290DB87B1F0A23B89F99EE4469E34B | CC7D03262EBFAF2F06772C1A480C7D9D5F4A38E1 |
| Pokemon Gold & Silver | Game Boy Color | — | — | — | 6BDE3C3E | A6924CE1F9AD2228E1C6580779B23878 | D8B8A3600A465308C9953DFA04F0081C05BDCB94 |
| Pokemon Crystal | Game Boy Color | Rev 1 | — | — | 3358E30A | 301899B8087289A6436B0A241FBBB474 | F2F52230B536214EF7C9924F483392993E226CFB |
| Tetris | Game Boy | Rev 1 | — | — | 46DF91AD | 982ED5D2B12A0377EB14BCDC4123744E | 74591CC9501AF93873F9A5D3EB12DA12C0723BBC |
| Super Mario Land | Game Boy | Rev 1 | — | — | 2C27EC70 | B259FEB41811C7E4E1DC200167985C84 | 418203621B887CAA090215D97E3F509B79AFFD3E |
| Super Mario Land 2: 6 Golden Coins | Game Boy | Rev 2 | — | — | 635A9112 | 4BD6E929EC716A5C7FE7DC684860D551 | D11D94FA3C36B9F72E925070B66BB4F16D31001E |
| The Legend of Zelda: Link's Awakening DX | Game Boy Color | Rev 1 | — | — | B38EB9DE | CCBB56212E3DBAA9007D389A17E9D075 | 363D184D9B1E9FAA5A2FACD80897B7E118446164 |
| Kirby's Dream Land | Game Boy | — | — | — | 40F25740 | A66E4918EDCD042EC171A57FE3CE36C3 | 90979BAA1D0E24B41B5C304C5DDAF77450692D5A |
| Donkey Kong (1994 / DK94) | Game Boy | Rev 1 | — | — | F777A5D8 | 4859EC2B18C4FABF489EB570C1D7D326 | 397AD2FF25627B83E02C71B54C72BB4DEB39E0C0 |
| Wario Land: Super Mario Land 3 | Game Boy | — | — | — | 40BE3889 | D9D957771484EF846D4E8D241F6F2815 | AE65800302438E37A99E623A71D1C954D73C843E |
| Pokemon Pinball | Game Boy Color | — | — | — | 03CE8D9A | FBE20570C2E52C937A9395024069BA3C | 9402014D14969432142ABFDE728C6F1A10EE4DAC |

All twelve hashes were confirmed against gb.dat / gbc.dat. The "why" field was
intentionally blank — these are broad regression smoke-test staples, not
single-quirk targets.

---

## Game Boy / Game Boy Color hardware-quirk emulation titles (hash-verification batch)

| Title | Platform | Revision | Why it tests accuracy | Emus that failed | CRC32 | MD5 | SHA1 |
|---|---|---|---|---|---|---|---|
| Pinball Deluxe | Game Boy | — | — | — | 02864C32 | FA4AF656B09274E5B8139BE0A3A39A2C | A99D8A9880ED79B40766B3A532F8A165171CA450 |
| Pinball Fantasies | Game Boy | — | — | — | F056A911 | 3496B0CAB86AE1981142C0DDB9AE6183 | 58243F69598F2119F42DC4B35E89F04F92BB6D72 |
| Altered Space - A 3-D Alien Adventure | Game Boy | — | — | — | 141675D3 | 012EE0A196C03CCA91A43A9EADBECFB6 | CA586C59B2473A8BEC2F0F37504CB74E3A1E4D11 |
| Prehistorik Man | Game Boy | — | — | — | C5204156 | 64F43161EB16EB1BE99262C36867BC79 | 1139D6B799B8585BBB7868BD3BCA5BA9DEA4EEF5 |
| Road Rash | Game Boy | — | — | — | 88EDC83D | 71AF355CBF7B8C7FE30F509803BBCED6 | 488E699490598234E762ECF864C75EDCB1BC6066 |
| Zerd no Densetsu (Legend of Zerd) | Game Boy | — | — | — | 492EDB36 | FD280448AE0F60BAE1D10016A87FC1ED | 8DB8AD90505D9A2B4485E903B9F5D67BB9C0C149 |
| The Smurfs | Game Boy | Rev 1 | — | — | 8B5BCDE7 | A574E5F7119B31E5112221C3A0ADA813 | A0D6A85331FB034F68F05629A5FF85E13ADAB205 |
| Thunderbirds | Game Boy Color | — | — | — | B5BECECF | 5164521245F41B0F3A51CFFE0704D21D | 4DF8353CBB74D368CC139899EABE5288E59ADAEB |
| Mr. Do! | Game Boy | — | — | — | A1122FC0 | 65E455737DF458E59CC7B0892B95E6CD | 1262A3119E7D7CB724863AFA0AC467756F2FE611 |
| Alone in the Dark - The New Nightmare | Game Boy Color | — | — | — | C145C036 | D97055E4A2FD4624FC924C4834ACE35E | A348AEAC500D0D8FDAF90F5277631A026504FF44 |
| Warriors of Might and Magic | Game Boy Color | — | — | — | EF9F5BEA | F0656CF3AA3D6B539FB1B7DA0FD27617 | 06163ACAD95D6AB87FFCD56F18B37C8C9E37A6EC |

**Caveats.** Every title in this batch was hash-verified against the No-Intro
DAT, but the "why it tests accuracy" rationale field was empty, so the verifier
could **not** certify the specific hardware-quirk claim
(`rationaleOk = false` for all 11). Treat the quirk as *unconfirmed* until a
documented behavior is attached. Specific correction for **Road Rash**: the DAT
name `Road Rash (USA, Europe)` exists in **both** gb.dat and gbc.dat. The MD5
listed above (`71AF355CBF7B8C7FE30F509803BBCED6`) matches the **Game Boy**
version; the Game Boy Color version is a different dump
(MD5 `F5767F97F44365B703EAE78AFB7562E6`).

---

## Game Boy / Game Boy Color special-hardware titles (tilt/accelerometer, RTC, camera, sonar, IR, rumble, Mobile Adapter GB)

| Title | Platform | Revision | Why it tests accuracy | Emus that failed | CRC32 | MD5 | SHA1 |
|---|---|---|---|---|---|---|---|
| Kirby Tilt 'n' Tumble | Game Boy Color | — | MBC7 tilt cartridge (ADXL202 accelerometer + rumble) | unknown | E541ACF1 | F2E24776D93082362C9B435ABC167D89 | 6AB8D666E2BEBBB3FEE7796C8968AAB2EA21B8F9 |
| Command Master | Game Boy Color | — | MBC7 tilt/accelerometer cartridge (2nd GBC title to use it, after Kirby) | unknown | D10B5645 | 4D3E8BF64B69EB56C5F1786CEF422C59 | F1D3A1FF7A76C49CE3E094CA994D488DBDADF9A2 |
| Pokemon Crystal | Game Boy Color | Rev 1 | MBC3 + real-time clock | unknown | 3358E30A | 301899B8087289A6436B0A241FBBB474 | F2F52230B536214EF7C9924F483392993E226CFB |
| Pocket Monsters Crystal (Japanese, MBC30) | Game Boy Color | — | MBC30 (extended MBC3, more SRAM) + RTC + Mobile Adapter GB support | unknown | 270C4ECC | 9C3AE66BFFB28EA8ED2896822DA02992 | 95127B901BBCE2407DAF43CCE9F45D4C27EF635D |
| Pokemon Gold | Game Boy Color | — | MBC3 + real-time clock | unknown | 6BDE3C3E | A6924CE1F9AD2228E1C6580779B23878 | D8B8A3600A465308C9953DFA04F0081C05BDCB94 |
| Harvest Moon GBC | Game Boy Color | — | in-cart MBC3 real-time clock for day/season progression | unknown | AB5738A1 | 498C0A50A5E5CDE16127617A97AD6162 | D407B9C20C5381F46EC460858539A5B6F559E04F |
| Game Boy Camera | Game Boy | — | built-in M64282FP camera sensor | unknown | 4640909F | 42D2F65E2549BE9D1D126A6828B5D1C1 | 461C3C37ED270681E3E94053EFB21504B600AEF5 |
| Net de Get: Minigame @ 100 | Game Boy Color | — | downloads minigames via Mobile Adapter GB (Mobile System GB) | unknown | 6E33D509 | 77893D4574B1013A0699C4199C271B8A | 819EFDE3EBD0F52B080A8307979803914D029035 |
| Robopon Sun Version | Game Boy Color | — | cart carries an infrared port + real-time clock | unknown | 32CAEF11 | 398F7B60EA114B90B24503178F47E8D8 | 399C928A38A3901B7A1093BD61F5A4D8C05B9771 |
| Pokemon Card GB (Japanese Trading Card Game) | Game Boy Color | — | Japanese cart (DMG-ACXJ) has built-in infrared hardware for the Card Pop! feature | unknown | 1926F570 | 1633BEC4CABEC857C0EC67C99D2F982B | 2287627C5B4D56BD9A01AAB83408C301B9CF1A6C |
| Pokemon Pinball | Game Boy Color | — | Rumble Version cart: MBC5 + battery-powered rumble motor | unknown | 03CE8D9A | FBE20570C2E52C937A9395024069BA3C | 9402014D14969432142ABFDE728C6F1A10EE4DAC |
| Pocket Sonar (Game de Hakken!!) | Game Boy | — | Bandai cart contains an actual sonar fish-finder transducer | unknown | D68C9F79 | E7E0943CB9B8D6DD29C18E6A41E8D346 | 788CDF148431B82B5308C68B3E45B133A7074196 |

All twelve hashes were confirmed, and each special-hardware rationale was
verified as genuine (`rationaleOk = true`). The rationale text shown in the "why"
column was reconstructed from the verifier's notes (the source records left the
raw "why" blank). "Emus that failed" is `unknown` — no specific emulator failure
was recorded, only that these features defeat naive emulation.

---

## GB/GBC ROM hash and rationale verification

| Title | Platform | Revision | Why it tests accuracy | Emus that failed | CRC32 | MD5 | SHA1 |
|---|---|---|---|---|---|---|---|
| Cannon Fodder | Game Boy Color | — | — | — | 824C3BF3 | 3DD6B4DD7DA7F2B412F92C2509B9F1DF | 9A6901CAD65E6D0CE89AF8442688FEC6A9FEDD43 |
| Pocket Music | Game Boy Color | — | — | — | 1BFB531E | 84EEDE6BB298DD354F251ECCB1259316 | 74DC5EFAB773FDE400304D3DF8034A095AF8A742 |
| R-Type | Game Boy | — | — | — | E0F23FC0 | 972DC35B3B2BD0762999B1AE48DA94F6 | 28531A4EB668477DF98CAF0E87CEDC0E5FDFE53B |
| DuckTales | Game Boy | — | — | — | 2BBBB54D | 785441D3D75913393807B10B3194DC48 | 93460364E33E8FB09A0659738044D0297CD4DF69 |
| Prehistorik Man | Game Boy | — | — | — | C5204156 | 64F43161EB16EB1BE99262C36867BC79 | 1139D6B799B8585BBB7868BD3BCA5BA9DEA4EEF5 |
| Shantae | Game Boy Color | — | — | — | E994B59B | 028C4262DBB49F4FC462A6EB3E514D72 | 520E48C50F6E997FCD841CA368FC9ABC1DBDDEC1 |
| Final Fantasy Adventure | Game Boy | — | — | — | 18C78B3A | 24CD3BDF490EF2E1AA6A8AF380ECCD78 | 8B93C55EE2660C60CF86DD70058F96ACE98782C8 |
| Klax | Game Boy Color | — | — | — | 7181CBD0 | 3BD0DAD0C695A534B9E89264E09E2B11 | 23B5601748BDA8E0DBA69E589FA2B6D2D79781B3 |
| The Lion King: Simba's Mighty Adventure | Game Boy Color | — | — | — | D5B4B7BB | 67117CC76E2B270E65C2778C734F905F | 4FCB6698E4FD6BB03812A35ED545EC68B7C11FA7 |
| Disney's Tarzan | Game Boy Color | — | — | — | 4224F930 | 55FEA8E7BE17975374AB24518BD83171 | CE23EAA9AEF5909883252D1340CF94E3483652B3 |

All ten hashes were confirmed for the exact DAT name. The "why" field was empty —
no hardware-quirk claim was made, so these stand as general hash-verified
regression titles.

---

## GBC hash + rationale verification

| Title | Platform | Revision | Why it tests accuracy | Emus that failed | CRC32 | MD5 | SHA1 |
|---|---|---|---|---|---|---|---|
| Pokemon Crystal Version | Game Boy Color | Rev 1 | — | — | 3358E30A | 301899B8087289A6436B0A241FBBB474 | F2F52230B536214EF7C9924F483392993E226CFB |
| Toki Tori | Game Boy Color | — | — | — | 0A0F9289 | E1BF59102BCD5E3601F4B24B3E873FD2 | 2025275BB55710594E990AB61CDE622947A2FA8D |
| The Little Mermaid II: Pinball Frenzy | Game Boy Color | — | — | — | 364F9CCD | 7F8C472F3C7BD1EEC56A3BAD10A2E94C | 0940A1A86127CF8228A6A015035D8189218F57DB |
| F1 Championship Season 2000 | Game Boy Color | — | — | — | 5C10315E | 4448C35BEBF32629E23F64C61CC50565 | DC566028E0E86F32D99D5B0846D4B113525BE0A2 |
| NASCAR 2000 | Game Boy Color | — | — | — | 54D90A4C | 42E66DD2C0470D98487EF364E7DAF710 | 37D3FFB5AE9BCF4C1CAB7CECE0A19D6B0B45C3A2 |
| Toy Story Racer | Game Boy Color | — | — | — | D911DD97 | 01A67ED2DC935044BA69EDA42BDDEBF3 | 5DEB31321A86B260AA84CAB5E45A3FA36CE5EFBD |
| Alone in the Dark: The New Nightmare | Game Boy Color | — | — | — | C145C036 | D97055E4A2FD4624FC924C4834ACE35E | A348AEAC500D0D8FDAF90F5277631A026504FF44 |
| Shantae | Game Boy Color | — | — | — | E994B59B | 028C4262DBB49F4FC462A6EB3E514D72 | 520E48C50F6E997FCD841CA368FC9ABC1DBDDEC1 |
| Warriors of Might and Magic | Game Boy Color | — | — | — | EF9F5BEA | F0656CF3AA3D6B539FB1B7DA0FD27617 | 06163ACAD95D6AB87FFCD56F18B37C8C9E37A6EC |
| Wario Land 3 | Game Boy Color | — | — | — | 480D0259 | 16BB3FB83E8CBBF2C4C510B9F50CF4EE | BB7877309834441FD03ADB7FA65738E5D5B2D7BA |
| Wendy: Every Witch Way | Game Boy Color | — | — | — | 4AC6907B | 4E1A5F02CCE49842D4717A8B0CE501F5 | 8CA45ED882C23DF714BDA46B227A857DEDDC899F |

**Caveats.** All eleven hashes (CRC32/MD5/SHA1) were confirmed in gbc.dat against
the exact DAT name, but the "why it tests accuracy" field was empty for every
entry, so the verifier could **not** certify any hardware-quirk rationale
(`rationaleOk = false` for all 11). Note in particular that **Pokemon Crystal
Version** genuinely uses MBC3 + RTC, but because its "why" was blank in this
batch it is recorded as uncertified here — see the special-hardware section above
for the verified RTC entry. The Little Mermaid II hash is the **Rumble Version**
entry.

---

## GB/GBC homebrew & demoscene (PD) — verified absent from No-Intro/libretro DATs (gb.dat & gbc.dat v2026.05.02); all hashes TBD

| Title | Platform | Revision | Why it tests accuracy | Emus that failed | CRC32 | MD5 | SHA1 |
|---|---|---|---|---|---|---|---|
| Demotronic | GBC | TBD | Demotronic by 1.000.000 boys (PD) [almost certainly absent from No-Intro; hash TBD] | TBD | TBD | TBD | TBD |
| GBVideoPlayer | GBC | TBD | GBVideoPlayer (PD) [not a commercial title; not in No-Intro; hash TBD] | TBD | TBD | TBD | TBD |
| GBVideoPlayer 2 | GBC | TBD | GBVideoPlayer 2 (PD) [homebrew tech demo; not in No-Intro; hash TBD] | TBD | TBD | TBD | TBD |
| Mental Respirator | GBC | TBD | Mental Respirator by Phantasy (PD) [demoscene; not in No-Intro; hash TBD] | TBD | TBD | TBD | TBD |
| It Came from Planet Zilog | GBC | TBD | It Came from Planet Zilog by Phantasy (PD) [demoscene; not in No-Intro; hash TBD] | TBD | TBD | TBD | TBD |
| Back to Color | GBC | TBD | Back to Color by AntonioND (PD) [homebrew compo entry; not in No-Intro; hash TBD] | TBD | TBD | TBD | TBD |
| oh! | GB | TBD | Oh! by Snorpung (PD) [DMG demoscene; not in No-Intro; hash TBD] | TBD | TBD | TBD | TBD |
| Is That a Demo in Your Pocket? | GB | TBD | Is That a Demo in Your Pocket by Snorpung (PD) [DMG demoscene; not in No-Intro; hash TBD] | TBD | TBD | TBD | TBD |
| Gejmbåj | GB | TBD | Gejmbaj by Snorpung & Nordloef (PD) [DMG demoscene; not in No-Intro; hash TBD] | TBD | TBD | TBD | TBD |

All nine rationales were accepted as legitimate (`rationaleOk = true`), but none
have a confirmed hash — homebrew and demoscene productions are usually absent
from No-Intro, and a grep of gb.dat / gbc.dat (v2026.05.02) returned no match for
any of these titles or their groups. Hashes are therefore **TBD** until a
canonical release is matched.

---

## Hash provenance

- All CRC32 / MD5 / SHA1 values come from the **No-Intro** datfiles
  **"Nintendo - Game Boy"** (gb.dat) and **"Nintendo - Game Boy Color"**
  (gbc.dat), as redistributed via the **libretro-database** project.
- A value is shown only when it was confidently matched to the exact DAT entry
  named for that title.
- **TBD** means the dump was **not** confidently matched in the DAT. This is
  expected for homebrew and demoscene productions, which are typically not
  cataloged by No-Intro.
- Hashes are printed **uppercase**. Hash your own legally-dumped cartridge and
  compare; a mismatch means you have a different revision/region than the note
  assumes.

## How to use this list

Commercial games have **no automated golden-frame check** in slopgb, so each
category below has a concrete behavioral pass signal a human can watch for.

- **Essential titles** — the emulator should **boot to the title screen** and
  play through normal gameplay with **no documented glitch** (corrupted tiles,
  wrong palette, hangs). Use these as a broad regression smoke test alongside the
  mooneye/game-boy-test-roms battery.

- **Hardware-quirk emulation batch** — boot to title and play; watch for the
  **absence of the rendering/timing glitch** these titles are tracked for. Since
  the specific quirk text was not recorded (see Caveats), treat a clean
  boot-and-play with correct raster/sound as the provisional pass and attach the
  documented quirk before relying on it.

- **Special-hardware titles** — verify the **named peripheral actually
  responds**:
  - *RTC* (Pokemon Crystal/Gold, Harvest Moon GBC, Robopon, JP MBC30 Crystal):
    the in-game clock **advances in real time** and persists across resets;
    time-of-day and daily/seasonal events fire.
  - *Tilt/accelerometer* (Kirby Tilt 'n' Tumble, Command Master): tilting the
    input device (or mapped axes) **moves the on-screen object** proportionally.
  - *Camera* (Game Boy Camera): a live capture source feeds the
    **M64282FP sensor** and produces an image.
  - *Sonar* (Pocket Sonar): the fish-finder sweep renders/updates.
  - *Infrared* (Robopon Sun, JP Pokemon Card GB): IR-gated features
    (Card Pop!, trades) negotiate instead of stalling.
  - *Rumble* (Pokemon Pinball Rumble Version): the rumble motor flag is asserted
    on the expected events.
  - *Mobile Adapter GB* (Net de Get, JP MBC30 Crystal): the Mobile System GB
    handshake proceeds far enough to attempt a download/connection.

- **ROM hash + rationale verification (both batches)** — these are
  **hash-anchored regression titles**; the pass signal is **boots to title and
  plays without a documented glitch**. No specific quirk was certified for the
  GBC-only batch (see its Caveats), so do not treat them as quirk oracles.

- **Homebrew & demoscene (PD)** — the pass signal is that the **demo effect
  renders correctly**: raster/scanline splits land on the right line, FMV
  (GBVideoPlayer 1/2) plays back at the intended rate, and music/visual sync
  holds. These productions deliberately stress cycle-exact timing, so a wrong
  scanline or torn effect is an immediate fail. Hashes are TBD, so pin the exact
  build you test by recording its hash yourself.
