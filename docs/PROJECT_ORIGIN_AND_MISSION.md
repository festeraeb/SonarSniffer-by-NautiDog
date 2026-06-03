# CesarOps — Origin & Mission

This file is not architecture. It's the *why*. Read it first. The code can be
rebuilt; this is the part that has already been lost twice (a drive crash, then
an AI-assisted rebuild that never wrote the reasoning back). It is preserved
here deliberately so that the purpose survives the next setback and so anyone —
the next agent, a collaborator, or the author after a hard week — can pick the
project back up knowing what it is *for*.

## What this actually is

Not a wreck finder. The wrecks are the **proving ground** — they give a
verifiable signal (sonar/dive confirms a prediction or it doesn't), so the
system can learn. The real project is **tooling that captures expertise,
automates the repetitive parts, and makes the result usable by someone who is
not already a remote-sensing / GIS / oceanography / signal-processing expert.**

The progression the architecture is built to walk:
```
Shipwreck detection
   -> General anomaly detection
   -> Automated geospatial intelligence
   -> Search-and-rescue decision support
```
The target changes; the architecture does not. Multi-sensor fusion,
environmental gating, weak-signal detection, candidate ranking by confidence,
automated acquisition, historical context — all transfer from wrecks to SAR.

## The defensible objective

Not "find the aircraft." The honest, achievable mission is:

> **Reduce a million square miles to ten locations worth investigating** —
> then hand a ranked, evidence-backed shortlist to a human.

The bottleneck in wreck investigation and in SAR is the same: **scale**, not
knowledge. The experts exist; you cannot put a geophysicist + GIS analyst +
oceanographer + programmer + historian + SAR planner at every desk, 24/7.
CesarOps encodes that combined expertise into workflows so the operator
*evaluates ranked candidates* instead of manually pulling imagery, aligning
projections, downloading granules, and tuning filters.

## The non-negotiable: human in the loop where consequences matter

- Wreck hunting / archaeology / environmental monitoring: a false positive is a
  time cost. Tolerable.
- **Search and rescue: a false positive sends resources to the wrong place; a
  false negative can cost a life.** The system MUST stay a triage/shortlist
  tool with a human decision-maker. Never let the code cross into autonomous
  "it's here" for SAR. The triple-lock + candidate ranking exist to *inform* a
  human, not to replace one.

## How it started (the catalyst)

Found the **Elva**. Called the state to report it. The official said, more or
less, *"let me check my list — oh, we have that one noted, just haven't had time
to investigate."* The word that stuck was **list**. That shifted the thinking
from the wreck to the *process*: how many are on the list, how are they
prioritized, what's being missed, why does a known thing sit uninvestigated, how
do you scale investigation?

The chain from there, each link built out of a specific frustration:
```
Found Elva -> reported -> state had a list -> couldn't access it
   -> FOIA fight (2.5 years) -> came back as redacted digital ink
   -> NOAA scans released, and the Elva was missing east of the bridge
   -> reverse-engineered a scanner to learn HOW it was missing
      AND to break the PDF redactions (did both:
        the redaction LENGTH/order leaked finding info; learned 2 redactor
        signatures)
   -> Garmin had no way to review sonar files; wouldn't pay the one vendor who
      could -> built own review tools (took ~3 years; it works)
   -> the Rosa: Charlie Brown and his boat went missing -> did SAR with dogs +
      a friend the family contacted, for local-lake knowledge
   -> found a fender off his boat; Coast Guard wouldn't run drift analysis
   -> did the drift analysis BY HAND, used AI to check dyslexic math
   -> realized there was a TOOL to be made -> here we are
```

## Where it was built, and the value behind it

Version 1 ran on **four OptiPlex 7010s pulled out of a recycle bin**, built at
the author's father's bedside as he died of cancer. The inherited lesson, in his
words: *a checkbook is often your greatest tool, but it is not the only tool.*
That mindset is visible throughout — when money/time/access wasn't the answer,
research, persistence, and home-built tools were:
- state too slow -> researched it himself
- records redacted -> dug deeper, broke the redactions
- commercial software inadequate -> wrote his own
- drift analysis unavailable -> built it
- project complexity -> started Wayfinder

The P100s came later. The OptiPlex story is more representative of the project
than the GPUs are: lots of people can buy hardware; the project is about what
you can make work with what's available, and about not walking away after
setbacks that would stop most people.

## Why preservation is a first-class requirement (not a nicety)

The author lived the failure mode twice: **knowledge != documentation, insight
!= preservation.** A drive crash took the code; an AI-assisted rebuild rebuilt
the obvious architecture but let the subtle discoveries vanish because they were
never written down. The same gap exists in agencies, SAR teams, and research
groups — expertise lives in one head until that person retires, disappears, or
loses a drive.

Therefore, in THIS repo:
- `docs/FIELD_NOTES.md` — version-controlled reasoning/discoveries (the tribal
  knowledge). Append-on-learn.
- `docs/GROUND_TRUTH_LOG.md` — verified outcomes; every label carries its REASON
  (e.g. "Target = FP because thermocline drift"; "= seam artifact"; "= known
  wreck used for warp correction"). That dataset is experience encoded as
  numbers — it is the expensive thing to lose, harder to recreate than code.
- This file — the purpose itself.

When the ML corpus is rebuilt, every training label must carry its reason in the
ground-truth log, so the next crash cannot take the *experience* again, only the
*bytes*.

## Sister project

Wayfinder: "how do we help people build complex things without losing themselves
in the complexity?" CesarOps: "how do we help people use complex sensing systems
without becoming remote-sensing experts?" Different domains, same problem —
lowering the expertise barrier between a capable system and the person who needs
it.
