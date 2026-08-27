# Perturbation Workbench

Run the iterative perturbation protocol of *Adversarial Assignment
Perturbation* (Gilson, Tabarsi & Barnes, AIED 2026) in the browser, and export
an assignment and a run manifest a collaborator can verify.

Under construction — the browser app arrives with milestone 4. The protocol
engine is in `crates/core` and runs under `cargo test` today.

```sh
make test                       # the protocol test suite (native, no browser needed)
make verify RUN=run-manifest.json   # audit a run from the command line
make help                       # every command
```

## Licence

MIT.
