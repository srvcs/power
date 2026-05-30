# srvcs-power

The exponentiation orchestrator of the srvcs.cloud distributed standard library.

Its single concern: **`base` raised to `exp`**. It does no arithmetic of its
own. It computes the power as a counted loop of repeated multiplications: it
seeds an accumulator at `1` and asks [`srvcs-multiply`](https://github.com/srvcs/multiply)
for `acc * base`, `exp` times.

```text
acc = 1
for _ in 0..exp:
    acc = multiply(acc, base)
```

As a consequence `power(base, 0) == 1` makes no dependency calls at all. A
negative exponent is undefined over the integers and is rejected with `422`
before any call is made.

## API

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/` | Service identity, concern, and dependency list |
| `POST` | `/` | Compute `base` raised to `exp` |
| `GET` | `/healthz` `/readyz` `/metrics` `/openapi.json` | srvcs service standard surface |

```sh
curl -s -X POST localhost:8080/ -H 'content-type: application/json' -d '{"base": 2, "exp": 10}'
# {"base":2,"exp":10,"result":1024}
```

Responses:

- `200 {"base": b, "exp": e, "result": r}` — evaluated.
- `422 {"error": "negative exponent"}` — `exp` is negative.
- `422` — an operand was rejected by `srvcs-multiply` (forwarded).
- `503` — the dependency is unavailable.

## Dependencies

- [`srvcs-multiply`](https://github.com/srvcs/multiply)

A single request fans out into `exp` sequential calls to `srvcs-multiply`, each
folding the running accumulator with `base`.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `SRVCS_BIND_ADDR` | `0.0.0.0:8080` | Bind address |
| `SRVCS_MULTIPLY_URL` | `http://127.0.0.1:8086` | Base URL of `srvcs-multiply` |
| `SRVCS_ENV` | `development` | Environment label for logs |
| `RUST_LOG` | `info,tower_http=info` | Tracing filter |

## Local checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Orchestration tests stand up an in-process mock `srvcs-multiply` that actually
computes `a * b`, so the counted-loop fold is genuinely exercised. They cover
the happy path (`power(2, 10) == 1024`, `power(5, 0) == 1`, `power(3, 3) == 27`),
the negative-exponent rejection (`422`), and a degraded dependency (`503`). See
[`srvcs/platform`](https://github.com/srvcs/platform) for the shared standard.

> Note: the `cargoHash` in `flake.nix` is inherited from the template and must be
> refreshed with a `nix build` before the Nix gates pass.
