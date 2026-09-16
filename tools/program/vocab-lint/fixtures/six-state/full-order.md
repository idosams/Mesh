# A status table with all six words in exactly reverse order

Six rows, so nothing is "missing" — the only thing wrong is the order. The expectations pin the
exact message for every one of the six positions, which is what makes this fixture sensitive to
*any* permutation of `APPROVED_STATUS`: change the order of two words and at least one position's
expected value changes with it.

A two-row fixture cannot do that. `wrong-order.md` has only rows 1 and 2, so a swap of the first
two status words leaves its loose expectation still matching and the suite still green.

<!-- vocab-lint:six-state -->

| Status | Shown when |
|---|---|
| Approved | The change reached the shared version. |
| Needs attention | Someone has to decide something. |
| Ready for review | There is an exact change waiting for a person. |
| Available to team | Peers can open it, read-only. |
| Saved privately | It survived; nobody else has it. |
| Working | An actor is changing files right now. |
