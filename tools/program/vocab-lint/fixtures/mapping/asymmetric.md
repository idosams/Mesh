# A mapping that only reads one way

<!-- vocab-lint:mapping-forward -->

| Internal state | User-facing wording |
|---|---|
| Actor working head | Their work |
| Canonical head | Shared version |
| Durable actor checkpoint | Saved privately |
| Replicated actor checkpoint | Available to team |
| Review bundle | Ready for review |
| Publish operation | Approve to shared version |
| Conflict | Needs attention |
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
| Needs attention | Conflict |
