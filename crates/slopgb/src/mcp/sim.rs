//! The `simulate` what-if fork: a cloned machine advanced cooperatively on the
//! UI thread — one bounded slice per [`super::Mcp::pump`] — so a long run neither
//! freezes the UI nor blocks the 5 s MCP reply window. Golden-safe: only the
//! clone ever steps; the live machine is never advanced.
//!
//! `simulate` clones the live machine (a full independent GB incl. VRAM),
//! optionally rewinds it to a savestate, overlays a memdump file, sets PC, and
//! registers the fork — returning a job id at once. `sim-result` polls it: still
//! running, or the stop-code + registers + output-range dump.

use slopgb_core::GameBoy;

use super::Mcp;
use super::addr::{self, Addr};
use super::tools::{self, ToolResult};

/// Instructions to advance the fork per pump. The calibration knob: larger
/// finishes a run in fewer pumps but hitches the UI more. ~15k is roughly one
/// frame's worth (a frame is ~17.5k M-cycles), so the fork runs near real time
/// and each wake adds about one extra frame of work — playback stays smooth.
const SIM_SLICE: u64 = 15_000;
/// Hard ceiling on a run's instruction budget, so a runaway `budget` argument
/// can't keep a fork alive indefinitely.
const MAX_BUDGET: u64 = 100_000_000;

/// Parsed `simulate` arguments (raw strings from the MCP call). Addresses take
/// the tools' `AAAA`/`BB:AAAA` form; `start`/`end` are bare hex; `budget` is a
/// decimal instruction count.
pub struct SimArgs {
    pub memdump: String,
    pub in_from: String,
    pub in_to: String,
    pub out_from: String,
    pub out_to: String,
    pub start: String,
    pub end: Option<String>,
    pub budget: String,
    pub savestate: Option<String>,
}

/// Why a fork stopped.
enum SimStop {
    ReachedEnd,
    Runaway,
    TimedOut,
}

impl SimStop {
    fn label(&self) -> &'static str {
        match self {
            SimStop::ReachedEnd => "reached_end",
            SimStop::Runaway => "runaway",
            SimStop::TimedOut => "timed_out",
        }
    }
}

/// A finished fork's captured result, held until `sim-result` reads it.
struct SimOutcome {
    stop: SimStop,
    regs: String,
    dump: String,
}

/// An in-flight (or just-finished) `simulate` fork.
pub struct SimJob {
    id: u64,
    gb: GameBoy,
    end: Option<u16>,
    remaining: u64,
    out_from: Addr,
    out_to: Addr,
    /// `Some` once the fork has stopped; the result waits here for `sim-result`.
    done: Option<SimOutcome>,
}

impl Mcp {
    /// Start a `simulate` fork. Rejects a second start while one is still
    /// running; a finished-but-unpolled fork is replaced.
    pub(super) fn start_sim(&mut self, gb: &GameBoy, a: &SimArgs) -> Result<ToolResult, String> {
        if let Some(job) = &self.sim {
            if job.done.is_none() {
                return Err(format!(
                    "simulation job {} is still running; poll `sim-result` or wait",
                    job.id
                ));
            }
        }
        let (in_from, in_to) = addr::parse_range(&a.in_from, &a.in_to)?;
        let (out_from, out_to) = addr::parse_range(&a.out_from, &a.out_to)?;
        let start = parse_addr16(&a.start)?;
        let end = a.end.as_deref().map(parse_addr16).transpose()?;
        let budget = parse_budget(&a.budget)?.min(MAX_BUDGET);

        let bytes = std::fs::read(&a.memdump).map_err(|e| format!("read '{}': {e}", a.memdump))?;
        let want = usize::from(in_to.addr - in_from.addr) + 1;
        if bytes.len() != want {
            return Err(format!(
                "memdump '{}' is {} bytes but the dump_in range is {want} bytes",
                a.memdump,
                bytes.len()
            ));
        }

        // Fork now: a clone is a full independent machine (VRAM/PPU/banking/ROM),
        // so the live machine is never touched — this stays golden-safe.
        let mut sim = gb.clone();
        if let Some(f) = &a.savestate {
            let sb = std::fs::read(f).map_err(|e| format!("read '{f}': {e}"))?;
            sim.load_state(&sb)
                .map_err(|e| format!("load savestate '{f}': {e:?}"))?;
        }
        for (i, byte) in bytes.iter().enumerate() {
            sim.debug_write_banked(in_from.bank, in_from.addr.wrapping_add(i as u16), *byte);
        }
        sim.debug_set_pc(start);

        let id = self.next_sim_id;
        self.next_sim_id += 1;
        self.sim = Some(SimJob {
            id,
            gb: sim,
            end,
            remaining: budget,
            out_from,
            out_to,
            done: None,
        });
        Ok(ToolResult::Text(format!(
            "simulation job {id} started ({budget} instruction budget); poll `sim-result` with job={id}"
        )))
    }

    /// Advance the in-flight fork by one slice. No-op when nothing runs or the
    /// fork already finished (its result waits for `sim-result`).
    pub(super) fn advance_sim(&mut self) {
        let Some(job) = &mut self.sim else { return };
        if job.done.is_some() {
            return;
        }
        let slice = SIM_SLICE.min(job.remaining);
        let bps: &[u16] = match &job.end {
            Some(e) => std::slice::from_ref(e),
            None => &[],
        };
        let hit = job.gb.run_until_breakpoint(bps, slice);
        job.remaining -= slice;

        // Precedence: reaching `end` wins over a lockup that happened this slice,
        // which wins over exhausting the budget.
        let stop = if hit.is_some() {
            Some(SimStop::ReachedEnd)
        } else if job.gb.debug_undefined_hit() {
            Some(SimStop::Runaway)
        } else if job.remaining == 0 {
            Some(SimStop::TimedOut)
        } else {
            None
        };
        if let Some(stop) = stop {
            job.done = Some(SimOutcome {
                regs: tools::registers(&job.gb),
                dump: tools::peek_range(&job.gb, job.out_from, job.out_to),
                stop,
            });
        }
    }

    /// Poll a `simulate` job by id.
    pub(super) fn sim_result(&self, job: u64) -> Result<ToolResult, String> {
        let Some(s) = &self.sim else {
            return Err(format!("no simulation job {job} (none has been started)"));
        };
        if s.id != job {
            return Err(format!(
                "no simulation job {job} (the current job is {})",
                s.id
            ));
        }
        match &s.done {
            None => Ok(ToolResult::Text(format!(
                "job {job} still running ({} instructions of budget left)",
                s.remaining
            ))),
            Some(o) => Ok(ToolResult::Text(format!(
                "stop: {}\n{}\n{}",
                o.stop.label(),
                o.regs,
                o.dump
            ))),
        }
    }
}

/// A bare-hex 16-bit address (`start`/`end`), the CPU-space form with no bank.
fn parse_addr16(s: &str) -> Result<u16, String> {
    u16::from_str_radix(s.trim(), 16)
        .map_err(|_| format!("bad address '{s}' (want hex, e.g. C000)"))
}

fn parse_budget(s: &str) -> Result<u64, String> {
    s.trim()
        .parse::<u64>()
        .map_err(|_| format!("bad budget '{s}' (want a decimal instruction count)"))
}

#[cfg(test)]
#[path = "sim_tests.rs"]
mod tests;
