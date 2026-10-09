# Wiki protocol v1 fixtures

These JSON fixtures are the language-neutral compatibility harness for a
`WikiProvider` adapter. They contain no Rust-specific representation.

- `requests.json` covers every v1 operation.
- `responses.json` pins successful negotiation, retrieval, context,
  curation, and error envelopes.

Each top-level key is a fixture name; its value is the exact object sent over
the one-shot CLI bridge. IDs and timestamps are fixed examples. Consumers
must ignore additive fields they do not understand, as required by protocol
v1.

The Athenaeum test suite deserializes and losslessly reserializes every value
through the production protocol types.

## This copy is generated

`requests.json` and `responses.json` here are generated, not hand-edited, by
`athenaeum_validation/generate_app_fixtures.py`:

- the original fixtures are kept as they were;
- the additive methods published in `athenaeum_protocol/fixtures/bridge/v1` are copied from there;
- `capabilities_ok` is captured verbatim from the real engine's bridge output, so it is the
  product's own answer rather than a hand-written expectation.

Regenerate after a protocol change:

```bash
cd ../athenaeum_validation
ATHENAEUM_BIN=../athenaeum/target/debug/athenaeum python3 generate_app_fixtures.py
python3 generate_app_fixtures.py --check   # non-zero when this copy is stale
python3 verify_all.py --only engine        # the preserved test that consumes it
```

The published protocol has since added optional `note`/`confidence` fields to relationship and
traversal payloads (`related_ok`, `navigate_ok`, `path_ok`). They are copied here only once the
engine's own types can round-trip them; `python3 ../athenaeum_validation/verify_all.py --only engine`
is the gate that proves it, and `protocol_fixtures.rs` fails loudly if this copy drifts.
