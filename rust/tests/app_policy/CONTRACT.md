The shared `newt_core::app` module translates the pure routing, dispatch, wake
destination and packed graph-navigation policy of Newt `bc9ffda5` `src/app.cpp`.
All twelve transported state fields and the derived device page are compared
with the original C++ implementation. This module performs no drawing, settings
commit, sleep instruction or device I/O.

Rust's nominal enums accept the declared variants. Every valid u8/u32/boolean
field combination is in scope, including unusual combinations of flags. Other
C++ enum representations may have defined fallback behavior but are outside this
Rust caller domain; they are not described as necessarily undefined C++.
Persistence/native adapters must validate representation before constructing enums.

Wake preserves the complete state when already awake. Sleep destination, settings
notice precedence and hidden About-counter changes follow the pinned branches.
`FirstNewt` has device-page value3, independently of the active screen. Header
discriminants are frozen with the oracle; expected values are authored separately
from both implementations. Empty/malformed batches must be rejected silently and
atomically before any result. Clean, admitted whole-state differences establish
semantic mutation detection; build errors, diagnostics and panics do not.

Host comparisons and retained RV32 borrowed functions establish neither physical
sleep/wake behavior nor startup restoration wiring. Cold RAM loss, durable record
restore, battery/USB power and real firmware integration remain separate acceptance.
