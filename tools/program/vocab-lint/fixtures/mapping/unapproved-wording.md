# A mapping that is a perfect inverse of itself and still contradicts the six status words

Both tables agree with each other, so the symmetry check passes. They agree on a word that is
not one of the six and not a declared object wording — which is exactly how "Needs review" came
to sit beside "Needs attention" and "Ready for review" in the PRD without any rule noticing.

<!-- vocab-lint:mapping-forward -->

| Internal state | User-facing wording |
|---|---|
| Actor working head | Their work |
| Canonical head | Shared version |
| Durable actor checkpoint | Saved privately |
| Replicated actor checkpoint | Available to team |
| Review bundle | Ready for review |
| Publish operation | Approve to shared version |
| Conflict | Needs review |
| Historical state | Earlier version |

<!-- vocab-lint:mapping-reverse -->

| User-facing wording | Internal state |
|---|---|
| Their work | Actor working head |
| Shared version | Canonical head |
| Saved privately | Durable actor checkpoint |
| Available to team | Replicated actor checkpoint |
| Ready for review | Review bundle |
| Approve to shared version | Publish operation |
| Needs review | Conflict |
| Earlier version | Historical state |
