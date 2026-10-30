# The papers standard: conformance suite

The conformance suite is in papers itself (`design/cases`, perrette/papers#206), with the
specification it checks (`design/*.md`). `spec/papers` is a git submodule of perrette/papers,
pinned at the commit papers-rs follows: **papers v3 at e179067**. Moving the pin is catching
up with papers: check out the new commit in `spec/papers`, port what changed, and commit the
submodule with the port.

```
git submodule update --init spec/papers                     # once, after a clone
cargo build -p papers-rs-cli
python3 spec/papers/design/cases/run.py --papers target/debug/papers-rs --contract-only
python3 spec/papers/design/cases/run.py --papers target/debug/papers-rs 'file/*'
```

`spec/papers/design/cases/README.md` tells how a case is written, run and recorded, and
which outputs are Contract and which Free. Cases are added and recorded upstream, by running
papers (`record.py`).
