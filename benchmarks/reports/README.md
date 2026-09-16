# benchmarks/reports

Published result rows. A row here is a **deliberate commit**, never a side effect of measuring:
`benchmarks/runners/run.sh` writes out of tree by default, and publishing means appending one
line here and committing it. `benchmarks/runners/README.md` says why, and `mesh-bench fields`
lists what a row must carry.

| File | What it is | Task |
|---|---|---|
| `storage-footprint.jsonl` | the CAS and index footprint figures behind `benchmarks/budgets/storage.md` | `01KZE6CMABVAV37FPJ3ZTPP8CJ` |
| `storage-amplification.jsonl` | what a week of agent work leaves on disk — the plan §12.4 storage row, measured over W7 | `01KZE5FDN0NPGJ6NQ1NBYRFVH0` |

**Mostly unstarted, and visibly so.** Two files is not a benchmark programme. The rest of E08
fills this directory; find what is outstanding with:

```bash
See the public GitHub issue tracker
```

Reproduce any row from the row alone — clone the `repository.remote` it names, check out its
`repository.commit`, and run its `invocation`. `benchmarks/runners/README.md` has the recipe and
`mesh-bench compare` has the variance band.
