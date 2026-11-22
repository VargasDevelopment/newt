# Books browsing with pending reader recovery

Home Books previously redirected into the selected reader whenever an exact
checkpoint was pending. A failed progress save after Back can create that state.
The existing route regression required the redirect, although Home's shared app
state correctly assigned Books to Library and Read to Reading.

Books now always closes the reader workspace, scans the catalog and shows Library.
It preserves the selected full-SHA identity, exact pending page, settings and
statistics. While recovery is pending, Library Open stops before its selection
callback and displays `Position kept. Use Read on Home to resume.` Home Read
remains the route that attempts the exact recovery. This prevents prospective
selection of another book from replacing an unacknowledged checkpoint.

The production change is confined to `firmware/library_reader.h`. It changes no
durable format, NVS startup guard, admission rule, sleep ownership or book identity
validation. Catalog scanning releases source/session ownership without clearing
the retained selection or independent pending checkpoint.

The generic route regression verifies Books and Read in both orientations,
including failed storage and heap/parser retry. The SD regression uses authored
distinct EPUBs, the actual selected/progress stores, a separate Home intent store,
and extracted production selection, reader-exit, publication and boot code. It
checks healthy selection, blocked replacement work, exact recovery and reset/sleep
boundaries. Independent copied-source negative controls restore the historical
Books redirect and remove only the Library Open guard; each must fail at its own
runtime boundary. The complete host gate runs these controls.

The owner's Books/Read observation is consistent with the old pending redirect.
The device's actual pending checkpoint and save-failure cause were not observed
at the time, so the host reproduction does not establish that physical cause.
The installed source's routing and checkpoint paths match the frozen warm-read
candidate. Physical route/recovery confirmation remains a separate acceptance
check after a final gated, preserving install.

Efficiency work must remain generic. No library title, filename, known digest or
known-book selector may receive privileged behavior. Ordinary per-book full-SHA
cache validation remains allowed. Host parity and read-call counts do not prove
physical latency or that all slow or timed-out books are fixed.
