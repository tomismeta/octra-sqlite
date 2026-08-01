# Mainnet Proof

`0.6.3` was proven against the Octra Vitals mainnet Lab DB Circle before
publishing to crates.io.

Vitals mainnet upgrade snapshot:

```text
operator: octra-vitals
client candidate: octra-sqlite 0.6.3
engine: SQLite 3.53.4
program version: 5
read_ready: true
write_ready: true
engine_current: true
upgrade_needed: false
gateway: active
updater timer: active
lab mirror trigger: active
30d history: served from the SQLite mirror path
latest mirror watermark: complete through snapshot #3305
recurring mirror write cost: 1000 OU
candidate promoted to shared global binary: false
upgrade tx: 8275695e2ba6c8e190d20d38940ef47c31418ade7fcd8dd713adc48db9b64ad9
upgrade tx url: https://octrascan.io/tx.html?hash=8275695e2ba6c8e190d20d38940ef47c31418ade7fcd8dd713adc48db9b64ad9
```

The recurring mirror write cost matters operationally: normal application
writes remained ordinary `circle_call` writes around `1000` OU, while the
one-time Circle program upgrade used the higher program-update budget.
