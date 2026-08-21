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
